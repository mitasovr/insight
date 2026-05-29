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

Opt-in by design. A silver class is checked ONLY when its silver model declares
`contract: enforced: true`. That contract is the single source of truth: every
connector tagged `silver:<class>` MUST emit exactly the columns (name +
data_type) the silver model declares, and must itself carry an enforced contract
(so its declared columns are verified against its SQL by `dbt build` — this gate
only compares declarations, and trusts that dbt keeps them honest).

    silver contract: class_git_commits   (what consumers expect)
        == github__commits
        == bitbucket_cloud__commits
        == github_v2__commits             (the moment it ships)

Classes whose silver model has no enforced contract are SKIPPED (reported, not
failed), so contracts can be rolled out one class at a time.

Convention: the silver class model for tag `silver:<class>` is the model named
`<class>` (e.g. tag `silver:class_git_commits` -> model `class_git_commits`).

Source of truth is the dbt manifest (`dbt parse` -> target/manifest.json), which
is fully offline.

Usage:
    dbt parse  # produces target/manifest.json, no DB connection needed
    python scripts/check_silver_contract_parity.py \
        --manifest src/ingestion/dbt/target/manifest.json

Exit code 0 = all enforced silver classes consistent. 1 = drift found (CI fails).
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


def index_manifest(manifest: dict):
    """Return (connectors_by_tag, model_index).

    connectors_by_tag: tag -> [model_name, ...] for models carrying a
        `silver:*` tag (the per-connector staging models).
    model_index: model_name -> {"columns": {col: data_type}, "enforced": bool}
        for every model; used to resolve the silver class model per tag.
    """
    connectors_by_tag: dict[str, list[str]] = defaultdict(list)
    model_index: dict[str, dict] = {}
    for node in manifest.get("nodes", {}).values():
        if node.get("resource_type") != "model":
            continue
        name = node["name"]
        model_index[name] = {
            "columns": {
                col_name: (col.get("data_type") or "<no data_type>")
                for col_name, col in node.get("columns", {}).items()
            },
            "enforced": bool(node.get("contract", {}).get("enforced", False)),
        }
        for tag in node.get("tags", []):
            if tag.startswith(SILVER_TAG_PREFIX):
                connectors_by_tag[tag].append(name)
    return connectors_by_tag, model_index


def check_class(reference: dict[str, str], connectors: dict[str, dict]) -> list[str]:
    """Compare every connector model against the silver contract column set."""
    problems: list[str] = []
    ref_cols = set(reference)
    for model in sorted(connectors):
        info = connectors[model]
        if not info["enforced"]:
            problems.append(
                f"{model}: contract not enforced — its declared columns are "
                f"unverified against its SQL (add `contract: enforced: true`)"
            )
        cols = info["columns"]
        if not cols:
            problems.append(f"{model}: no typed columns declared in schema.yml")
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


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--manifest",
        type=Path,
        default=Path("src/ingestion/dbt/target/manifest.json"),
        help="path to dbt manifest.json (run `dbt parse` to produce it)",
    )
    args = parser.parse_args()

    connectors_by_tag, model_index = index_manifest(load_manifest(args.manifest))
    if not connectors_by_tag:
        print("No `silver:*`-tagged models found — nothing to check.")
        return 0

    failed = False
    checked = skipped = 0
    for tag in sorted(connectors_by_tag):
        silver_model = tag[len(SILVER_TAG_PREFIX):]  # e.g. "class_git_commits"
        anchor = model_index.get(silver_model)
        connector_names = sorted(connectors_by_tag[tag])
        label = f"{tag}  ({len(connector_names)} connector(s): {', '.join(connector_names)})"

        # Opt-in: only enforce classes whose silver model has an enforced contract.
        if anchor is None or not anchor["enforced"]:
            skipped += 1
            reason = "no silver model found" if anchor is None else "silver contract not enforced"
            print(f"· {tag}  [skipped — {reason}]")
            continue

        checked += 1
        connectors = {n: model_index[n] for n in connector_names}
        problems = check_class(anchor["columns"], connectors)
        if problems:
            failed = True
            print(f"\n✗ {label}  [vs silver contract `{silver_model}`]")
            for p in problems:
                print(f"    - {p}")
        else:
            print(f"✓ {label}  [vs silver contract `{silver_model}`]")

    print(f"\nChecked {checked} enforced class(es); skipped {skipped} without an enforced contract.")
    if failed:
        print(
            "Contract drift detected. Every connector feeding an enforced silver "
            "class must emit exactly the columns/types the silver contract "
            "declares.\nFix: align the `columns:` blocks in each connector's "
            "schema.yml to the silver class contract.",
            file=sys.stderr,
        )
        return 1

    print("All enforced silver classes are connector-consistent.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
