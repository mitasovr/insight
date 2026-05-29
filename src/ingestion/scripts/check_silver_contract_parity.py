#!/usr/bin/env python3
"""Silver-contract consistency gate (connectors <-> silver class).

Catches the "schema drift between connectors" class of bug *at PR time*,
without a warehouse:

    Connectors (e.g. github, bitbucket-cloud) feed one silver class via
    `union_by_tag('silver:class_git_commits')` -> `SELECT * ... UNION ALL`.
    A dev adds/removes a column in one connector's staging model and forgets
    the others, or drifts from what the silver class (and its downstream
    facts/metrics) expect. On a cluster that materialises the lagging connector
    the UNION ALL fails at runtime (ClickHouse `Code: 386 NO_COMMON_TYPE` or a
    column-count mismatch). PR CI stays green because it never materialises
    every connector at once.

This gate enforces a single source of truth per class: the silver class model's
own contract. Every connector tagged `silver:<class>` MUST emit exactly the
columns the silver model `<class>` declares (name + data_type). That guarantees,
transitively, that connectors agree with each other AND that the silver layer
(hence the facts/metrics reading it) gets exactly what it expects.

    silver contract: class_git_commits   (what consumers expect)
        == github__commits
        == bitbucket_cloud__commits
        == github_v2__commits             (the moment it ships)

Convention: the silver class model for tag `silver:<class>` is the model named
`<class>` (e.g. tag `silver:class_git_commits` -> model `class_git_commits`).
If that silver model has no contract yet, the gate falls back to comparing
connectors against each other and warns that the silver contract is missing.

Source of truth is the dbt manifest (`dbt parse` -> target/manifest.json), which
is fully offline.

Usage:
    dbt parse  # produces target/manifest.json, no DB connection needed
    python scripts/check_silver_contract_parity.py \
        --manifest src/ingestion/dbt/target/manifest.json

Exit code 0 = all silver classes consistent. 1 = drift found (CI fails).
"""

from __future__ import annotations

import argparse
import json
import sys
from collections import defaultdict
from pathlib import Path

SILVER_TAG_PREFIX = "silver:"


def load_manifest(path: Path) -> dict:
    if not path.exists():
        sys.exit(
            f"manifest not found: {path}\n"
            f"Run `dbt parse` first (offline, no warehouse needed)."
        )
    with path.open(encoding="utf-8") as fh:
        return json.load(fh)


def _columns(node: dict) -> dict[str, str]:
    return {
        name: (col.get("data_type") or "<no data_type>")
        for name, col in node.get("columns", {}).items()
    }


def index_manifest(manifest: dict):
    """Return (connectors_by_tag, columns_by_model_name).

    connectors_by_tag: tag -> {model_name -> {column -> data_type}} for models
        carrying a `silver:*` tag (the per-connector staging models).
    columns_by_model_name: every model name -> {column -> data_type}; used to
        resolve the silver class model that anchors each tag.
    """
    connectors_by_tag: dict[str, dict[str, dict[str, str]]] = defaultdict(dict)
    columns_by_model_name: dict[str, dict[str, str]] = {}
    for node in manifest.get("nodes", {}).values():
        if node.get("resource_type") != "model":
            continue
        name = node["name"]
        cols = _columns(node)
        columns_by_model_name[name] = cols
        for tag in node.get("tags", []):
            if tag.startswith(SILVER_TAG_PREFIX):
                connectors_by_tag[tag][name] = cols
    return connectors_by_tag, columns_by_model_name


def compare_to_reference(
    reference: dict[str, str], models: dict[str, dict[str, str]]
) -> list[str]:
    """Compare every connector model against a reference column set."""
    problems: list[str] = []
    ref_cols = set(reference)
    for model in sorted(models):
        cols = models[model]
        if not cols:
            problems.append(
                f"{model}: no typed contract (add `contract: enforced: true` "
                f"+ typed columns to its schema.yml)"
            )
            continue
        missing = sorted(ref_cols - set(cols))
        extra = sorted(set(cols) - ref_cols)
        if missing:
            problems.append(f"{model}: MISSING columns {missing}")
        if extra:
            problems.append(f"{model}: UNEXPECTED columns {extra} (not in silver contract)")
        for column in sorted(ref_cols & set(cols)):
            if cols[column] != reference[column]:
                problems.append(
                    f"{model}.{column}: type {cols[column]} != silver {reference[column]}"
                )
    return problems


def compare_pairwise(models: dict[str, dict[str, str]]) -> list[str]:
    """Fallback when the silver class has no contract: connectors vs each other."""
    problems: list[str] = []
    contracted = {m: c for m, c in models.items() if c}
    uncontracted = sorted(m for m, c in models.items() if not c)
    if uncontracted:
        problems.append("models without a typed contract: " + ", ".join(uncontracted))
    if len(contracted) < 2:
        return problems
    all_columns: set[str] = set().union(*(set(c) for c in contracted.values()))
    for model in sorted(contracted):
        missing = sorted(all_columns - set(contracted[model]))
        if missing:
            problems.append(f"{model}: MISSING columns {missing}")
    for column in sorted(all_columns):
        types = {m: c[column] for m, c in contracted.items() if column in c}
        if len(set(types.values())) > 1:
            rendered = ", ".join(f"{m}={t}" for m, t in sorted(types.items()))
            problems.append(f"column `{column}` has conflicting types: {rendered}")
    return problems


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--manifest",
        type=Path,
        default=Path("src/ingestion/dbt/target/manifest.json"),
        help="path to dbt manifest.json (run `dbt parse` to produce it)",
    )
    args = parser.parse_args()

    connectors_by_tag, columns_by_model_name = index_manifest(load_manifest(args.manifest))
    if not connectors_by_tag:
        print("No `silver:*`-tagged models found — nothing to check.")
        return 0

    failed = False
    for tag in sorted(connectors_by_tag):
        connectors = connectors_by_tag[tag]
        silver_model = tag[len(SILVER_TAG_PREFIX):]  # e.g. "class_git_commits"
        silver_cols = columns_by_model_name.get(silver_model)
        label = f"{tag}  ({len(connectors)} connector(s): {', '.join(sorted(connectors))})"

        if silver_cols:
            problems = compare_to_reference(silver_cols, connectors)
            mode = f"vs silver contract `{silver_model}`"
        else:
            problems = compare_pairwise(connectors)
            mode = "connector-vs-connector (FALLBACK)"
            problems.insert(
                0,
                f"silver model `{silver_model}` has no contract — add "
                f"`contract: enforced: true` to make it the source of truth",
            )

        if problems:
            failed = True
            print(f"\n✗ {label}  [{mode}]")
            for p in problems:
                print(f"    - {p}")
        else:
            print(f"✓ {label}  [{mode}]")

    if failed:
        print(
            "\nContract drift detected. Every connector feeding a silver class "
            "must emit exactly the columns/types the silver contract declares.\n"
            "Fix: align the `columns:` blocks in each connector's schema.yml to "
            "the silver class contract.",
            file=sys.stderr,
        )
        return 1

    print("\nAll silver classes are connector-consistent.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
