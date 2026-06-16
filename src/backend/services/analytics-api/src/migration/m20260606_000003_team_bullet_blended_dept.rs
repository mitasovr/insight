//! Rebase the four Team Bullet `query_ref`s onto a headcount-weighted
//! blend of their members' DEPARTMENT cohorts, replacing the company-wide
//! cohort installed by `m20260604_00000{1,2,3,4}`.
//!
//! Today each team bullet joins a single company-wide cohort and surfaces it
//! with `any(c.*)` — every team sees the same company band regardless of who
//! is on the roster. With peer cohorts consolidated to the department level,
//! the team band should be the blend of the departments actually represented
//! in the roster. We achieve that by:
//!   1. swapping the company cohort for the section's DEPARTMENT cohort
//!      (the cohort subquery grouped by `metric_key, org_unit_id`, joined on
//!      `c.metric_key = p.metric_key AND c.org_unit_id = p.org_unit_id`); and
//!   2. changing the outer cohort aggregation from `any(c.*)` to `avg(c.*)`,
//!      so a department's band contributes once per roster member in it —
//!      i.e. a headcount-weighted average over the roster.
//!
//! The cohort size `n` becomes `toFloat64(count(p.v_period))` (the roster's
//! contributing rows). The VALUE column is unchanged from each section's
//! current team query.
//!
//! Roster scoping stays at the handler (`person_id IN (roster)`), so the
//! outer keeps `GROUP BY p.metric_key`. Both leaves keep `GROUP BY
//! person_id`, so the date-walker injects the `metric_date` range before each
//! per-person GROUP BY exactly as before.
//!
//! For Delivery (`…0003`) and Collab (`…0005`) the department cohort is
//! exactly the section's existing IC query (`m20260604_00000{1,2}::ic_query`):
//! we copy it, swap `any(c.team_*)` → `avg(c.team_*)`, and set `n`.
//! For AI (`…0006`) we copy `m20260604_000003::ic_query` (department cohort
//! with the active-counter `multiIf` that already NULLs p25/p75 for the
//! `ACTIVE_LIST` keys) and keep the active-aware value `multiIf(... sum, avg)`;
//! the active keys' `team_p25`/`team_p75` are NULL so `avg(NULL)` stays NULL (neutral,
//! correct). For Code Quality (`…0004`) there is no IC query, so we convert
//! its current company cohort into a department cohort (add `org_unit_id` to
//! the cohort SELECT + GROUP BY, join on `org_unit_id`) before blending.
//!
//! `down()` for each restores the verbatim CURRENT team query (company
//! cohort, `any(c.*)`) — exactly what `m20260604_00000{1,2,3,4}` install.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

const TEAM_BULLET_DELIVERY_ID: &str = "00000000000000000001000000000003";
const TEAM_BULLET_CODE_QUALITY_ID: &str = "00000000000000000001000000000004";
const TEAM_BULLET_COLLAB_ID: &str = "00000000000000000001000000000005";
const TEAM_BULLET_AI_ID: &str = "00000000000000000001000000000006";

// ── Task Delivery (…0003) ────────────────────────────────────────────────

const DELIVERY_P95_LIST: &str = "'mean_time_to_resolution', 'task_dev_time', 'pickup_time'";

/// Inner wide-aggregate block, copied verbatim from
/// `m20260604_000001_task_delivery_bullet_distribution::wide_aggregate_pp`.
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

fn delivery_range_max_expr() -> String {
    format!("if(metric_key IN ({DELIVERY_P95_LIST}), quantileExact(0.95)(v_period), max(v_period))")
}

