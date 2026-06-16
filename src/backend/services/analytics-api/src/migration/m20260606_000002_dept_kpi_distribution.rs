//! Per-department quartile distribution for the team heatmap's `team_row`
//! KPI keys, from `insight.ic_kpis`. The same per-person rollup the IC KPIs
//! metric uses (`m20260604_000006`) is run once, then unpivoted to long
//! `(metric_key, value)` rows and rolled into per-(department, metric)
//! quartiles. One row per `(org_unit_id, metric_key)`:
//!   `org_unit_id, metric_key, p25, median, p75, range_min, range_max, n`.
//!
//! The five keys are exactly the heatmap's `team_row` columns:
//! `tasks_closed`, `bugs_fixed`, `prs_merged`, `focus_time_pct`,
//! `ai_loc_share_pct`.
//!
//! Caveat: the `prs_merged` department distribution here derives from
//! `insight.ic_kpis`, whose PR attribution still has the known pre-#627
//! name-fallback gap (an unresolved author falls back to a name match and
//! can mis-attribute). That is NOT fixed here — this migration only reshapes
//! the existing rollup into a department distribution; correcting the
//! attribution is a separate upstream change.
//!
//! The per-person rollup is copied verbatim from
//! `m20260604_000006_ic_kpis_peer_median::per_person_rollup` as a
//! self-contained helper (repo convention: a migration owns the exact SQL it
//! installs). `inject_date_filter_into_subqueries` injects the metric_date
//! range before the rollup's `GROUP BY person_id`; the handler re-appends the
//! outer `GROUP BY org_unit_id, metric_key`, and an `org_unit_id IN (...)`
//! filter binds against the promoted `org_unit_id` column.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

const ZERO_TENANT: &str = "00000000000000000000000000000000";
const DEPT_KPI_DIST_HEX: &str = "00000000000000000001000000000047";

/// Per-person rollup over `insight.ic_kpis` (daily rows → one row per person
/// for the period), copied verbatim from
/// `m20260604_000006_ic_kpis_peer_median::per_person_rollup`. Surfaces
/// `org_unit_id` (the department cohort key) alongside each per-person KPI.
fn per_person_rollup() -> &'static str {
    "SELECT person_id, any(org_unit_id) AS org_unit_id, \
         sum(loc) AS loc, \
         round(avg(ai_loc_share_pct), 1) AS ai_loc_share_pct, \
         sum(prs_merged) AS prs_merged, \
         avg(pr_cycle_time_h) AS pr_cycle_time_h, \
         round(avg(focus_time_pct), 1) AS focus_time_pct, \
         sum(tasks_closed) AS tasks_closed, \
         sum(bugs_fixed) AS bugs_fixed, \
         anyOrNull(build_success_pct) AS build_success_pct, \
         sum(ai_sessions) AS ai_sessions \
     FROM insight.ic_kpis \
     GROUP BY person_id"
}

fn dept_kpi_distribution_query() -> String {
    let pp = per_person_rollup();
    format!(
        "SELECT org_unit_id, kv.1 AS metric_key, \
                quantileExact(0.25)(kv.2) AS p25, \
                quantileExact(0.5)(kv.2) AS median, \
                quantileExact(0.75)(kv.2) AS p75, \
                min(kv.2) AS range_min, \
                max(kv.2) AS range_max, \
                count(kv.2) AS n \
         FROM ({pp}) pp \
         ARRAY JOIN [ \
             ('tasks_closed', toFloat64(tasks_closed)), \
             ('bugs_fixed', toFloat64(bugs_fixed)), \
             ('prs_merged', toFloat64(prs_merged)), \
             ('focus_time_pct', toFloat64(focus_time_pct)), \
             ('ai_loc_share_pct', toFloat64(ai_loc_share_pct)) \
         ] AS kv \
         GROUP BY org_unit_id, metric_key"
    )
}

const NAME: &str = "Dept Distribution — Heatmap KPIs";
const DESCRIPTION: &str = "Per-(department, metric) quartile distribution for the team heatmap KPI keys (tasks_closed, bugs_fixed, prs_merged, focus_time_pct, ai_loc_share_pct), from insight.ic_kpis. Filter by org_unit_id IN (...). NOTE: prs_merged inherits the pre-#627 PR name-fallback attribution gap.";

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(&format!(
            "INSERT INTO metrics (id, insight_tenant_id, name, description, query_ref, is_enabled) \
             VALUES (UNHEX('{DEPT_KPI_DIST_HEX}'), UNHEX('{ZERO_TENANT}'), '{name}', '{description}', '{qr}', 1) \
             ON DUPLICATE KEY UPDATE name=VALUES(name), description=VALUES(description), query_ref=VALUES(query_ref), is_enabled=1",
            name = NAME.replace('\'', "''"),
            description = DESCRIPTION.replace('\'', "''"),
            qr = dept_kpi_distribution_query().replace('\'', "''"),
        ))
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(&format!(
            "DELETE FROM metrics WHERE id = UNHEX('{DEPT_KPI_DIST_HEX}')"
        ))
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The five heatmap `team_row` KPI keys that must each be unpivoted into
    /// a long `(metric_key, value)` row and rolled into a department
    /// distribution.
    const HEATMAP_KEYS: &[&str] = &[
        "tasks_closed",
        "bugs_fixed",
        "prs_merged",
        "focus_time_pct",
        "ai_loc_share_pct",
    ];

    #[test]
    fn query_shape() {
        let q = dept_kpi_distribution_query();
        assert!(
            q.contains("GROUP BY org_unit_id, metric_key"),
            "outer GROUP BY must be `org_unit_id, metric_key`, got:\n{q}"
        );
        assert!(
            q.contains("org_unit_id, kv.1 AS metric_key"),
            "outer projection must keep org_unit_id and unpivot metric_key, got:\n{q}"
        );
        for key in HEATMAP_KEYS {
            assert!(
                q.contains(&format!("('{key}', toFloat64({key}))")),
                "missing ARRAY JOIN entry for heatmap key {key}, got:\n{q}"
            );
        }
        for alias in [
            "quantileExact(0.25)(kv.2) AS p25",
            "quantileExact(0.5)(kv.2) AS median",
            "quantileExact(0.75)(kv.2) AS p75",
            "min(kv.2) AS range_min",
            "max(kv.2) AS range_max",
            "count(kv.2) AS n",
        ] {
            assert!(
                q.contains(alias),
                "missing quartile/range/size alias `{alias}`, got:\n{q}"
            );
        }
        // Derives from the IC KPIs rollup (single ic_kpis read).
        assert_eq!(
            q.matches("FROM insight.ic_kpis").count(),
            1,
            "expected the per-person rollup to read ic_kpis once, got:\n{q}"
        );
    }
}
