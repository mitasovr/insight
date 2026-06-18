-- =====================================================================
-- ai_bullet_rows — Claude Team overage (spend-over-limit) extension
-- =====================================================================
--
-- Surfaces per-seat Claude Team overage (silver.class_ai_overage) in the
-- AI bullet Gold view. Adds Branch 6; Branches 1–5 are unchanged from
-- 20260609000000_ai-chatgpt-team-gold.sql.
--
--   Branch 6 — Claude overage (metric_key = 'cc_overage'):
--     cc_overage ← class_ai_overage.overage_cents — cents a seat spent
--       ABOVE its monthly credit limit (max(0, used − limit)).
--   Source-scoped to source='claude_team' (the only overage source today;
--   future OpenAI overage gets its own metric_key, not this one).
--
--   Grain note: class_ai_overage is a per-seat MONTHLY snapshot (one row
--   per seat per billing month). We date each row at toDate(collected_at)
--   — the day we last read the snapshot — NOT period_month. The snapshot
--   for the current month is therefore always stamped with a recent date,
--   so it is captured by the dashboard's rolling date window (which filters
--   on metric_date); period_month (the 1st) would fall outside short
--   windows. sumIf over a multi-month window adds each month's closing
--   overage — total overage incurred in the window.
--
--   honest-NULL: rows are emitted ONLY where overage_cents IS NOT NULL
--   (a computable overage — the seat has a known limit). Seats with an
--   unknown limit emit no row, so the backend's countIf-guarded aggregate
--   renders them ComingSoon, never a fake $0. A seat within its limit
--   emits overage_cents = 0 (a real "no overage" reading → renders 0).
--
-- Paired backend migration:
--   m20260618_000001_ai_claude_team_overage_metric.rs — adds cc_overage to
--   the Team / IC Bullet AI query_ref (counter, avg per person, honest-NULL
--   guarded). Catalog metadata seeded by
--   m20260618_000002_seed_claude_team_overage_catalog.rs.
--
-- Idempotent: DROP VIEW IF EXISTS + CREATE VIEW.
-- =====================================================================

DROP VIEW IF EXISTS insight.ai_bullet_rows;

CREATE VIEW insight.ai_bullet_rows AS

-- ─── Branch 1: tool-agnostic (any dev AI tool counts) ────────────────
SELECT
    lower(c.email)                                 AS person_id,
    p.org_unit_id                                  AS org_unit_id,
    c.day                                          AS metric_date,
    kv.1                                           AS metric_key,
    kv.2                                           AS metric_value
FROM silver.class_ai_dev_usage AS c
LEFT JOIN insight.people AS p ON lower(c.email) = p.person_id
ARRAY JOIN [
    ('active_ai_members', toFloat64(1)),
    ('team_ai_loc',       toFloat64(coalesce(c.lines_added, 0)))
] AS kv
WHERE c.email IS NOT NULL AND c.email != ''

UNION ALL

-- ─── Branch 2: Cursor (tool = 'cursor') ──────────────────────────────
SELECT
    lower(c.email)                                 AS person_id,
    p.org_unit_id                                  AS org_unit_id,
    c.day                                          AS metric_date,
    kv.1                                           AS metric_key,
    kv.2                                           AS metric_value
FROM silver.class_ai_dev_usage AS c
LEFT JOIN insight.people AS p ON lower(c.email) = p.person_id
ARRAY JOIN [
    ('cursor_active',       toFloat64(1)),
    ('cursor_completions',  toFloat64(coalesce(c.tool_use_accepted, 0))),
    ('cursor_agents',       toFloat64(coalesce(c.agent_sessions,    0))),
    ('cursor_lines',        toFloat64(coalesce(c.lines_added,       0))),
    ('cursor_offered',      toFloat64(coalesce(c.tool_use_offered,  0))),
    ('cursor_total_lines',  toFloat64(coalesce(c.total_lines_added, 0)))
] AS kv
WHERE c.tool = 'cursor'
  AND c.email IS NOT NULL AND c.email != ''

UNION ALL

-- ─── Branch 3: Claude Code (tool = 'claude_code') ────────────────────
SELECT
    lower(c.email)                                 AS person_id,
    p.org_unit_id                                  AS org_unit_id,
    c.day                                          AS metric_date,
    kv.1                                           AS metric_key,
    kv.2                                           AS metric_value
FROM silver.class_ai_dev_usage AS c
LEFT JOIN insight.people AS p ON lower(c.email) = p.person_id
ARRAY JOIN [
    ('cc_active',       toFloat64(1)),
    ('cc_sessions',     toFloat64(coalesce(c.session_count,     0))),
    ('cc_lines',        toFloat64(coalesce(c.lines_added,       0))),
    ('cc_tool_accept',  toFloat64(coalesce(c.tool_use_accepted, 0))),
    ('cc_offered',      toFloat64(coalesce(c.tool_use_offered,  0))),
    ('cc_cost',         toFloat64(coalesce(c.cost_cents,        0)))
] AS kv
WHERE c.tool = 'claude_code'
  AND c.email IS NOT NULL AND c.email != ''

UNION ALL

-- ─── Branch 4: Codex (tool = 'codex', ChatGPT Team) ──────────────────
SELECT
    lower(c.email)                                 AS person_id,
    p.org_unit_id                                  AS org_unit_id,
    c.day                                          AS metric_date,
    kv.1                                           AS metric_key,
    kv.2                                           AS metric_value
FROM silver.class_ai_dev_usage AS c
LEFT JOIN insight.people AS p ON lower(c.email) = p.person_id
ARRAY JOIN [
    ('codex_active',   toFloat64(1)),
    ('codex_lines',    toFloat64(coalesce(c.lines_added,   0))),
    ('codex_sessions', toFloat64(coalesce(c.session_count, 0)))
] AS kv
WHERE c.tool = 'codex'
  AND c.email IS NOT NULL AND c.email != ''

UNION ALL

-- ─── Branch 5: ChatGPT chat (tool = 'chatgpt') ───────────────────────
-- Conversational surface — sourced from class_ai_assistant_usage.
SELECT
    lower(a.email)                                 AS person_id,
    p.org_unit_id                                  AS org_unit_id,
    a.day                                          AS metric_date,
    kv.1                                           AS metric_key,
    kv.2                                           AS metric_value
FROM silver.class_ai_assistant_usage AS a
LEFT JOIN insight.people AS p ON lower(a.email) = p.person_id
ARRAY JOIN [
    ('chatgpt_active', toFloat64(1)),
    ('chatgpt',        toFloat64(coalesce(a.message_count, 0)))
] AS kv
WHERE a.tool = 'chatgpt'
  AND a.email IS NOT NULL AND a.email != ''

UNION ALL

-- ─── Branch 6: Claude overage (metric_key = 'cc_overage') ────────────
-- Per-seat spend over the monthly credit limit — from class_ai_overage.
-- Dated at the snapshot's collection day (see header). Only computable
-- overages (overage_cents IS NOT NULL) are emitted.
SELECT
    lower(o.email)                                 AS person_id,
    p.org_unit_id                                  AS org_unit_id,
    toDate(o.collected_at)                         AS metric_date,
    'cc_overage'                                   AS metric_key,
    toFloat64(o.overage_cents)                     AS metric_value
FROM silver.class_ai_overage AS o
LEFT JOIN insight.people AS p ON lower(o.email) = p.person_id
WHERE o.source = 'claude_team'
  AND o.email IS NOT NULL AND o.email != ''
  AND o.overage_cents IS NOT NULL;
