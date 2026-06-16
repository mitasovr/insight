//! Per-(department, metric) distribution metrics for the three bullet
//! sections that already expose a cohort distribution to the bullet gauge
//! (Task Delivery, Collaboration, Git). Each section's cohort subquery —
//! the one the IC bullet already computes per `metric_key, org_unit_id` —
//! is PROMOTED to the top level, keeping `org_unit_id` in the projection so
//! the FE can read a department's quartile band for any metric in one row.
//!
//! Output columns are uniform across the three:
//!   `org_unit_id, metric_key, p25, median, p75, range_min, range_max, n`.
//!
//! The wide-aggregate + ARRAY JOIN blocks are copied verbatim from each
//! section's distribution migration (`m20260604_00000{1,2,5}`) as
//! self-contained module helpers, per the repo convention that a migration
//! captures the exact SQL it installs rather than referencing another
//! migration's private helpers (which may later change under it).
//!
//! Both bullet-row leaves keep `GROUP BY person_id`, so
//! `inject_date_filter_into_subqueries` injects the metric_date range before
//! each per-person GROUP BY exactly as in the source migrations. The handler
//! re-appends the outer `GROUP BY org_unit_id, metric_key`; an
//! `org_unit_id IN (...)` filter lands in the outer WHERE against the
//! promoted `org_unit_id` column.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

const ZERO_TENANT: &str = "00000000000000000000000000000000";

const DEPT_DIST_DELIVERY_HEX: &str = "00000000000000000001000000000044";
const DEPT_DIST_COLLAB_HEX: &str = "00000000000000000001000000000045";
const DEPT_DIST_GIT_HEX: &str = "00000000000000000001000000000046";

// ── Task Delivery ───────────────────────────────────────────────────────

/// Time-bound metrics whose period distribution has a long right tail; P95
/// caps `range_max` so a single old issue closed in-window doesn't blow the
/// gauge scale. Copied verbatim from `m20260604_000001`.
const DELIVERY_P95_LIST: &str = "'mean_time_to_resolution', 'task_dev_time', 'pickup_time'";

/// Inner wide-aggregate block (copied verbatim from
/// `m20260604_000001_task_delivery_bullet_distribution::wide_aggregate_pp`):
/// one row per `person_id`, every FE-visible metric in its own column.
fn delivery_wide_aggregate_pp() -> &'static str {
    "SELECT person_id, any(org_unit_id) AS org_unit_id, \
         sumIf(metric_value, metric_key = 'tasks_completed') AS tasks_completed_v, \
         sumIf(metric_value, metric_key = 'stale_in_progress') AS stale_in_progress_v, \
         quantileExactIf(0.5)(metric_value, metric_key = 'task_dev_time' AND isNotNull(metric_value)) AS task_dev_time_v, \
         quantileExactIf(0.5)(metric_value, metric_key = 'mean_time_to_resolution' AND isNotNull(metric_value)) AS mttr_v, \
         quantileExactIf(0.5)(metric_value, metric_key = 'pickup_time' AND isNotNull(metric_value)) AS pickup_time_v, \
         if(sumIf(metric_value, metric_key = 'task_reopen_rate' AND metric_value > 0) >= 5, \
            round((-sumIf(metric_value, metric_key = 'task_reopen_rate' AND metric_value < 0) \
                   / sumIf(metric_value, metric_key = 'task_reopen_rate' AND metric_value > 0)) * 100, 1), \
            CAST(NULL AS Nullable(Float64))) AS task_reopen_rate_v, \
         if(sumIf(metric_value, metric_key = 'due_date_with_due') > 0, \
            round(toFloat64(100) * sumIf(metric_value, metric_key = 'due_date_on_time') \
                                 / sumIf(metric_value, metric_key = 'due_date_with_due'), 1), \
            CAST(NULL AS Nullable(Float64))) AS due_date_compliance_v, \
         if(sumIf(metric_value, metric_key = 'tasks_completed') > 0, \
            round(toFloat64(100) * sumIf(metric_value, metric_key = 'bugs_fixed') \
                                 / sumIf(metric_value, metric_key = 'tasks_completed'), 1), \
            CAST(NULL AS Nullable(Float64))) AS bugs_to_task_ratio_v, \
         if(sumIf(metric_value, metric_key = 'flow_efficiency_den') > 0, \
            least(toFloat64(100), \
                  round(toFloat64(100) * sumIf(metric_value, metric_key = 'flow_efficiency_num') \
                                       / sumIf(metric_value, metric_key = 'flow_efficiency_den'), 1)), \
            CAST(NULL AS Nullable(Float64))) AS flow_efficiency_v, \
         if(sumIf(metric_value, metric_key = 'in_progress_seconds') > 0, \
            least(toFloat64(100), \
                  round(toFloat64(100) * sumIf(metric_value, metric_key = 'worklog_seconds') \
                                       / sumIf(metric_value, metric_key = 'in_progress_seconds'), 1)), \
            CAST(NULL AS Nullable(Float64))) AS worklog_logging_accuracy_v, \
         if(countIf(metric_key = 'estimation_accuracy' AND metric_value > 0 AND metric_value <= 200) > 0, \
            greatest(toFloat64(0), \
                     toFloat64(100) - avgIf(abs(toFloat64(100) - metric_value), \
                                             metric_key = 'estimation_accuracy' AND metric_value > 0 AND metric_value <= 200)), \
            CAST(NULL AS Nullable(Float64))) AS estimation_accuracy_v \
     FROM insight.task_delivery_bullet_rows \
     GROUP BY person_id"
}

