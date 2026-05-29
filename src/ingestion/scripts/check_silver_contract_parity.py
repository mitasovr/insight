#!/usr/bin/env python3
"""Cross-connector silver-contract parity gate.

Catches the "schema drift between connectors" class of bug *at PR time*,
without a warehouse:

    Two connectors (e.g. github, bitbucket-cloud) feed the same silver class
    via `union_by_tag('silver:class_git_commits')` -> `SELECT * ... UNION ALL`.
    A dev adds a column to one connector's staging model and forgets the other.
    On a cluster where only the lagging connector is materialised the UNION ALL
    fails at runtime (ClickHouse `Code: 386 NO_COMMON_TYPE` or column-count
    mismatch). PR CI stays green because it never materialises every connector
    at once.

This gate makes that impossible: every staging model tagged `silver:<class>`
must declare an IDENTICAL contract (column name + data_type, order-insensitive).
Adding a column to one connector forces you to add it to all of them.

Source of truth is the dbt manifest (`dbt parse` -> target/manifest.json), which
is fully offline. Contracts (`contract: enforced: true` + typed columns in
schema.yml) make the build itself reject SQL that drifts from the declared
columns; this gate then makes the *declared columns* drift-proof across
connectors. The two layers together close the loop.

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


def collect_contracts_by_tag(manifest: dict) -> dict[str, dict[str, dict[str, str]]]:
    """tag -> {model_name -> {column_name -> data_type}} for silver-class tags."""
    by_tag: dict[str, dict[str, dict[str, str]]] = defaultdict(dict)
    for node in manifest.get("nodes", {}).values():
        if node.get("resource_type") != "model":
            continue
        silver_tags = [t for t in node.get("tags", []) if t.startswith(SILVER_TAG_PREFIX)]
        if not silver_tags:
            continue
        columns = {
            name: (col.get("data_type") or "<no data_type>")
            for name, col in node.get("columns", {}).items()
        }
        for tag in silver_tags:
            by_tag[tag][node["name"]] = columns
    return by_tag


def diff_group(models: dict[str, dict[str, str]]) -> list[str]:
    """Return human-readable problems for one silver-class group, or []."""
    problems: list[str] = []

    # 1. Any model with no declared contract columns is a hole in the gate.
    uncontracted = sorted(m for m, cols in models.items() if not cols)
    if uncontracted:
        problems.append(
            "models without a typed contract (add `contract: enforced: true` "
            "+ typed columns to schema.yml): " + ", ".join(uncontracted)
        )

    contracted = {m: cols for m, cols in models.items() if cols}
    if len(contracted) < 2:
        return problems  # nothing to compare

    # 2. Union of all columns across the group is the reference set.
    all_columns: set[str] = set()
    for cols in contracted.values():
        all_columns |= set(cols)

    for model in sorted(contracted):
        cols = contracted[model]
        missing = sorted(all_columns - set(cols))
        if missing:
            problems.append(f"{model}: MISSING columns {missing}")

    # 3. Same column name, different declared type -> guaranteed Code: 386.
    for column in sorted(all_columns):
        types = {m: cols[column] for m, cols in contracted.items() if column in cols}
        distinct = set(types.values())
        if len(distinct) > 1:
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

    by_tag = collect_contracts_by_tag(load_manifest(args.manifest))
    if not by_tag:
        print("No `silver:*`-tagged models found — nothing to check.")
        return 0

    failed = False
    for tag in sorted(by_tag):
        models = by_tag[tag]
        problems = diff_group(models)
        if problems:
            failed = True
            print(f"\n✗ {tag}  ({len(models)} connector model(s): {', '.join(sorted(models))})")
            for p in problems:
                print(f"    - {p}")
        else:
            print(f"✓ {tag}  ({len(models)} connector model(s) in sync)")

    if failed:
        print(
            "\nContract drift detected. Every connector feeding a silver class "
            "must declare the same columns with the same types.\n"
            "Fix: align the `columns:` blocks in each connector's schema.yml.",
            file=sys.stderr,
        )
        return 1

    print("\nAll silver classes are connector-consistent.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