/// NEW delivery team query: department cohort (from `m20260604_000001::
/// ic_query`) blended with `avg(c.team_*)`; value unchanged (`avg(p.v_period)`).
fn delivery_new_team_query() -> String {
    let pp = delivery_wide_aggregate_pp();
    let kv = delivery_array_join_kv();
    let rmax = delivery_range_max_expr();
    format!(
        "SELECT p.metric_key AS metric_key, \
                avg(p.v_period) AS value, \
                avg(c.team_median) AS median, \
                avg(c.team_min) AS range_min, \
                avg(c.team_max) AS range_max, \
                avg(c.team_p25) AS p25, \
                avg(c.team_p75) AS p75, \
                toFloat64(count(p.v_period)) AS n \
         FROM ( \
             SELECT person_id, org_unit_id, \
                    kv.1 AS metric_key, kv.2 AS v_period \
             FROM ({pp}) pp \
             {kv} \
         ) p \
         LEFT JOIN ( \
             SELECT metric_key, org_unit_id, \
                    quantileExact(0.5)(v_period) AS team_median, \
                    min(v_period) AS team_min, \
                    {rmax} AS team_max, \
                    quantileExact(0.25)(v_period) AS team_p25, \
                    quantileExact(0.75)(v_period) AS team_p75, \
                    count(v_period) AS team_n \
             FROM ( \
                 SELECT person_id, org_unit_id, \
                        kv.1 AS metric_key, kv.2 AS v_period \
                 FROM ({pp}) ppc \
                 {kv} \
             ) inner_c \
             GROUP BY metric_key, org_unit_id \
         ) c ON c.metric_key = p.metric_key AND c.org_unit_id = p.org_unit_id \
         GROUP BY p.metric_key"
    )
}

/// CURRENT delivery team query (company cohort, `any(c.*)`), copied verbatim
/// from `m20260604_000001::team_query`. Restored by `down()`.
fn delivery_old_team_query() -> String {
    let pp = delivery_wide_aggregate_pp();
    let kv = delivery_array_join_kv();
    let rmax = delivery_range_max_expr();
    format!(
        "SELECT p.metric_key AS metric_key, \
                avg(p.v_period) AS value, \
                any(c.company_median) AS median, \
                any(c.company_min) AS range_min, \
                any(c.company_max) AS range_max, \
                any(c.company_p25) AS p25, \
                any(c.company_p75) AS p75, \
                any(c.company_n) AS n \
         FROM ( \
             SELECT person_id, org_unit_id, \
                    kv.1 AS metric_key, kv.2 AS v_period \
             FROM ({pp}) pp \
             {kv} \
         ) p \
         LEFT JOIN ( \
             SELECT metric_key, \
                    quantileExact(0.5)(v_period) AS company_median, \
                    min(v_period) AS company_min, \
                    {rmax} AS company_max, \
                    quantileExact(0.25)(v_period) AS company_p25, \
                    quantileExact(0.75)(v_period) AS company_p75, \
                    count(v_period) AS company_n \
             FROM ( \
                 SELECT kv.1 AS metric_key, kv.2 AS v_period \
                 FROM ({pp}) ppc \
                 {kv} \
             ) inner_c \
             GROUP BY metric_key \
         ) c ON c.metric_key = p.metric_key \
         GROUP BY p.metric_key"
    )
}

// ── Collaboration (…0005) ────────────────────────────────────────────────

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

/// NEW collab team query: department cohort (from `m20260604_000002::
/// ic_query`) blended with `avg(c.team_*)`; value unchanged.
fn collab_new_team_query() -> String {
    let pp = collab_wide_aggregate_pp();
    let kv = collab_array_join_kv();
    format!(
        "SELECT p.metric_key AS metric_key, \
                avg(p.v_period) AS value, \
                avg(c.team_median) AS median, \
                avg(c.team_min) AS range_min, \
                avg(c.team_max) AS range_max, \
                avg(c.team_p25) AS p25, \
                avg(c.team_p75) AS p75, \
                toFloat64(count(p.v_period)) AS n \
         FROM ( \
             SELECT person_id, org_unit_id, \
                    kv.1 AS metric_key, kv.2 AS v_period \
             FROM ({pp}) pp \
             {kv} \
         ) p \
         LEFT JOIN ( \
             SELECT metric_key, org_unit_id, \
                    quantileExact(0.5)(v_period) AS team_median, \
                    min(v_period) AS team_min, \
                    max(v_period) AS team_max, \
                    quantileExact(0.25)(v_period) AS team_p25, \
                    quantileExact(0.75)(v_period) AS team_p75, \
                    count(v_period) AS team_n \
             FROM ( \
                 SELECT person_id, org_unit_id, \
                        kv.1 AS metric_key, kv.2 AS v_period \
                 FROM ({pp}) ppc \
                 {kv} \
             ) inner_c \
             GROUP BY metric_key, org_unit_id \
         ) c ON c.metric_key = p.metric_key AND c.org_unit_id = p.org_unit_id \
         GROUP BY p.metric_key"
    )
}