/// `ARRAY JOIN` unpivot (copied verbatim from `m20260604_000001`).
fn delivery_array_join_kv() -> &'static str {
    "ARRAY JOIN [ \
         ('tasks_completed',           tasks_completed_v), \
         ('stale_in_progress',         stale_in_progress_v), \
         ('task_dev_time',             task_dev_time_v), \
         ('mean_time_to_resolution',   mttr_v), \
         ('pickup_time',               pickup_time_v), \
         ('task_reopen_rate',          task_reopen_rate_v), \
         ('due_date_compliance',       due_date_compliance_v), \
         ('bugs_to_task_ratio',        bugs_to_task_ratio_v), \
         ('flow_efficiency',           flow_efficiency_v), \
         ('worklog_logging_accuracy',  worklog_logging_accuracy_v), \
         ('estimation_accuracy',       estimation_accuracy_v) \
     ] AS kv"
}

/// `range_max` aggregator: P95 for time-tail metrics, plain max otherwise.
/// Copied verbatim from `m20260604_000001::range_max_expr`.
fn delivery_range_max_expr() -> String {
    format!("if(metric_key IN ({DELIVERY_P95_LIST}), quantileExact(0.95)(v_period), max(v_period))")
}

fn delivery_query() -> String {
    let pp = delivery_wide_aggregate_pp();
    let kv = delivery_array_join_kv();
    let rmax = delivery_range_max_expr();
    format!(
        "SELECT org_unit_id, metric_key, \
                quantileExact(0.25)(v_period) AS p25, \
                quantileExact(0.5)(v_period) AS median, \
                quantileExact(0.75)(v_period) AS p75, \
                min(v_period) AS range_min, \
                {rmax} AS range_max, \
                count(v_period) AS n \
         FROM ( \
             SELECT org_unit_id, kv.1 AS metric_key, kv.2 AS v_period \
             FROM ({pp}) ppc \
             {kv} \
         ) inner_c \
         GROUP BY org_unit_id, metric_key"
    )
}

// ── Collaboration ───────────────────────────────────────────────────────

