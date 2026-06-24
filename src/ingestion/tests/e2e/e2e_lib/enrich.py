"""Connector *enrich* step: run a connector's pre-built enrich binary.

Some connectors materialize part of their silver via a compiled "enrich" binary
that reads the connector's staging tables and writes back into `staging.*`, which
dbt then unions into `silver.*` via `union_by_tag`. This is a first-class step in
the connector pipeline, NOT a per-connector special case: any connector whose
`descriptor.yaml` declares an `images.enrich` block participates.

Build vs run split (deliberate — see the e2e DESIGN notes):
  * The BINARY IS BUILT BY THE CONNECTOR'S OWN `Dockerfile` (the same one that
    ships the prod image), driven by `e2e.sh build`, which then extracts the
    compiled binary into `tests/e2e/target/enrich/<binary>`. The rig does NOT
    compile anything itself — there is no cargo logic here and no duplication of
    the connector's build recipe. `e2e.sh` runs on the host (which has the Docker
    daemon); the runner container cannot build images (no docker-in-docker).
  * This module only DISCOVERS the steps (from descriptors) and RUNS the staged
    binary inside the runner, between the staging and silver dbt builds — mirrors
    prod `dbt(tag:<c>) -> <c>-enrich -> dbt(silver)`. Data-driven, so a new
    connector (e.g. youtrack) participates as soon as it ships an `images.enrich`.

`python -m e2e_lib.enrich --plan` prints the build plan (one TSV row per enrich
step) that `e2e.sh build` consumes to build+stage the binaries — keeping the
descriptor the single source of truth for both build and run.

The enrich binary contract (shared by the family — verified against jira-enrich):
  - CLI/env: `--insight-source-id` (INSIGHT_SOURCE_ID), `--clickhouse-host`
    (CLICKHOUSE_HOST), `--clickhouse-port` (CLICKHOUSE_PORT, HTTP), `--clickhouse-user`
    (CLICKHOUSE_USER); password via CLICKHOUSE_PASSWORD env. Reads+writes `staging.*`.
"""

from __future__ import annotations

import logging
import os
import subprocess
import tomllib
from dataclasses import dataclass
from pathlib import Path

import yaml

from e2e_lib import clickhouse as ch
from e2e_lib.config import SessionConfig

LOG = logging.getLogger("e2e.enrich")

_CONNECTORS_GLOB = "src/ingestion/connectors/**/descriptor.yaml"
# Where `e2e.sh build` stages the extracted enrich binaries (under the dbt target
# tree, which is gitignored and mounted into the runner at /workspace).
_STAGE_DIR_REL = "src/ingestion/tests/e2e/target/enrich"


class EnrichError(RuntimeError):
    """A connector enrich step failed to run, or its binary was not staged."""


@dataclass(frozen=True)
class EnrichStep:
    """A connector's declared enrich step (from descriptor.yaml.images.enrich)."""

    name: str  # connector name, e.g. "jira" — also its dbt tag
    namespace: str  # bronze schema, e.g. "bronze_jira"
    dockerfile: Path  # the connector's OWN Dockerfile (built by e2e.sh, not the rig)
    context: Path  # docker build context for that Dockerfile
    binary_name: str  # crate [package].name, e.g. "jira-enrich" — the staged filename
    stage_dir: Path  # tests/e2e/target/enrich (where e2e.sh drops the binary)

    @property
    def binary_path(self) -> Path:
        """Path to the binary staged by `e2e.sh build` (extracted from the image)."""
        return self.stage_dir / self.binary_name


def discover_enrich_steps(repo_root: Path) -> list[EnrichStep]:
    """Every connector descriptor that declares `images.enrich`."""
    stage_dir = repo_root / _STAGE_DIR_REL
    steps: list[EnrichStep] = []
    for desc_path in sorted(repo_root.glob(_CONNECTORS_GLOB)):
        try:
            doc = yaml.safe_load(desc_path.read_text(encoding="utf-8")) or {}
        except yaml.YAMLError as e:
            LOG.warning("skipping unreadable descriptor %s: %s", desc_path, e)
            continue
        enrich = (doc.get("images") or {}).get("enrich")
        if not enrich:
            continue
        namespace = (doc.get("connection") or {}).get("namespace")
        if not namespace:
            LOG.warning("descriptor %s has images.enrich but no connection.namespace; skipping", desc_path)
            continue
        dockerfile_rel = enrich.get("dockerfile")
        context_rel = enrich.get("context", "./enrich")
        if not dockerfile_rel:
            LOG.warning("descriptor %s images.enrich missing `dockerfile`; skipping", desc_path)
            continue
        dockerfile = (desc_path.parent / dockerfile_rel).resolve()
        context = (desc_path.parent / context_rel).resolve()
        cargo_toml = context / "Cargo.toml"
        if not cargo_toml.is_file():
            LOG.warning("enrich crate Cargo.toml not found at %s (descriptor %s); skipping", cargo_toml, desc_path)
            continue
        try:
            binary_name = tomllib.loads(cargo_toml.read_text(encoding="utf-8"))["package"]["name"]
        except (KeyError, ValueError) as e:
            LOG.warning("cannot read [package].name from %s: %s; skipping", cargo_toml, e)
            continue
        steps.append(
            EnrichStep(
                name=doc.get("name") or namespace.removeprefix("bronze_"),
                namespace=namespace,
                dockerfile=dockerfile,
                context=context,
                binary_name=binary_name,
                stage_dir=stage_dir,
            )
        )
    return steps


