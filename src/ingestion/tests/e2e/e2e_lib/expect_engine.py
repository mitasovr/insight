"""Evaluate `expect` rules against a batch response.

Implements `cpt-bronze-to-api-e2e-algo-yaml-eval-expect`
(DoD `cpt-bronze-to-api-e2e-dod-yaml-expect-engine`).

Each rule:
  in:     select the batch result by request id (omit when one query)
  find:   Mongo-style selector → exactly one row of result.items  (binds `it`)
  then ONE of:
    equal:  subset equality on the matched row (explicit null supported)
    assert: CEL boolean over bindings `it`, `items`, `result`, `results`, `status`
"""

from __future__ import annotations

import re
from typing import Any

import celpy


class ExpectError(AssertionError):
    """A failing expect rule. Message names the case, rule and the mismatch."""


# ---------------------------------------------------------------------------
# Mongo-style selector
# ---------------------------------------------------------------------------

_FIND_OPS = {"$eq", "$ne", "$gt", "$gte", "$lt", "$lte", "$in", "$regex", "$exists"}


def _match_value(actual: Any, cond: Any) -> bool:
    if isinstance(cond, dict) and any(k.startswith("$") for k in cond):
        for op, operand in cond.items():
            if op not in _FIND_OPS:
                raise ExpectError(
                    f"unknown find operator {op!r} (supported: {sorted(_FIND_OPS)})"
                )
            if op == "$eq" and not (actual == operand):
                return False
            elif op == "$ne" and not (actual != operand):
                return False
            elif op == "$gt" and not (actual is not None and actual > operand):
                return False
            elif op == "$gte" and not (actual is not None and actual >= operand):
                return False
            elif op == "$lt" and not (actual is not None and actual < operand):
                return False
            elif op == "$lte" and not (actual is not None and actual <= operand):
                return False
            elif op == "$in" and actual not in operand:
                return False
            elif op == "$regex" and (actual is None or not re.search(operand, str(actual))):
                return False
            elif op == "$exists" and ((actual is not None) != bool(operand)):
                return False
        return True
    return actual == cond


def _find(items: list[dict], selector: dict) -> list[dict]:
    return [it for it in items if all(_match_value(it.get(f), c) for f, c in selector.items())]


# ---------------------------------------------------------------------------
# CEL
# ---------------------------------------------------------------------------

_CEL_ENV = celpy.Environment()


def _floatify(obj: Any) -> Any:
    """Coerce JSON numbers to float so CEL comparisons on metric values work
    regardless of whether the API serialized e.g. `40` or `40.0` (CEL is strictly
    typed and will not compare IntType to DoubleType). `bool` is left intact."""
    if isinstance(obj, bool):
        return obj
    if isinstance(obj, int):
        return float(obj)
    if isinstance(obj, dict):
        return {k: _floatify(v) for k, v in obj.items()}
    if isinstance(obj, list):
        return [_floatify(v) for v in obj]
    return obj


def _eval_cel(expr: str, bindings: dict) -> bool:
    ast = _CEL_ENV.compile(expr)
    prog = _CEL_ENV.program(ast)
    # Float-normalize the response-bound values (it/items/result/results); keep
    # `status` an int so `status == 200` and `size(items) == N` stay int-typed.
    activation = {}
    for k, v in bindings.items():
        activation[k] = celpy.json_to_cel(v if k == "status" else _floatify(v))
    result = prog.evaluate(activation)
    return bool(result)


# ---------------------------------------------------------------------------
# Rule evaluation
# ---------------------------------------------------------------------------

def _select_result(rule: dict, results: list[dict], where: str) -> dict | None:
    if "in" in rule:
        wanted = rule["in"]
        for r in results:
            if r.get("id") == wanted:
                return r
        raise ExpectError(f"{where}: no batch result with id '{wanted}' (have {[r.get('id') for r in results]})")
    if len(results) == 1:
        return results[0]
    return None


def evaluate_case(case: dict, batch: dict, http_status: int) -> None:
    """Run every rule of `case`. Raise ExpectError on the first failure."""
    name = case.get("name", "<unnamed>")
    results = batch.get("results", []) if isinstance(batch, dict) else []

    for i, rule in enumerate(case.get("expect", [])):
        where = f"case '{name}' rule #{i}"
        result = _select_result(rule, results, where)
        items = result.get("items", []) if result else []

        it = None
        if "find" in rule:
            matches = _find(items, rule["find"])
            if len(matches) != 1:
                raise ExpectError(
                    f"{where}: find {rule['find']} matched {len(matches)} rows (expected exactly 1)"
                )
            it = matches[0]

        if "equal" in rule:
            if it is None:
                raise ExpectError(f"{where}: `equal` requires a `find` that selects one row")
            for field, exp in rule["equal"].items():
                got = it.get(field)
                if got != exp:
                    raise ExpectError(f"{where}: {field}: expected {exp!r}, got {got!r}")
        elif "assert" in rule:
            # CANONICAL source of the CEL `assert` bindings (documented in the
            # yaml-rig FEATURE, DESIGN expect-engine component, README, and the
            # /metric-e2e-test skill). `it` is None unless this rule had a `find`.
            bindings = {
                "it": it,
                "items": items,
                "result": result,
                "results": results,
                "status": http_status,
            }
            try:
                ok = _eval_cel(rule["assert"], bindings)
            except Exception as e:  # noqa: BLE001 - surface CEL errors as rule failures
                raise ExpectError(f"{where}: CEL error in {rule['assert']!r}: {e}") from e
            if not ok:
                raise ExpectError(f"{where}: assert failed: {rule['assert']}")
        else:
            raise ExpectError(f"{where}: rule must have `equal` or `assert`")