/// Inner wide-aggregate block (copied verbatim from
/// `m20260604_000002_collab_bullet_distribution::wide_aggregate_pp`).
fn collab_wide_aggregate_pp() -> &'static str {
    "SELECT person_id, any(org_unit_id) AS org_unit_id, \
         sumIf(metric_value, metric_key = 'm365_emails_sent') AS m365_emails_sent_v, \
         sumIf(metric_value, metric_key = 'm365_emails_received') AS m365_emails_received_v, \
         sumIf(metric_value, metric_key = 'm365_emails_read') AS m365_emails_read_v, \
         sumIf(metric_value, metric_key = 'meeting_hours') AS meeting_hours_v, \
         sumIf(metric_value, metric_key = 'meetings_count') AS meetings_count_v, \
         sumIf(metric_value, metric_key = 'teams_meeting_hours') AS teams_meeting_hours_v, \
         sumIf(metric_value, metric_key = 'zoom_meeting_hours') AS zoom_meeting_hours_v, \
         sumIf(metric_value, metric_key = 'teams_meetings') AS teams_meetings_v, \
         sumIf(metric_value, metric_key = 'zoom_meetings') AS zoom_meetings_v, \
         sumIf(metric_value, metric_key = 'meeting_free') AS meeting_free_v, \
         sumIf(metric_value, metric_key = 'm365_teams_chats') AS m365_teams_chats_v, \
         sumIf(metric_value, metric_key = 'slack_messages_sent') AS slack_messages_sent_v, \
         sumIf(metric_value, metric_key = 'slack_channel_posts') AS slack_channel_posts_v, \
         sumIf(metric_value, metric_key = 'slack_active_days') AS slack_active_days_v, \
         sumIf(metric_value, metric_key = 'm365_files_shared_internal') AS m365_files_shared_internal_v, \
         sumIf(metric_value, metric_key = 'm365_files_shared_external') AS m365_files_shared_external_v, \
         sumIf(metric_value, metric_key = 'm365_files_engaged') AS m365_files_engaged_v, \
         sumIf(metric_value, metric_key = 'm365_active_days') AS m365_active_days_v, \
         if(sumIf(metric_value, metric_key = 'slack_active_days') > 0, \
            round(sumIf(metric_value, metric_key = 'slack_messages_sent') \
                  / sumIf(metric_value, metric_key = 'slack_active_days'), 1), \
            CAST(NULL AS Nullable(Float64))) AS slack_msgs_per_active_day_v, \
         if(sumIf(metric_value, metric_key = 'slack_messages_sent') > 0, \
            round(toFloat64(100) \
                  * greatest(toFloat64(0), \
                             sumIf(metric_value, metric_key = 'slack_messages_sent') \
                             - sumIf(metric_value, metric_key = 'slack_channel_posts')) \
                  / sumIf(metric_value, metric_key = 'slack_messages_sent'), 1), \
            CAST(NULL AS Nullable(Float64))) AS slack_dm_ratio_v \
     FROM insight.collab_bullet_rows \
     GROUP BY person_id"
}

/// `ARRAY JOIN` unpivot (copied verbatim from `m20260604_000002`).
fn collab_array_join_kv() -> &'static str {
    "ARRAY JOIN [ \
         ('m365_emails_sent',            m365_emails_sent_v), \
         ('m365_emails_received',        m365_emails_received_v), \
         ('m365_emails_read',            m365_emails_read_v), \
         ('meeting_hours',               meeting_hours_v), \
         ('meetings_count',              meetings_count_v), \
         ('teams_meeting_hours',         teams_meeting_hours_v), \
         ('zoom_meeting_hours',          zoom_meeting_hours_v), \
         ('teams_meetings',              teams_meetings_v), \
         ('zoom_meetings',               zoom_meetings_v), \
         ('meeting_free',                meeting_free_v), \
         ('m365_teams_chats',            m365_teams_chats_v), \
         ('slack_messages_sent',         slack_messages_sent_v), \
         ('slack_channel_posts',         slack_channel_posts_v), \
         ('slack_active_days',           slack_active_days_v), \
         ('m365_files_shared_internal',  m365_files_shared_internal_v), \
         ('m365_files_shared_external',  m365_files_shared_external_v), \
         ('m365_files_engaged',          m365_files_engaged_v), \
         ('m365_active_days',            m365_active_days_v), \
         ('slack_msgs_per_active_day',   slack_msgs_per_active_day_v), \
         ('slack_dm_ratio',              slack_dm_ratio_v) \
     ] AS kv"
}

fn collab_query() -> String {
    let pp = collab_wide_aggregate_pp();
    let kv = collab_array_join_kv();
    format!(
        "SELECT org_unit_id, metric_key, \
                quantileExact(0.25)(v_period) AS p25, \
                quantileExact(0.5)(v_period) AS median, \
                quantileExact(0.75)(v_period) AS p75, \
                min(v_period) AS range_min, \
                max(v_period) AS range_max, \
                count(v_period) AS n \
         FROM ( \
             SELECT org_unit_id, kv.1 AS metric_key, kv.2 AS v_period \
             FROM ({pp}) ppc \
             {kv} \
         ) inner_c \
         GROUP BY org_unit_id, metric_key"
    )
}