class EnrichRunner:
    """Session-scoped: discover enrich steps once; run their pre-staged binaries."""

    def __init__(self, cfg: SessionConfig):
        """Discover enrich steps from connector descriptors once per session."""
        self.cfg = cfg
        self.steps = discover_enrich_steps(cfg.repo_root)

    def steps_for(self, schemas: set[str]) -> list[EnrichStep]:
        """Enrich steps whose bronze namespace is among the seeded schemas."""
        return [s for s in self.steps if s.namespace in schemas]

    def discover_source_ids(self, step: EnrichStep, tables: set[tuple[str, str]]) -> list[str]:
        """Distinct non-empty `source_id`s across the seeded tables in the step's namespace.

        enrich is scoped per connector instance (`--insight-source-id`); the rig
        derives the instances to enrich from the data the test actually seeded.
        """
        found: set[str] = set()
        for schema, table in sorted(tables):
            if schema != step.namespace:
                continue
            cols = ch.query(
                self.cfg,
                f"SELECT name FROM system.columns WHERE database = '{schema}' AND table = '{table}' AND name = 'source_id'",
            )
            if not cols:
                continue
            rows = ch.query(
                self.cfg,
                f"SELECT DISTINCT source_id FROM `{schema}`.`{table}` WHERE source_id IS NOT NULL AND source_id != ''",
            )
            found.update(str(r[0]) for r in rows)
        return sorted(found)

    def run(self, step: EnrichStep, source_ids: list[str], *, timeout_s: float = 180.0) -> None:
        """Run the pre-staged enrich binary once per connector instance.

        The binary must have been built+staged by `e2e.sh build` (it compiles the
        connector's own Dockerfile and extracts the binary into `target/enrich/`).
        """
        if not source_ids:
            LOG.warning(
                "enrich %s: no source_id found in seeded %s tables; skipping (nothing to enrich)",
                step.name,
                step.namespace,
            )
            return
        if not step.binary_path.exists():
            raise EnrichError(
                f"{step.name} enrich binary not staged at {step.binary_path} — run `./e2e.sh build` "
                f"first (it builds {step.dockerfile} and extracts the binary)."
            )
        for sid in source_ids:
            env = os.environ.copy()
            env.update(
                {
                    "CLICKHOUSE_HOST": self.cfg.ch_host,
                    "CLICKHOUSE_PORT": str(self.cfg.ch_http_port),
                    "CLICKHOUSE_USER": self.cfg.ch_user,
                    "CLICKHOUSE_PASSWORD": self.cfg.ch_password,
                    "INSIGHT_SOURCE_ID": sid,
                    "RUST_LOG": env.get("RUST_LOG", "info"),
                }
            )
            LOG.info("running %s enrich for source_id=%s", step.name, sid)
            result = subprocess.run(
                [
                    str(step.binary_path),
                    "--insight-source-id", sid,
                    "--clickhouse-host", self.cfg.ch_host,
                    "--clickhouse-port", str(self.cfg.ch_http_port),
                    "--clickhouse-user", self.cfg.ch_user,
                ],
                env=env,
                capture_output=True,
                text=True,
                check=False,
                timeout=timeout_s,
            )
            if result.returncode != 0:
                raise EnrichError(
                    f"{step.name} enrich failed for source_id={sid} (exit={result.returncode}):\n"
                    f"stdout tail:\n{result.stdout[-1500:]}\nstderr tail:\n{result.stderr[-1500:]}"
                )


def _print_build_plan(repo_root: Path) -> None:
    """Emit one TSV row per enrich step for `e2e.sh build` (paths repo-root-relative).

    Columns: name <TAB> dockerfile <TAB> context <TAB> binary_name. `e2e.sh` builds
    each `dockerfile` (the connector's own), then extracts the binary named
    `binary_name` into `target/enrich/`. Keeps the descriptor the single source of
    truth for both build and run sides.
    """
    for s in discover_enrich_steps(repo_root):
        dockerfile = s.dockerfile.relative_to(repo_root)
        context = s.context.relative_to(repo_root)
        print(f"{s.name}\t{dockerfile}\t{context}\t{s.binary_name}")


if __name__ == "__main__":
    import argparse

    parser = argparse.ArgumentParser(description="enrich-step helper")
    parser.add_argument("--plan", action="store_true", help="print the build plan as TSV and exit")
    parser.add_argument(
        "--repo-root",
        default=os.environ.get("INSIGHT_REPO_ROOT", "/workspace"),
        help="repo root (default: $INSIGHT_REPO_ROOT or /workspace)",
    )
    args = parser.parse_args()
    if args.plan:
        _print_build_plan(Path(args.repo_root))