/// CURRENT collab team query (company cohort, `any(c.*)`), copied verbatim
/// from `m20260604_000002::team_query`. Restored by `down()`.
fn collab_old_team_query() -> String {
    let pp = collab_wide_aggregate_pp();
    let kv = collab_array_join_kv();
    format!(
        "SELECT p.metric_key AS metric_key, \
                avg(p.v_period) AS value, \
                any(c.company_median) AS median, \
                any(c.company_min) AS range_min, \
                any(c.company_max) AS range_max, \
                any(c.company_p25) AS p25, \
                any(c.company_p75) AS p75, \
                any(c.company_n) AS n \
         FROM ( \
             SELECT person_id, org_unit_id, \
                    kv.1 AS metric_key, kv.2 AS v_period \
             FROM ({pp}) pp \
             {kv} \
         ) p \
         LEFT JOIN ( \
             SELECT metric_key, \
                    quantileExact(0.5)(v_period) AS company_median, \
                    min(v_period) AS company_min, \
                    max(v_period) AS company_max, \
                    quantileExact(0.25)(v_period) AS company_p25, \
                    quantileExact(0.75)(v_period) AS company_p75, \
                    count(v_period) AS company_n \
             FROM ( \
                 SELECT kv.1 AS metric_key, kv.2 AS v_period \
                 FROM ({pp}) ppc \
                 {kv} \
             ) inner_c \
             GROUP BY metric_key \
         ) c ON c.metric_key = p.metric_key \
         GROUP BY p.metric_key"
    )
}

// ── AI (…0006) ───────────────────────────────────────────────────────────

const AI_ACTIVE_LIST: &str = "'active_ai_members', 'cursor_active', 'cc_active', 'codex_active'";

fn ai_wide_aggregate_pp() -> &'static str {
    "SELECT person_id, any(org_unit_id) AS org_unit_id, \
         if(countIf(metric_key = 'active_ai_members') > 0, toFloat64(1), CAST(NULL AS Nullable(Float64))) AS active_ai_members_v, \
         if(countIf(metric_key = 'cursor_active') > 0, toFloat64(1), CAST(NULL AS Nullable(Float64))) AS cursor_active_v, \
         if(countIf(metric_key = 'cc_active') > 0, toFloat64(1), CAST(NULL AS Nullable(Float64))) AS cc_active_v, \
         sumIf(metric_value, metric_key = 'cursor_completions') AS cursor_completions_v, \
         sumIf(metric_value, metric_key = 'cursor_agents') AS cursor_agents_v, \
         sumIf(metric_value, metric_key = 'cursor_lines') AS cursor_lines_v, \
         sumIf(metric_value, metric_key = 'cc_sessions') AS cc_sessions_v, \
         sumIf(metric_value, metric_key = 'cc_lines') AS cc_lines_v, \
         sumIf(metric_value, metric_key = 'cc_tool_accept') AS cc_tool_accept_v, \
         sumIf(metric_value, metric_key = 'team_ai_loc') AS team_ai_loc_v, \
         if(sumIf(metric_value, metric_key = 'cursor_offered') > 0, \
            round(toFloat64(100) \
                  * sumIf(metric_value, metric_key = 'cursor_completions') \
                  / sumIf(metric_value, metric_key = 'cursor_offered'), 1), \
            CAST(NULL AS Nullable(Float64))) AS cursor_acceptance_v, \
         if(sumIf(metric_value, metric_key = 'cc_offered') > 0, \
            round(toFloat64(100) \
                  * sumIf(metric_value, metric_key = 'cc_tool_accept') \
                  / sumIf(metric_value, metric_key = 'cc_offered'), 1), \
            CAST(NULL AS Nullable(Float64))) AS cc_tool_acceptance_v, \
         if(sumIf(metric_value, metric_key = 'cursor_total_lines') > 0, \
            round(toFloat64(100) \
                  * sumIf(metric_value, metric_key = 'cursor_lines') \
                  / sumIf(metric_value, metric_key = 'cursor_total_lines'), 1), \
            CAST(NULL AS Nullable(Float64))) AS ai_loc_share2_v, \
         CAST(NULL AS Nullable(Float64)) AS codex_active_v, \
         CAST(NULL AS Nullable(Float64)) AS chatgpt_v, \
         CAST(NULL AS Nullable(Float64)) AS claude_web_v \
     FROM insight.ai_bullet_rows \
     GROUP BY person_id"
}