// ── Git ─────────────────────────────────────────────────────────────────

/// Inner git block (wide aggregate + ARRAY JOIN with `toFloat64`), copied
/// verbatim from the cohort (`inner_c`) leaf of `m20260604_000005`'s
/// `NEW_QUERY_REF`. Keeps `any(org_unit_id) AS org_unit_id`; the `*If`
/// family + ARRAY JOIN `toFloat64` casts are preserved exactly.
const GIT_INNER: &str = "SELECT org_unit_id, kv.1 AS metric_key, kv.2 AS v_period FROM (SELECT person_id, any(org_unit_id) AS org_unit_id, sumIf(metric_value, metric_key = 'commits') AS commits, sumIf(metric_value, metric_key = 'loc') AS loc, sumIf(metric_value, metric_key = 'clean_loc') AS clean_loc, sumIf(metric_value, metric_key = 'prs_created') AS prs_created, sumIf(metric_value, metric_key = 'prs_merged') AS prs_merged, countIf(metric_key = 'commits' AND metric_value > 0) AS active_days, quantileExactIf(0.5)(metric_value, metric_key = 'pr_cycle_time_h') AS pr_cycle_time_h, quantileExactIf(0.5)(metric_value, metric_key = 'pr_size') AS pr_size FROM insight.git_bullet_rows GROUP BY person_id) ARRAY JOIN [('commits', toFloat64(commits)), ('prs_created', toFloat64(prs_created)), ('prs_merged', toFloat64(prs_merged)), ('clean_loc', toFloat64(clean_loc)), ('pr_cycle_time_h', pr_cycle_time_h), ('pr_size', pr_size), ('merge_rate', if(prs_created > 0, prs_merged * 100.0 / prs_created, NULL)), ('lines_per_commit', if(commits > 0, loc * 1.0 / commits, NULL)), ('commits_per_active_day', if(active_days > 0, commits * 1.0 / active_days, NULL))] AS kv";

fn git_query() -> String {
    format!(
        "SELECT org_unit_id, metric_key, \
                quantileExactIf(0.25)(v_period, isNotNull(v_period)) AS p25, \
                quantileExactIf(0.5)(v_period, isNotNull(v_period)) AS median, \
                quantileExactIf(0.75)(v_period, isNotNull(v_period)) AS p75, \
                minIf(v_period, isNotNull(v_period)) AS range_min, \
                maxIf(v_period, isNotNull(v_period)) AS range_max, \
                countIf(isNotNull(v_period)) AS n \
         FROM ({GIT_INNER}) inner_c \
         GROUP BY org_unit_id, metric_key"
    )
}

struct Seed {
    hex: &'static str,
    name: &'static str,
    description: &'static str,
    query: String,
}

