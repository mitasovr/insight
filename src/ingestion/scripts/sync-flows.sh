#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/.."

# KUBECONFIG can be empty when running in-cluster

WORKFLOWS_DIR="./workflows"
CONNECTORS_DIR="./connectors"
CONNECTIONS_DIR="./connections"

# Shared WorkflowTemplates (airbyte-sync, dbt-run, ingestion-pipeline,
# tt-enrich-jira-run) are owned by the umbrella chart and rendered into
# the release namespace on `helm install` (see charts/insight/templates/
# ingestion/*.yaml, controlled by ingestion.templates.enabled). We skip
# applying them here — they are already in the cluster, and re-applying
# from a long-deleted local copy was a stale leftover from before PR #224.
INSIGHT_NS="${INSIGHT_NAMESPACE:-insight}"
if ! kubectl get workflowtemplate -n "$INSIGHT_NS" airbyte-sync >/dev/null 2>&1; then
  echo "ERROR: WorkflowTemplate airbyte-sync not found in namespace '$INSIGHT_NS'." >&2
  echo "       The umbrella chart should have installed it. Check:" >&2
  echo "         helm get values insight -n $INSIGHT_NS | grep ingestion" >&2
  echo "       and ensure ingestion.templates.enabled=true with" >&2
  echo "       ingestion.toolboxImage and ingestion.jiraEnrichImage set." >&2
  exit 1
fi
echo "  Found shared WorkflowTemplates in $INSIGHT_NS"

# --- Get connection_id from toolkit state ---
export TOOLKIT_DIR="${SCRIPT_DIR}/../airbyte-toolkit"
source "${TOOLKIT_DIR}/lib/state.sh"

get_connection_id() {
  local tenant="$1" connector="$2"
  local conn_id=""
  for source_key in $(state_list "tenants.${tenant}.connectors.${connector}"); do
    conn_id=$(state_get "tenants.${tenant}.connectors.${connector}.${source_key}.connection_id")
    [[ -n "$conn_id" ]] && break
  done
  [[ -n "$conn_id" ]] || return 1
  echo "$conn_id"
}

# --- Generate and apply CronWorkflows for a tenant ---
sync_tenant() {
  local tenant="$1"
  local tenant_dir="${WORKFLOWS_DIR}/${tenant}"
  mkdir -p "$tenant_dir"

  # Wipe stale generated workflows from prior runs — connectors may have
  # been removed, renamed, or the namespace contract may have changed
  # (e.g. PR #224 dropped the `argo` namespace). Without this cleanup,
  # `kubectl apply -f $tenant_dir/` below will pick up old YAMLs and try
  # to apply them. Path is rooted under WORKFLOWS_DIR/<tenant> so this
  # is safe.
  rm -f "$tenant_dir"/*.yaml

  # Iterate over all connectors with descriptor.yaml
  for descriptor in "${CONNECTORS_DIR}"/*/*/descriptor.yaml; do
    [[ -f "$descriptor" ]] || continue

    local connector schedule dbt_select workflow
    connector=$(yq -r '.name' "$descriptor")
    schedule=$(yq -r '.schedule' "$descriptor" 2>/dev/null | grep -v null || echo "0 2 * * *")
    dbt_select=$(yq -r '.dbt_select' "$descriptor" 2>/dev/null | grep -v null || echo "+tag:silver")
    workflow=$(yq -r '.workflow' "$descriptor" 2>/dev/null | grep -v null || echo "sync")

    # Find the workflow template
    local tpl="${WORKFLOWS_DIR}/schedules/${workflow}.yaml.tpl"
    if [[ ! -f "$tpl" ]]; then
      echo "  SKIP: no template ${tpl} for connector ${connector}"
      continue
    fi

    # Get connection_id from state
    local connection_id
    connection_id=$(get_connection_id "$tenant" "$connector") || true
    if [[ -z "$connection_id" ]]; then
      echo "  SKIP: no connection_id for ${connector} tenant ${tenant}"
      continue
    fi

    # Generate CronWorkflow
    local output="${tenant_dir}/${connector}-sync.yaml"
    CONNECTOR="$connector" \
    TENANT_ID="$tenant" \
    CONNECTION_ID="$connection_id" \
    SCHEDULE="$schedule" \
    DBT_SELECT="$dbt_select" \
    NAMESPACE="$INSIGHT_NS" \
      envsubst < "$tpl" > "$output"

    echo "  Generated: ${output}"
  done

  # Apply generated workflows
  if ls "${tenant_dir}"/*.yaml >/dev/null 2>&1; then
    kubectl apply -f "$tenant_dir/"
  fi
}

# --- Main ---
if [[ "${1:-}" == "--all" ]]; then
  for tenant in $(state_list "tenants"); do
    echo "  Syncing workflows for tenant: $tenant"
    sync_tenant "$tenant"
  done
else
  tenant="${1:?Usage: $0 <tenant_id> | --all}"
  echo "  Syncing workflows for tenant: $tenant"
  sync_tenant "$tenant"
fi