fn ai_array_join_kv() -> &'static str {
    "ARRAY JOIN [ \
         ('active_ai_members',  active_ai_members_v), \
         ('cursor_active',      cursor_active_v), \
         ('cc_active',          cc_active_v), \
         ('cursor_completions', cursor_completions_v), \
         ('cursor_agents',      cursor_agents_v), \
         ('cursor_lines',       cursor_lines_v), \
         ('cc_sessions',        cc_sessions_v), \
         ('cc_lines',           cc_lines_v), \
         ('cc_tool_accept',     cc_tool_accept_v), \
         ('team_ai_loc',        team_ai_loc_v), \
         ('cursor_acceptance',  cursor_acceptance_v), \
         ('cc_tool_acceptance', cc_tool_acceptance_v), \
         ('ai_loc_share2',      ai_loc_share2_v), \
         ('codex_active',       codex_active_v), \
         ('chatgpt',            chatgpt_v), \
         ('claude_web',         claude_web_v) \
     ] AS kv"
}

/// NEW AI team query: department cohort (from `m20260604_000003::ic_query`,
/// with the active-counter `multiIf` that nulls `p25`/`p75` for `ACTIVE_LIST` keys)
/// blended with `avg(c.team_*)`. Value keeps the active-aware multiIf.
fn ai_new_team_query() -> String {
    let pp = ai_wide_aggregate_pp();
    let kv = ai_array_join_kv();
    format!(
        "SELECT p.metric_key AS metric_key, \
                multiIf(p.metric_key IN ({AI_ACTIVE_LIST}), sum(p.v_period), avg(p.v_period)) AS value, \
                avg(c.team_median) AS median, \
                avg(c.team_min) AS range_min, \
                avg(c.team_max) AS range_max, \
                avg(c.team_p25) AS p25, \
                avg(c.team_p75) AS p75, \
                toFloat64(count(p.v_period)) AS n \
         FROM ( \
             SELECT person_id, org_unit_id, \
                    kv.1 AS metric_key, kv.2 AS v_period \
             FROM ({pp}) pp \
             {kv} \
         ) p \
         LEFT JOIN ( \
             SELECT metric_key, org_unit_id, \
                    multiIf(metric_key IN ({AI_ACTIVE_LIST}), \
                            if(count(v_period) = 0, CAST(NULL AS Nullable(Float64)), toFloat64(0)), \
                            quantileExact(0.5)(v_period)) AS team_median, \
                    multiIf(metric_key IN ({AI_ACTIVE_LIST}), \
                            if(count(v_period) = 0, CAST(NULL AS Nullable(Float64)), toFloat64(0)), \
                            min(v_period)) AS team_min, \
                    multiIf(metric_key IN ({AI_ACTIVE_LIST}), \
                            if(count(v_period) = 0, CAST(NULL AS Nullable(Float64)), toFloat64(count())), \
                            max(v_period)) AS team_max, \
                    multiIf(metric_key IN ({AI_ACTIVE_LIST}), \
                            CAST(NULL AS Nullable(Float64)), \
                            quantileExact(0.25)(v_period)) AS team_p25, \
                    multiIf(metric_key IN ({AI_ACTIVE_LIST}), \
                            CAST(NULL AS Nullable(Float64)), \
                            quantileExact(0.75)(v_period)) AS team_p75, \
                    multiIf(metric_key IN ({AI_ACTIVE_LIST}), \
                            if(count(v_period) = 0, CAST(NULL AS Nullable(Float64)), toFloat64(count())), \
                            toFloat64(count(v_period))) AS team_n \
             FROM ( \
                 SELECT person_id, org_unit_id, \
                        kv.1 AS metric_key, kv.2 AS v_period \
                 FROM ({pp}) ppc \
                 {kv} \
             ) inner_c \
             GROUP BY metric_key, org_unit_id \
         ) c ON c.metric_key = p.metric_key AND c.org_unit_id = p.org_unit_id \
         GROUP BY p.metric_key"
    )
}

