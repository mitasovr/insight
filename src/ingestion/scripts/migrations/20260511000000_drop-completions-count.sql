-- =====================================================================
-- #262: drop completions_count from silver.class_ai_dev_usage
-- =====================================================================
--
-- In every staging model that feeds class_ai_dev_usage,
-- `completions_count` is mapped from the same source field as
-- `tool_use_accepted`:
--
--   cursor__ai_dev_usage         : both = totalTabsAccepted
--   claude_enterprise__ai_dev_usage: both = code_tool_accepted_count
--   claude_admin__ai_dev_usage    : both = NULL (no real data)
--   copilot__ai_dev_usage         : both = code_acceptance_activity_count
--
-- The two columns have therefore always been numerically identical at the
-- Silver class level, providing no semantic differentiation. Per issue
-- #262 we drop `completions_count` outright and re-source the
-- `cursor_completions` gold bullet from `tool_use_accepted` — same value,
-- no behavioural change for any downstream consumer.

ALTER TABLE silver.class_ai_dev_usage DROP COLUMN IF EXISTS completions_count;

-- Recreate `insight.ai_bullet_rows` without referencing completions_count.
-- The only line that changes vs the previous definition
-- (20260427180000_ai-bullet-rows-tool-filter.sql) is the `cursor_completions`
-- row, which now sources `c.tool_use_accepted` instead of
-- `c.completions_count`. Everything else is verbatim.

DROP VIEW IF EXISTS insight.ai_bullet_rows;

CREATE VIEW insight.ai_bullet_rows AS
SELECT
    lower(c.email)                                AS person_id,
    p.org_unit_id,
    c.day                                         AS metric_date,
    'active_ai_members'                           AS metric_key,
    toFloat64(1)                                  AS metric_value
FROM silver.class_ai_dev_usage AS c
LEFT JOIN insight.people AS p ON lower(c.email) = p.person_id
UNION ALL
SELECT lower(c.email), p.org_unit_id, c.day, 'cursor_active', toFloat64(1)
FROM silver.class_ai_dev_usage AS c
LEFT JOIN insight.people AS p ON lower(c.email) = p.person_id
WHERE c.tool = 'cursor'
UNION ALL
SELECT lower(c.email), p.org_unit_id, c.day, 'cursor_acceptance',
    if(toFloat64(coalesce(c.tool_use_offered, 0)) > 0,
       round((toFloat64(coalesce(c.tool_use_accepted, 0)) /
              toFloat64(c.tool_use_offered)) * 100, 1),
       CAST(NULL AS Nullable(Float64)))
FROM silver.class_ai_dev_usage AS c
LEFT JOIN insight.people AS p ON lower(c.email) = p.person_id
WHERE c.tool = 'cursor'
UNION ALL
SELECT lower(c.email), p.org_unit_id, c.day, 'cursor_completions',
    -- #262: was c.completions_count (numerically equal to tool_use_accepted).
    toFloat64(coalesce(c.tool_use_accepted, 0))
FROM silver.class_ai_dev_usage AS c
LEFT JOIN insight.people AS p ON lower(c.email) = p.person_id
WHERE c.tool = 'cursor'
UNION ALL
SELECT lower(c.email), p.org_unit_id, c.day, 'cursor_agents',
    toFloat64(coalesce(c.agent_sessions, 0))
FROM silver.class_ai_dev_usage AS c
LEFT JOIN insight.people AS p ON lower(c.email) = p.person_id
WHERE c.tool = 'cursor'
UNION ALL
SELECT lower(c.email), p.org_unit_id, c.day, 'cursor_lines',
    toFloat64(coalesce(c.lines_added, 0))
FROM silver.class_ai_dev_usage AS c
LEFT JOIN insight.people AS p ON lower(c.email) = p.person_id
WHERE c.tool = 'cursor'
UNION ALL
SELECT lower(c.email), p.org_unit_id, c.day, 'cc_active', toFloat64(1)
FROM silver.class_ai_dev_usage AS c
LEFT JOIN insight.people AS p ON lower(c.email) = p.person_id
WHERE c.tool = 'claude_code'
UNION ALL
SELECT lower(c.email), p.org_unit_id, c.day, 'cc_sessions',
    toFloat64(coalesce(c.session_count, 0))
FROM silver.class_ai_dev_usage AS c
LEFT JOIN insight.people AS p ON lower(c.email) = p.person_id
WHERE c.tool = 'claude_code'
UNION ALL
SELECT lower(c.email), p.org_unit_id, c.day, 'cc_lines',
    toFloat64(coalesce(c.lines_added, 0))
FROM silver.class_ai_dev_usage AS c
LEFT JOIN insight.people AS p ON lower(c.email) = p.person_id
WHERE c.tool = 'claude_code'
UNION ALL
SELECT lower(c.email), p.org_unit_id, c.day, 'cc_tool_accept',
    toFloat64(coalesce(c.tool_use_accepted, 0))
FROM silver.class_ai_dev_usage AS c
LEFT JOIN insight.people AS p ON lower(c.email) = p.person_id
WHERE c.tool = 'claude_code'
UNION ALL
SELECT lower(c.email), p.org_unit_id, c.day, 'cc_tool_acceptance',
    if(toFloat64(coalesce(c.tool_use_offered, 0)) > 0,
       round((toFloat64(coalesce(c.tool_use_accepted, 0)) /
              toFloat64(c.tool_use_offered)) * 100, 1),
       CAST(NULL AS Nullable(Float64)))
FROM silver.class_ai_dev_usage AS c
LEFT JOIN insight.people AS p ON lower(c.email) = p.person_id
WHERE c.tool = 'claude_code'
UNION ALL
SELECT lower(c.email), p.org_unit_id, c.day, 'codex_active',
    CAST(NULL AS Nullable(Float64))
FROM silver.class_ai_dev_usage AS c
LEFT JOIN insight.people AS p ON lower(c.email) = p.person_id
UNION ALL
SELECT lower(c.email), p.org_unit_id, c.day, 'team_ai_loc',
    toFloat64(coalesce(c.lines_added, 0))
FROM silver.class_ai_dev_usage AS c
LEFT JOIN insight.people AS p ON lower(c.email) = p.person_id
UNION ALL
SELECT lower(c.email), p.org_unit_id, c.day, 'ai_loc_share2',
    if(toFloat64(coalesce(c.total_lines_added, 0)) > 0,
       round((toFloat64(coalesce(c.lines_added, 0)) /
              toFloat64(c.total_lines_added)) * 100, 1),
       CAST(NULL AS Nullable(Float64)))
FROM silver.class_ai_dev_usage AS c
LEFT JOIN insight.people AS p ON lower(c.email) = p.person_id
WHERE c.tool = 'cursor'
UNION ALL
SELECT lower(c.email), p.org_unit_id, c.day, 'chatgpt',
    CAST(NULL AS Nullable(Float64))
FROM silver.class_ai_dev_usage AS c
LEFT JOIN insight.people AS p ON lower(c.email) = p.person_id
UNION ALL
SELECT lower(c.email), p.org_unit_id, c.day, 'claude_web',
    CAST(NULL AS Nullable(Float64))
FROM silver.class_ai_dev_usage AS c
LEFT JOIN insight.people AS p ON lower(c.email) = p.person_id;
