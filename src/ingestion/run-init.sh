#!/usr/bin/env bash
# Initialize the ingestion stack: create databases, register connectors, apply connections, sync workflows.
# Runs from the host machine (requires kubectl, curl, node, python3).
# Run AFTER: ./dev-up.sh && ./secrets/apply.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

export KUBECONFIG="${KUBECONFIG:-${HOME}/.kube/insight.kubeconfig}"

# Single-namespace umbrella (PR #224). All Insight components live in the
# release namespace, default `insight`. Override via INSIGHT_NAMESPACE.
INSIGHT_NS="${INSIGHT_NAMESPACE:-insight}"

# --- Verify the umbrella is installed ---
echo "=== Verifying umbrella install ==="
if ! kubectl get -n "$INSIGHT_NS" statefulset/insight-clickhouse >/dev/null 2>&1; then
  echo "ERROR: insight-clickhouse StatefulSet not found in namespace '$INSIGHT_NS'" >&2
  echo "  Run: ./dev-up.sh --env local  (or your environment installer)" >&2
  exit 1
fi
if ! kubectl get -n "$INSIGHT_NS" secret insight-db-creds >/dev/null 2>&1; then
  echo "ERROR: insight-db-creds Secret not found in namespace '$INSIGHT_NS'" >&2
  echo "  The umbrella chart should have created it on install." >&2
  exit 1
fi

# --- Run init directly from host ---
source ./scripts/init.sh