/// CURRENT AI team query (company cohort, `any(c.*)`), copied verbatim from
/// `m20260604_000003::team_query`. Restored by `down()`.
fn ai_old_team_query() -> String {
    let pp = ai_wide_aggregate_pp();
    let kv = ai_array_join_kv();
    format!(
        "SELECT p.metric_key AS metric_key, \
                multiIf(p.metric_key IN ({AI_ACTIVE_LIST}), sum(p.v_period), avg(p.v_period)) AS value, \
                any(c.company_median) AS median, \
                any(c.company_min) AS range_min, \
                any(c.company_max) AS range_max, \
                any(c.company_p25) AS p25, \
                any(c.company_p75) AS p75, \
                any(c.company_n) AS n \
         FROM ( \
             SELECT person_id, org_unit_id, \
                    kv.1 AS metric_key, kv.2 AS v_period \
             FROM ({pp}) pp \
             {kv} \
         ) p \
         LEFT JOIN ( \
             SELECT metric_key, \
                    multiIf(metric_key IN ({AI_ACTIVE_LIST}), \
                            if(count(v_period) = 0, CAST(NULL AS Nullable(Float64)), toFloat64(0)), \
                            quantileExact(0.5)(v_period)) AS company_median, \
                    multiIf(metric_key IN ({AI_ACTIVE_LIST}), \
                            if(count(v_period) = 0, CAST(NULL AS Nullable(Float64)), toFloat64(0)), \
                            min(v_period)) AS company_min, \
                    multiIf(metric_key IN ({AI_ACTIVE_LIST}), \
                            if(count(v_period) = 0, CAST(NULL AS Nullable(Float64)), toFloat64(count())), \
                            max(v_period)) AS company_max, \
                    multiIf(metric_key IN ({AI_ACTIVE_LIST}), \
                            CAST(NULL AS Nullable(Float64)), \
                            quantileExact(0.25)(v_period)) AS company_p25, \
                    multiIf(metric_key IN ({AI_ACTIVE_LIST}), \
                            CAST(NULL AS Nullable(Float64)), \
                            quantileExact(0.75)(v_period)) AS company_p75, \
                    multiIf(metric_key IN ({AI_ACTIVE_LIST}), \
                            if(count(v_period) = 0, CAST(NULL AS Nullable(Float64)), toFloat64(count())), \
                            toFloat64(count(v_period))) AS company_n \
             FROM ( \
                 SELECT kv.1 AS metric_key, kv.2 AS v_period \
                 FROM ({pp}) ppc \
                 {kv} \
             ) inner_c \
             GROUP BY metric_key \
         ) c ON c.metric_key = p.metric_key \
         GROUP BY p.metric_key"
    )
}

// ── Code Quality (…0004) ──────────────────────────────────────────────────

fn code_quality_wide_aggregate_pp() -> &'static str {
    "SELECT person_id, any(org_unit_id) AS org_unit_id, \
         sumIf(metric_value, metric_key = 'bugs_fixed') AS bugs_fixed_v, \
         CAST(NULL AS Nullable(Float64)) AS prs_per_dev_v, \
         CAST(NULL AS Nullable(Float64)) AS pr_cycle_time_v, \
         CAST(NULL AS Nullable(Float64)) AS build_success_v \
     FROM insight.code_quality_bullet_rows \
     GROUP BY person_id"
}

