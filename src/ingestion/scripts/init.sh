#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/.."

# KUBECONFIG can be empty when running in-cluster

export TOOLKIT_DIR="${SCRIPT_DIR}/../airbyte-toolkit"

# Single-namespace umbrella (PR #224). All Insight components — including the
# bundled ClickHouse StatefulSet — live in the release namespace, default
# `insight`. Exported so child scripts (airbyte-toolkit/*.sh, sync-flows.sh)
# inherit the value.
export INSIGHT_NAMESPACE="${INSIGHT_NAMESPACE:-insight}"
INSIGHT_NS="$INSIGHT_NAMESPACE"
CH_POD="${CLICKHOUSE_POD:-statefulset/insight-clickhouse}"

# clickhouse-client inside the StatefulSet pod inherits CLICKHOUSE_USER /
# CLICKHOUSE_PASSWORD from the container env (set by the chart from
# auth.existingSecret), so we do not pass --user / --password.
ch_exec() {
  kubectl exec -n "$INSIGHT_NS" "$CH_POD" -- clickhouse-client "$@"
}
ch_exec_stdin() {
  kubectl exec -i -n "$INSIGHT_NS" "$CH_POD" -- clickhouse-client "$@"
}

echo "=== Verifying ClickHouse pod ==="
if ! kubectl get -n "$INSIGHT_NS" "$CH_POD" >/dev/null 2>&1; then
  echo "ERROR: ClickHouse not found at -n $INSIGHT_NS $CH_POD" >&2
  echo "  Ensure the umbrella chart is installed with clickhouse.deploy=true" >&2
  echo "  (helm list -n $INSIGHT_NS | grep insight)" >&2
  exit 1
fi

echo "=== Creating dbt databases ==="
for db in staging silver insight; do
  ch_exec --query "CREATE DATABASE IF NOT EXISTS $db"
done

echo "=== Creating bronze placeholders for missing connectors ==="
"$SCRIPT_DIR/create-bronze-placeholders.sh"

echo "=== Running ClickHouse migrations ==="
for migration in "$SCRIPT_DIR/migrations"/*.sql; do
  [ -f "$migration" ] || continue
  echo "  $(basename "$migration")"
  grep -v '^\s*--' "$migration" | ch_exec_stdin --multiquery
done

# MariaDB migrations: each backend service now owns and applies its own
# migrations at startup (SeaORM Migrator::up). See ADR-0006.

echo "=== Registering connectors ==="
"${TOOLKIT_DIR}/register.sh" --all

echo "=== Applying connections ==="
"${TOOLKIT_DIR}/connect.sh" --all

echo "=== Syncing workflows ==="
./scripts/sync-flows.sh --all

echo "=== Init complete ==="