fn seeds() -> Vec<Seed> {
    vec![
        Seed {
            hex: DEPT_DIST_DELIVERY_HEX,
            name: "Dept Distribution — Task Delivery",
            description: "Per-(department, metric) quartile distribution for the Task Delivery bullet keys, from insight.task_delivery_bullet_rows. Filter by org_unit_id IN (...).",
            query: delivery_query(),
        },
        Seed {
            hex: DEPT_DIST_COLLAB_HEX,
            name: "Dept Distribution — Collaboration",
            description: "Per-(department, metric) quartile distribution for the Collaboration bullet keys, from insight.collab_bullet_rows. Filter by org_unit_id IN (...).",
            query: collab_query(),
        },
        Seed {
            hex: DEPT_DIST_GIT_HEX,
            name: "Dept Distribution — Git",
            description: "Per-(department, metric) quartile distribution for the Git bullet keys, from insight.git_bullet_rows. Filter by org_unit_id IN (...).",
            query: git_query(),
        },
    ]
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        for seed in seeds() {
            db.execute_unprepared(&format!(
                "INSERT INTO metrics (id, insight_tenant_id, name, description, query_ref, is_enabled) \
                 VALUES (UNHEX('{hex}'), UNHEX('{ZERO_TENANT}'), '{name}', '{description}', '{qr}', 1) \
                 ON DUPLICATE KEY UPDATE name=VALUES(name), description=VALUES(description), query_ref=VALUES(query_ref), is_enabled=1",
                hex = seed.hex,
                name = seed.name.replace('\'', "''"),
                description = seed.description.replace('\'', "''"),
                qr = seed.query.replace('\'', "''"),
            ))
            .await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        for hex in [DEPT_DIST_DELIVERY_HEX, DEPT_DIST_COLLAB_HEX, DEPT_DIST_GIT_HEX] {
            db.execute_unprepared(&format!("DELETE FROM metrics WHERE id = UNHEX('{hex}')"))
                .await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every dept-distribution `query_ref` must promote the section's cohort
    /// subquery to the top level: group per `(org_unit_id, metric_key)`,
    /// keep `org_unit_id` in the outer projection, surface the quartile +
    /// size aliases, and read its own bullet-rows source table.
    fn assert_common_shape(query: &str, label: &str, source_table: &str, key: &str) {
        assert!(
            query.contains("GROUP BY org_unit_id, metric_key"),
            "{label}: outer GROUP BY must be `org_unit_id, metric_key`, got:\n{query}"
        );
        assert!(
            query.starts_with("SELECT org_unit_id, metric_key,"),
            "{label}: outer projection must lead with `org_unit_id, metric_key`, got:\n{query}"
        );
        for alias in ["AS p25", "AS median", "AS p75", "AS range_min", "AS range_max", "AS n"] {
            assert!(
                query.contains(alias),
                "{label}: missing output alias `{alias}`, got:\n{query}"
            );
        }
        assert!(
            query.contains(source_table),
            "{label}: must read its own source `{source_table}`, got:\n{query}"
        );
        assert!(
            query.contains(&format!("'{key}'")),
            "{label}: must contain its section key '{key}', got:\n{query}"
        );
    }

    #[test]
    fn delivery_query_shape() {
        let q = delivery_query();
        assert_common_shape(
            &q,
            "delivery_query",
            "insight.task_delivery_bullet_rows",
            "mean_time_to_resolution",
        );
        // Plain quantileExact (no *If) for the delivery section.
        assert!(
            q.contains("quantileExact(0.25)(v_period) AS p25")
                && q.contains("quantileExact(0.5)(v_period) AS median")
                && q.contains("quantileExact(0.75)(v_period) AS p75"),
            "delivery_query must use plain quantileExact for the quartiles, got:\n{q}"
        );
        // P95 range_max for the time-tail keys.
        assert!(
            q.contains("quantileExact(0.95)(v_period)"),
            "delivery_query range_max must use the P95 expr for time-tail keys, got:\n{q}"
        );
    }

    #[test]
    fn collab_query_shape() {
        let q = collab_query();
        assert_common_shape(
            &q,
            "collab_query",
            "insight.collab_bullet_rows",
            "meeting_hours",
        );
        assert!(
            q.contains("quantileExact(0.25)(v_period) AS p25")
                && q.contains("quantileExact(0.5)(v_period) AS median")
                && q.contains("quantileExact(0.75)(v_period) AS p75"),
            "collab_query must use plain quantileExact for the quartiles, got:\n{q}"
        );
        // Plain max for range_max (no P95 tail capping in collab).
        assert!(
            q.contains("max(v_period) AS range_max"),
            "collab_query range_max must be plain max(v_period), got:\n{q}"
        );
    }

    #[test]
    fn git_query_shape() {
        let q = git_query();
        assert_common_shape(&q, "git_query", "insight.git_bullet_rows", "prs_merged");
        // Git uses the *If family gated on isNotNull(v_period).
        assert!(
            q.contains("quantileExactIf(0.25)(v_period, isNotNull(v_period)) AS p25")
                && q.contains("quantileExactIf(0.5)(v_period, isNotNull(v_period)) AS median")
                && q.contains("quantileExactIf(0.75)(v_period, isNotNull(v_period)) AS p75"),
            "git_query quartiles must use quantileExactIf(..., isNotNull(v_period)), got:\n{q}"
        );
        assert!(
            q.contains("minIf(v_period, isNotNull(v_period)) AS range_min")
                && q.contains("maxIf(v_period, isNotNull(v_period)) AS range_max")
                && q.contains("countIf(isNotNull(v_period)) AS n"),
            "git_query min/max/n must use the *If family gated on isNotNull, got:\n{q}"
        );
    }
}