fn code_quality_array_join_kv() -> &'static str {
    "ARRAY JOIN [ \
         ('bugs_fixed',    bugs_fixed_v), \
         ('prs_per_dev',   prs_per_dev_v), \
         ('pr_cycle_time', pr_cycle_time_v), \
         ('build_success', build_success_v) \
     ] AS kv"
}

/// NEW code-quality team query: the current company cohort converted to a
/// DEPARTMENT cohort (add `org_unit_id` to the cohort SELECT + GROUP BY, join
/// on `org_unit_id`), blended with `avg(c.company_*)`. Value unchanged.
fn code_quality_new_team_query() -> String {
    let pp = code_quality_wide_aggregate_pp();
    let kv = code_quality_array_join_kv();
    format!(
        "SELECT p.metric_key AS metric_key, \
                avg(p.v_period) AS value, \
                avg(c.company_median) AS median, \
                avg(c.company_min) AS range_min, \
                avg(c.company_max) AS range_max, \
                avg(c.company_p25) AS p25, \
                avg(c.company_p75) AS p75, \
                toFloat64(count(p.v_period)) AS n \
         FROM ( \
             SELECT person_id, org_unit_id, \
                    kv.1 AS metric_key, kv.2 AS v_period \
             FROM ({pp}) pp \
             {kv} \
         ) p \
         LEFT JOIN ( \
             SELECT metric_key, org_unit_id, \
                    quantileExact(0.5)(v_period) AS company_median, \
                    min(v_period) AS company_min, \
                    max(v_period) AS company_max, \
                    quantileExact(0.25)(v_period) AS company_p25, \
                    quantileExact(0.75)(v_period) AS company_p75, \
                    count(v_period) AS company_n \
             FROM ( \
                 SELECT person_id, org_unit_id, \
                        kv.1 AS metric_key, kv.2 AS v_period \
                 FROM ({pp}) ppc \
                 {kv} \
             ) inner_c \
             GROUP BY metric_key, org_unit_id \
         ) c ON c.metric_key = p.metric_key AND c.org_unit_id = p.org_unit_id \
         GROUP BY p.metric_key"
    )
}

/// CURRENT code-quality team query (company cohort, `any(c.*)`), copied
/// verbatim from `m20260604_000004::team_query`. Restored by `down()`.
fn code_quality_old_team_query() -> String {
    let pp = code_quality_wide_aggregate_pp();
    let kv = code_quality_array_join_kv();
    format!(
        "SELECT p.metric_key AS metric_key, \
                avg(p.v_period) AS value, \
                any(c.company_median) AS median, \
                any(c.company_min) AS range_min, \
                any(c.company_max) AS range_max, \
                any(c.company_p25) AS p25, \
                any(c.company_p75) AS p75, \
                any(c.company_n) AS n \
         FROM ( \
             SELECT person_id, org_unit_id, \
                    kv.1 AS metric_key, kv.2 AS v_period \
             FROM ({pp}) pp \
             {kv} \
         ) p \
         LEFT JOIN ( \
             SELECT metric_key, \
                    quantileExact(0.5)(v_period) AS company_median, \
                    min(v_period) AS company_min, \
                    max(v_period) AS company_max, \
                    quantileExact(0.25)(v_period) AS company_p25, \
                    quantileExact(0.75)(v_period) AS company_p75, \
                    count(v_period) AS company_n \
             FROM ( \
                 SELECT kv.1 AS metric_key, kv.2 AS v_period \
                 FROM ({pp}) ppc \
                 {kv} \
             ) inner_c \
             GROUP BY metric_key \
         ) c ON c.metric_key = p.metric_key \
         GROUP BY p.metric_key"
    )
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        for (hex_id, query) in [
            (TEAM_BULLET_DELIVERY_ID, delivery_new_team_query()),
            (TEAM_BULLET_CODE_QUALITY_ID, code_quality_new_team_query()),
            (TEAM_BULLET_COLLAB_ID, collab_new_team_query()),
            (TEAM_BULLET_AI_ID, ai_new_team_query()),
        ] {
            db.execute_unprepared(&format!(
                "UPDATE metrics SET query_ref = '{qr}' WHERE id = UNHEX('{hex_id}')",
                qr = query.replace('\'', "''"),
            ))
            .await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        for (hex_id, query) in [
            (TEAM_BULLET_DELIVERY_ID, delivery_old_team_query()),
            (TEAM_BULLET_CODE_QUALITY_ID, code_quality_old_team_query()),
            (TEAM_BULLET_COLLAB_ID, collab_old_team_query()),
            (TEAM_BULLET_AI_ID, ai_old_team_query()),
        ] {
            db.execute_unprepared(&format!(
                "UPDATE metrics SET query_ref = '{qr}' WHERE id = UNHEX('{hex_id}')",
                qr = query.replace('\'', "''"),
            ))
            .await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Common shape for a blended-department team query: the cohort is joined
    /// on `c.org_unit_id = p.org_unit_id` (department, not company-wide), the
    /// cohort columns are surfaced via `avg(c.` (headcount-weighted blend, not
    /// `any(c.`), the size is `toFloat64(count(p.v_period)) AS n`, and the
    /// roster scope stays at the handler (`GROUP BY p.metric_key`, no people
    /// join).
    fn assert_blended_dept_shape(query: &str, label: &str, cohort_prefix: &str) {
        assert!(
            query.contains("c.org_unit_id = p.org_unit_id"),
            "{label}: new team query must join the department cohort on org_unit_id, got:\n{query}"
        );
        for col in ["median", "min", "max", "p25", "p75"] {
            let blended = format!("avg(c.{cohort_prefix}_{col})");
            assert!(
                query.contains(&blended),
                "{label}: cohort column must blend via `{blended}` (not any(c.…)), got:\n{query}"
            );
        }
        assert!(
            !query.contains(&format!("any(c.{cohort_prefix}_")),
            "{label}: new team query must NOT use any(c.{cohort_prefix}_…) for the cohort columns, got:\n{query}"
        );
        assert!(
            query.contains("toFloat64(count(p.v_period)) AS n"),
            "{label}: cohort size must be toFloat64(count(p.v_period)) AS n, got:\n{query}"
        );
        assert!(
            query.contains("GROUP BY p.metric_key"),
            "{label}: outer GROUP BY must stay metric_key (roster scope at handler), got:\n{query}"
        );
        assert!(
            !query.contains("insight.people") && !query.contains("supervisor_email"),
            "{label}: new team query must NOT introduce a people/supervisor join, got:\n{query}"
        );
    }

    /// Common shape for the restored `down()` company-cohort team query: the
    /// cohort is joined on `c.metric_key = p.metric_key` alone (no `org_unit_id`
    /// pairing), and the cohort columns are surfaced via `any(c.<prefix>_…`.
    fn assert_company_cohort_shape(query: &str, label: &str, cohort_prefix: &str) {
        assert!(
            query.contains("ON c.metric_key = p.metric_key")
                && !query.contains("c.org_unit_id = p.org_unit_id"),
            "{label}: down() must restore the company-cohort join (metric_key alone), got:\n{query}"
        );
        assert!(
            query.contains(&format!("any(c.{cohort_prefix}_median)")),
            "{label}: down() cohort columns must use any(c.{cohort_prefix}_…), got:\n{query}"
        );
        assert!(
            !query.contains(&format!("avg(c.{cohort_prefix}_")),
            "{label}: down() must NOT use avg(c.{cohort_prefix}_…), got:\n{query}"
        );
    }

    #[test]
    fn delivery_new_team_query_blends_department() {
        let q = delivery_new_team_query();
        assert_blended_dept_shape(&q, "delivery_new_team_query", "team");
        // Value unchanged: plain avg(p.v_period).
        assert!(
            q.contains("avg(p.v_period) AS value"),
            "delivery value must stay avg(p.v_period), got:\n{q}"
        );
        // Department cohort grouped per metric_key, org_unit_id.
        assert!(
            q.contains("GROUP BY metric_key, org_unit_id"),
            "delivery cohort must group per metric_key, org_unit_id, got:\n{q}"
        );
        // Time-tail range_max keeps the P95 expr.
        assert!(
            q.contains("quantileExact(0.95)(v_period)"),
            "delivery cohort team_max must keep the P95 expr, got:\n{q}"
        );
    }

    #[test]
    fn collab_new_team_query_blends_department() {
        let q = collab_new_team_query();
        assert_blended_dept_shape(&q, "collab_new_team_query", "team");
        assert!(
            q.contains("avg(p.v_period) AS value"),
            "collab value must stay avg(p.v_period), got:\n{q}"
        );
        assert!(
            q.contains("GROUP BY metric_key, org_unit_id"),
            "collab cohort must group per metric_key, org_unit_id, got:\n{q}"
        );
    }

    #[test]
    fn ai_new_team_query_blends_department_and_keeps_active_multiif() {
        let q = ai_new_team_query();
        assert_blended_dept_shape(&q, "ai_new_team_query", "team");
        // Value KEEPS the active-aware multiIf (sum for active, avg otherwise).
        assert!(
            q.contains(&format!(
                "multiIf(p.metric_key IN ({AI_ACTIVE_LIST}), sum(p.v_period), avg(p.v_period)) AS value"
            )),
            "ai value must keep the active-counter multiIf, got:\n{q}"
        );
        // Active keys' p25/p75 are still NULLed in the cohort via the multiIf
        // (the active-counter wrapper from m20260604_000003::ic_query).
        assert!(
            q.contains(&format!("multiIf(metric_key IN ({AI_ACTIVE_LIST}),")),
            "ai cohort must keep the active-counter multiIf NULLing p25/p75, got:\n{q}"
        );
        assert!(
            q.contains("quantileExact(0.25)(v_period)) AS team_p25")
                && q.contains("quantileExact(0.75)(v_period)) AS team_p75"),
            "ai cohort must keep the real quartile branch for non-active keys, got:\n{q}"
        );
        assert!(
            q.contains("GROUP BY metric_key, org_unit_id"),
            "ai cohort must group per metric_key, org_unit_id, got:\n{q}"
        );
    }

    #[test]
    fn code_quality_new_team_query_blends_department() {
        let q = code_quality_new_team_query();
        // Code quality has no IC query, so the (former company) cohort columns
        // keep the `company_*` labels but the join becomes department-scoped
        // and the blend uses avg(c.company_*).
        assert_blended_dept_shape(&q, "code_quality_new_team_query", "company");
        assert!(
            q.contains("avg(p.v_period) AS value"),
            "code quality value must stay avg(p.v_period), got:\n{q}"
        );
        // Cohort SELECT gains org_unit_id + groups per metric_key, org_unit_id.
        assert!(
            q.contains("SELECT metric_key, org_unit_id,"),
            "code quality cohort SELECT must add org_unit_id, got:\n{q}"
        );
        assert!(
            q.contains("GROUP BY metric_key, org_unit_id"),
            "code quality cohort must group per metric_key, org_unit_id, got:\n{q}"
        );
    }

    #[test]
    fn down_restores_company_cohort_for_each_section() {
        assert_company_cohort_shape(
            &delivery_old_team_query(),
            "delivery_old_team_query",
            "company",
        );
        assert_company_cohort_shape(&collab_old_team_query(), "collab_old_team_query", "company");
        assert_company_cohort_shape(&ai_old_team_query(), "ai_old_team_query", "company");
        assert_company_cohort_shape(
            &code_quality_old_team_query(),
            "code_quality_old_team_query",
            "company",
        );
        // AI down() keeps the active-aware value multiIf too.
        assert!(
            ai_old_team_query().contains(&format!(
                "multiIf(p.metric_key IN ({AI_ACTIVE_LIST}), sum(p.v_period), avg(p.v_period)) AS value"
            )),
            "ai down() must keep the active-counter value multiIf"
        );
    }
}
