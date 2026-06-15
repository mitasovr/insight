# DESIGN — GitLab Connector

> Status: draft for review
> Scope: self-hosted GitLab (CE & EE), REST API v4
> Type: CDK (Python), thin extractor per [ADR-0002](../../../../../domain/connector/specs/ADR/0002-connector-responsibility-scope.md)

## 1. Purpose & Scope

A typed Python CDK Airbyte source that extracts code, review, and flow data
from a self-hosted GitLab instance and emits source-native RECORD messages to
stdout. The connector transforms nothing: it pulls, trims oversized text,
attaches the platform envelope, and streams. All schema unification, identity
resolution, and metrics live downstream in dbt (silver/gold).

Design priorities, in order: **robustness → memory safety → throughput**. A
sync must never OOM, never accumulate unbounded state, and must complete or
fail loudly — never silently truncate coverage.

**Universality.** This is an open-source connector targeting no specific
instance. The same artifact must run correctly against any GitLab — CE or EE,
any supported version, any rate-limit configuration, any size, behind any
deployment. No behavior is simplified or tuned for a known first target;
characteristics of one instance are never assumed of another. Capabilities are
detected at runtime (§5.4), limits are honored as reported (§5.6).

### 1.1 Inherited contracts

- **Thin extractor** ([ADR-0002](../../../../../domain/connector/specs/ADR/0002-connector-responsibility-scope.md)): stdout only, zero DB access, no transformation.
- **Append-only sync**: `destinationSyncMode=append`. Dedup is a downstream RMT concern, never the connector's.
- **Envelope on every record**: `insight_tenant_id`, `insight_source_id`, `data_source`, `collected_at`, `unique_key`.
- **unique_key** ([ADR-0004](../../../../domain/ingestion-data-flow/specs/ADR/0004-unique-key-formula.md)): `{tenant}:{source}:{natural_key_parts}` joined with colons. Colon delimiter (not the ADR's literal dash) because git ref names forbid `:` but allow `-`/`/`, and the prefix parts already carry dashes (tenant UUID, `source_id`). Collision-free under two invariants: (A) `source_id` contains no colon; (B) only the terminal key component may contain a colon — all non-terminal parts are numeric ids or hex SHAs, making concatenation injective. Deviation from ADR-0004 is the git-connector convention.

## 2. Insight Objectives

Extraction is justified by the insights it must enable downstream. The data
captured must make all of the following derivable in dbt **without
re-extraction**:

| Family | Representative metrics | Primary inputs |
|---|---|---|
| Flow | MR cycle time by phase (code → open → first-review → approve → merge), throughput, WIP age/size | `merge_requests`, `merge_request_commits`, `merge_request_notes`, `merge_request_state_events`, `merge_request_approvals` |
| Review health | time-to-first-review, review load, comment depth, rubber-stamp detection, author↔reviewer graph | `merge_request_notes`, `merge_request_discussions`, `merge_request_approvals` |
| Volume | commits & lines per person/week, code-vs-test-vs-config split, size distributions | `commits` (+stats), `commit_file_changes` |
| Code health | file/module churn, hotspots (churn × authors), ownership | `commit_file_changes`, `commits` |
| Identity | resolve git email ↔ GitLab username ↔ person | `users`, every actor-bearing record |

## 3. Stream Catalog

Streams are GitLab-native and land unprefixed in the `bronze_gitlab` namespace
(e.g. `bronze_gitlab.merge_requests`). Field lists below are the
extraction-relevant projection, not the full API payload.

### 3.1 Structure / dimension streams

Small, bounded, refreshed in full each run (snapshot semantics; downstream RMT
keeps the latest version).

#### `projects`
Cursor: none (full snapshot). Natural key: `id`. Two enumeration modes, selected
by config with no extra flag:
- **Scoped** (`gitlab_groups` set): `GET /groups/:id/projects?include_subgroups=true`
  per configured group (offset pagination).
- **Whole instance** (`gitlab_groups` empty): `GET /projects?pagination=keyset&order_by=id&sort=asc`
  — every project the token can access (keyset). Full coverage requires an admin
  or broad service token; otherwise scope is whatever the token can see.

`gitlab_projects` adds explicit projects on top of either mode.

| Field | Type | Notes |
|---|---|---|
| `id` | Int | project id |
| `path_with_namespace` | String | `group/subgroup/repo` |
| `name`, `path` | String | |
| `namespace_id`, `namespace_full_path` | Int / String | group attribution |
| `default_branch` | String | defines trunk |
| `visibility` | String | private/internal/public |
| `archived` | Bool | |
| `created_at`, `last_activity_at` | DateTime | |
| `statistics_commit_count`, `statistics_repository_size` | Int | requires `statistics=true` (member/owner scope) |

#### `users`
Cursor: none. Natural key: `id`. Whole-instance scope (no groups/projects
configured) uses `/users` (keyset), naturally unique. Scoped runs use
`/groups/:id/members/all` and `/projects/:id/members/all`; a user in multiple
scopes is emitted once per scope and collapsed downstream by RMT on
`unique_key` — no in-connector dedup set, which keeps streams concurrency-safe.

| Field | Type | Notes |
|---|---|---|
| `id` | Int | |
| `username` | String | identity bridge to MR/note actors |
| `name` | String | |
| `public_email` / `email` | String | identity bridge to commit author email (often empty on CE) |
| `state` | String | active/blocked |

#### `branches`
Cursor: none. Natural key: `project_id` + `name`. **Listed only — never
commit-iterated.**

| Field | Type | Notes |
|---|---|---|
| `project_id` | Int | |
| `name` | String | |
| `commit_sha` | String | branch head |
| `default`, `protected`, `merged` | Bool | governance signal |

Label and milestone names are carried inline on the MR record
(`labels`/`milestone_id`); no standalone catalog stream is collected.

### 3.2 Code streams

#### `commits`
All-branch coverage via `?all=true` (server-side deduped single pass — no
per-branch fan-out, no client seen-set). `with_stats=true` returns
additions/deletions/total inline at no extra request.

Cursor: `committed_date` (`since` filter). Natural key: `project_id` + `id`.

| Field | Type | Notes |
|---|---|---|
| `project_id` | Int | |
| `id` (sha), `short_id` | String | |
| `title`, `message` | String | trimmed (§5.3) |
| `author_name`, `author_email` | String | identity key |
| `authored_date` | DateTime | |
| `committer_name`, `committer_email` | String | |
| `committed_date` | DateTime | cursor |
| `parent_ids` | Array(String) | `is_merge` derivable: `length > 1` |
| `stats_additions`, `stats_deletions`, `stats_total` | Int | from `with_stats` |
| `trailers` | Map | co-author / sign-off parsing downstream |

#### `commit_file_changes`
Per-file change for commits, from `GET /repository/commits/:sha/diff`. GitLab
returns no per-file counts → connector parses `+`/`-` hunk lines into
added/removed. Diffs flagged `too_large`/`collapsed` are emitted with counts
null and a `truncated` flag — never force-expanded.

Cursor: none of its own; driven by newly-seen commits. Natural key:
`project_id` + `sha` + `new_path`.

| Field | Type | Notes |
|---|---|---|
| `project_id`, `commit_sha` | Int / String | |
| `old_path`, `new_path` | String | |
| `new_file`, `deleted_file`, `renamed_file` | Bool | → change_type downstream |
| `lines_added`, `lines_removed` | Int | parsed from hunks; null if `too_large` |
| `diff_truncated` | Bool | true when GitLab elided the diff |

> Cost note: all-branch × per-commit diff is the dominant request cost. Which
> commits to diff (every commit vs MR-result granularity) is an **optimization
> decided in the optimization phase**, not here. Capture contract: per-file
> change for landed commits.

### 3.3 Review / flow streams

#### `merge_requests`
Cursor: `updated_at` (`updated_after` + `order_by=updated_at`,
`sort=asc`). Re-emitting a changed MR re-drives its children. Natural key:
`project_id` + `iid`.

| Field | Type | Notes |
|---|---|---|
| `project_id`, `iid`, `id` | Int | `iid` per-project, `id` global |
| `title`, `description` | String | trimmed (§5.3) |
| `state` | String | opened/closed/merged/locked |
| `draft` (`work_in_progress`) | Bool | |
| `author_id`, `author_username` | Int / String | |
| `assignee_ids`, `reviewer_ids` | Array(Int) | assigned reviewers |
| `merged_by_id`, `merged_by_username` | Int / String | |
| `source_branch`, `target_branch` | String | |
| `created_at`, `updated_at`, `merged_at`, `closed_at` | DateTime | phase anchors |
| `sha`, `merge_commit_sha`, `squash_commit_sha` | String | links MR → landed commit |
| `squash`, `squash_on_merge` | Bool | squash detection |
| `merge_status`, `changes_count`, `user_notes_count` | String / Int | |
| `labels`, `milestone_id` | Array(String) / Int | |

#### `merge_request_commits`
**Identity only** — no stats, no diffs. Captures squashed-away commit identity
and commit↔MR membership. Driven by parent MR. Natural key: `project_id` +
`mr_iid` + `sha`.

| Field | Type | Notes |
|---|---|---|
| `project_id`, `mr_iid` | Int | |
| `id` (sha), `short_id` | String | |
| `title`, `message` | String | trimmed |
| `author_name`, `author_email`, `authored_date` | String / DateTime | |
| `committed_date`, `parent_ids` | DateTime / Array | |
| `commit_order` | Int | position within MR |

#### `merge_request_notes`
User comments + system notes. Driven by parent MR. Natural key: `project_id` +
`note_id`.

| Field | Type | Notes |
|---|---|---|
| `project_id`, `mr_iid`, `id` | Int | |
| `body` | String | trimmed (§5.3) |
| `author_id`, `author_username` | Int / String | |
| `created_at`, `updated_at` | DateTime | |
| `system` | Bool | system note vs human comment |
| `resolvable`, `resolved`, `resolved_by_id` | Bool / Int | review-thread resolution |
| `position_new_path`, `position_new_line` | String / Int | diff-anchored comments (null for general) |

#### `merge_request_discussions`
Thread grouping for notes (which notes form one resolvable discussion). Driven
by parent MR. Natural key: `project_id` + `mr_iid` + `discussion_id` +
`note_id`.

| Field | Type | Notes |
|---|---|---|
| `project_id`, `mr_iid` | Int | |
| `discussion_id` | String | thread id |
| `note_id` | Int | member note |
| `individual_note` | Bool | standalone vs thread |

#### `merge_request_approvals`
EE feature — **degrades gracefully on CE** (§5.4). Driven by parent MR. Natural
key: `project_id` + `mr_iid` + `approver_user_id`.

| Field | Type | Notes |
|---|---|---|
| `project_id`, `mr_iid` | Int | |
| `approvals_required`, `approvals_left` | Int | |
| `approved_by_id`, `approved_by_username` | Int / String | one row per approver |

> Approval *timing* (when the click happened) is not on this endpoint. It is
> reconstructed downstream from `merge_request_state_events` /
> `merge_request_notes` system entries.

#### `merge_request_state_events`
Structured lifecycle events (opened/closed/reopened/merged) with actor +
timestamp. Cleaner than parsing system-note bodies. Driven by parent MR.
Natural key: `project_id` + `event_id`.

| Field | Type | Notes |
|---|---|---|
| `project_id`, `mr_iid`, `id` | Int | |
| `user_id`, `user_username` | Int / String | actor |
| `state` | String | merged/closed/reopened/opened |
| `created_at` | DateTime | event time |

### 3.4 Planning streams (recommended)

#### `issues`
Cursor: `updated_at`. Powers issue↔MR linking, ticket refs, issue cycle time.
Natural key: `project_id` + `iid`.

| Field | Type | Notes |
|---|---|---|
| `project_id`, `iid`, `id` | Int | |
| `title`, `description` | String | trimmed |
| `state` | String | opened/closed |
| `author_id`, `assignee_ids` | Int / Array | |
| `labels`, `milestone_id` | Array / Int | |
| `created_at`, `updated_at`, `closed_at` | DateTime | |

Issue notes / state events: same pattern as MR children, added when issue
analytics is built out.

### 3.5 Deferred (designed-for, separate milestone)

CI/CD → DORA: `pipelines`, `jobs`, `deployments`, `releases`. A distinct domain
(delivery vs code-review). Bronze namespacing leaves room; not in the first
cut.

## 4. State & Incremental Model

| Stream | Cursor | Strategy |
|---|---|---|
| `projects`, `users`, `branches` | — | full snapshot each run |
| `commits` | `committed_date` | `since` = last max committed_date |
| `commit_file_changes` | — | driven by newly-seen commits |
| `merge_requests`, `issues` | `updated_at` | `updated_after` = last max updated_at |
| MR / issue children | — | re-pulled in full when parent cursor advances |

State is per top-level stream only. Children are stateless — bounded by their
parent, re-emitted wholesale when the parent changes. This keeps state small
and removes any need for per-child cursors or seen-sets.

Commit incremental is date-based because commits are immutable (no
`updated_after`). A commit can be pushed with a `committed_date` older than the
cursor (rebase, history import, merge of an old branch), so a naive
`since = last_max` would miss it. Mitigation, baked in:

- **Trailing overlap**: query `since = last_max − OVERLAP` each run, not
  `last_max`. Re-emitting commits already in the overlap is idempotent — bronze
  is append-only and RMT dedups on `unique_key`. This recovers the common case
  (recently-dated commits pushed slightly late) as normal sync behavior, not a
  special path.
- **Long tail**: commits pushed with dates *older* than the overlap (ancient
  history imports) are recovered by a periodic full-refresh from
  `gitlab_start_date`. The connector supports full-refresh; cadence is an
  operational schedule decision, not a per-sync cost.

The exact OVERLAP value is fixed in code (no knob) when the stream is built and
validated. (Depends on §7 item 1 for the `all=true` traversal question.)

## 5. Memory & Robustness Model

### 5.1 Bounded streaming
Every stream is a generator. Top-level streams page lazily and yield each
record immediately. For each changed MR, its children are fetched, emitted, and
discarded before the next MR. Peak memory is O(one parent's children), never
O(repo). No global accumulation, no seen-hashes set anywhere.

Substream parents must stay bounded. A substream re-reads its parent's records
to build slices, and the CDK response-caches parent HTTP responses — so an
**unbounded** parent (e.g. `commits` as the parent of `file_changes`) would
balloon that cache. `projects` (bounded, ~pages) is a safe parent; for
`file_changes`, per-commit diffs are driven from within the `commits` traversal
(or a bounded per-project commit-sha partition), never by making the full
`commits` stream a cached substream parent. Each child stream also re-enumerates
`projects`; that redundant enumeration is acceptable while bounded and is
revisited in the concurrency/optimization phase (a shared project-id partition
source).

### 5.2 Pagination
Offset-based pagination is the universal mechanism and the only one most of our
endpoints support (`commits`, `merge_requests`, `branches`, group projects, MR
children, notes). Keyset pagination is documented only for `/projects`
(`order_by=id`) and `/users` — used there, offset everywhere else. `per_page` is
capped at 100.

Pages are followed via the `Link` header `rel="next"` until it is absent and the
array is empty — never by constructing page URLs, never relying on totals (above
10,000 results GitLab drops `x-total`/`x-total-pages`).

Offset pagination has a hard ceiling: the instance's max-allowed-offset (default
50,000) caps how deep one ordered query can page. Large collections must not be
paged past that wall — `commits` and `merge_requests` are **windowed by time**
(`since`/`until`, `updated_after`/`updated_before`) so each window stays shallow
on both backfill and incremental runs. Windowing doubles as a memory bound
(each request's result set stays small) and is what keeps the connector correct
on instances large enough to exceed the offset cap.

### 5.3 Text trimming (baked invariant, no config)
Oversized text is the primary per-record OOM vector. Hard caps applied at emit:

- `message`, `description`, note `body`: 16384 chars
- `title`: 1024 chars
- On truncation, set `<field>_truncated = true`.

Constants, not knobs — pathological multi-MB bodies (pasted logs, generated
changelogs) are cut before they reach memory or storage.

### 5.4 404 handling & capability degradation
GitLab overloads `404` across five meanings; the connector must skip the
benign ones (or it stalls on the first deleted entity) and fail loud on the
rest (or it masks bugs):

The decisive distinction is **discovered child** vs **configured root**:

| 404 cause | Example | Disposition |
|---|---|---|
| Discovered child deleted (TOCTOU) | MR removed before its notes are fetched | skip child, advance |
| Discovered child feature disabled | repository off on a discovered project → `/repository/branches` | skip child, advance |
| Discovered child auth-mask (404, not 403, to hide existence) | non-admin token, private discovered resource | skip child, advance |
| **Configured root** absent/typo/revoked | `gitlab_groups`/`gitlab_projects` entry that 404s | **fail loud** (preflighted in `check`) |
| Our bug — wrong path / wrong id | typo → every entity 404s | fail loud / detectable |
| Null-parent routing | `/projects/None/...` | fail loud |
| 404 **after pagination started** (page 2+) | entity deleted mid-read | **fail loud** (no partial slice) |

Policy (structural, no config):

- **Substreams skip `404` only on the first page of a slice**
  (`GitlabSubstream.skippable_statuses = {404}`, inherited; the base predicate
  also requires `next_page_token is None`). A discovered child that is gone
  before any data is read is skipped; a `404` *after* pagination has begun
  raises `GitlabApiError` rather than silently keeping a truncated slice — it
  self-heals on the next sync when the entity is fully gone.
- **Configured roots raise on `404`.** `ScopedGitlabStream` (the `projects` and
  `users` streams) does **not** skip — a configured group/project that 404s is
  explicit coverage loss the operator must see. Configured groups *and* projects
  are validated in `check_connection` (preflight), so a bad root fails at setup,
  not silently mid-sync. The whole-instance list endpoints (`/projects`,
  `/users`) also raise — they never 404 on success.
- **Null-parent-routing guard**: a substream raises `GitlabApiError` if the
  parent routing id is null/missing *before* building the path, so the
  `/None/...` bug fails loud instead of masquerading as a skippable 404.
- **Every skip is logged** with stream + url.
- `400` / `409` / `422` / `5xx`-after-retry always raise.

EE-only or license-gated endpoints (notably `merge_request_approvals`) extend
their own skip set (e.g. `{402, 403, 404}`) so CE and EE both work unchanged;
generic substreams keep raising on `403` (usually a token-scope problem worth
surfacing).

Residual: a discovered-child stream where the token can see *none* of the
resource (every first-page request 404s) collects zero rows. The bug shapes
that could cause this all raise (routing, mid-pagination, configured-root); the
remaining case is a genuine permission boundary, surfaced by per-table bronze
freshness monitoring (a stale `_airbyte_extracted_at` for that stream's table),
which is per-stream, not per-connection. An in-connector all-404 abort is not
added because the CDK's per-slice `read_records` has no stream-completion hook
to distinguish it from a single benign skip without a heuristic threshold.

### 5.5 Diff safety
`commit_file_changes` parses hunks with a per-diff line budget; diffs GitLab
marks `too_large`/`collapsed` are recorded as a row with null counts +
`diff_truncated=true` rather than fetched in expanded form.

### 5.6 Rate limits & retry
Rate-limit handling is first-class and universal. The connector makes **no
assumption** about any instance's limits — admins configure them freely (none,
strict per-user/IP, per-endpoint, search-specific, raw-blob, …) and the same
artifact must run correctly everywhere. It complies with whatever the instance
reports:

- On `429`: wait per `Retry-After` (seconds or HTTP-date). If absent, derive
  the wait from `RateLimit-Reset` / `RateLimit-ResetTime`.
- Proactive throttle: when `RateLimit-Remaining` approaches zero, slow down
  before being refused rather than spinning into 429s.
- Exponential backoff with jitter on `429` and `5xx`; bounded retry count;
  a page is never silently dropped — exhaustion raises.
- Auth failures (401, non-rate-limit 403) raise loudly rather than being
  swallowed as empty pages.

Verified header semantics: `RateLimit-Limit`, `RateLimit-Name`,
`RateLimit-Observed`, `RateLimit-Remaining`, `RateLimit-Reset` are returned on
**all** responses — `RateLimit-Reset` is **Unix epoch seconds**, not a relative
value. On `429`, `RateLimit-ResetTime` (RFC2616 HTTP-date) and `Retry-After`
(seconds) are added and the body is **plain text** (`Retry later`), never JSON.

Critically, application-level limits (distinct from the header-reported
`Rack::Attack` throttles) also return `429` but expose **no** `RateLimit-*`
headers — so a `429` must be handled even when `Remaining` looked healthy, and
the handler never assumes a 429 is explained by the headers. Wait precedence:
`Retry-After` → `RateLimit-ResetTime` / `RateLimit-Reset` → exponential backoff
with jitter. Under concurrency the limiter is respected per request, so a 429 on
one worker backs that request off without failing the stream.

### 5.7 Concurrency posture
A shared instance can be degraded by aggressive crawling whether or not it
enforces limits, so politeness is the connector's responsibility in addition to
honoring whatever limits the instance reports (§5.6). Therefore:

- Streams are written concurrency-safe from the start (no shared mutable state,
  clean `stream_slices`/cursor contracts), but run **sequentially until each is
  correct**. ConcurrentSource is enabled only after sequential correctness.
- Concurrency is **bounded (~4–8), not maximised** — it is the politeness
  throttle against the shared instance, not a throughput dial.
- Concurrency must be **≥4**: the CDK self-deadlocks at `default_concurrency=1`
  once a sync exceeds ~10k partitions.
- The primary throughput lever is **request count** (diff granularity), not
  thread count. Concurrency multiplies a reduced N.

## 6. Downstream Mapping (informative)

Bronze is source-native. Staging dbt models map each stream to the shared
silver contract via `silver:<class>` tags.

| Stream | Silver target tag |
|---|---|
| `projects` | `silver:class_git_repositories` |
| `branches` | `silver:class_git_repository_branches` |
| `commits` | `silver:class_git_commits` |
| `commit_file_changes` | `silver:class_git_file_changes` |
| `merge_requests` | `silver:class_git_pull_requests` |
| `merge_request_commits` | `silver:class_git_pull_requests_commits` |
| `merge_request_notes` | `silver:class_git_pull_requests_comments` |
| `merge_request_approvals` + distinct note authors | `silver:class_git_pull_requests_reviewers` |

GitLab's review participation maps to the unified reviewer contract as:
approvers → reviewer events with `approved`; distinct non-system note authors →
reviewer events with `commented`.

**Proposed silver centerpiece (silver/gold rebuild):**
`class_git_change_request_event` — one row per lifecycle event
(`opened`, `commit_pushed`, `comment`, `approved`, `merged`, `closed`),
assembled from the MR record + `merge_request_state_events` +
`merge_request_notes` + `merge_request_approvals` + `merge_request_commits`.
Every cycle-time metric becomes a window function over this log; no phase
timestamps are hardcoded in the connector.

The squash/line-stats problem dissolves without per-original-commit stat
fetches: landed per-file churn comes from `commit_file_changes` (a squash
commit's own diff is the MR's net change); MR size from `changes_count` + its
landed commit stats; person volume from `commits` stats. Squashed-away
per-original-commit line counts are intentionally not collected — no objective
in §2 requires them.

## 7. To Verify Empirically (blocks finalization)

1. **`all=true` ref scope** — confirm it traverses only real branches+tags, not
   keep-around / hidden MR-diff refs (force-push phantoms would double-count
   volume). Empirical check: force-push an MR branch on a scratch project, then
   compare commit counts from `?all=true` against the union of per-branch
   `ref_name` traversals; phantom SHAs present only in `all=true` confirm
   keep-around inclusion. If dirty → fallback to per-branch traversal (enumerate
   via the `branches` stream, fetch commits per `ref_name` with the overlap
   cursor), or default-branch-only + `merge_request_commits`. Decide before
   writing the stream.
2. **`all=true` + `since` + `with_stats` composition** — incremental + free
   inline stats must hold together.
3. **Max-allowed-offset on the target instance** (default 50,000) — confirm the
   value and that time-windowing keeps `commits`/`merge_requests` queries under
   it on the largest repos.
4. **Approvals shape on the target edition** (CE vs EE) — confirm degradation
   path.
5. **Commit incremental gap** — quantify missed-old-commit risk; size the
   full-refresh backfill cadence.

## 8. Decisions to Ratify (supersede legacy `../gitlab.md`)

1. **Drop `gitlab_files` + integer `file_id` lookup** — surrogate keys need
   global state a stateless extractor can't own; ClickHouse compresses repeated
   paths. → emit `new_path`/`old_path` inline on `commit_file_changes`.
2. **No connector-computed/enrichment fields** (`duration_seconds`,
   `ai_percentage`, `language_breakdown`, scancode) — transformation lives in
   dbt/enrichment per ADR-0002.
3. **All-branch commits via `all=true`**, not per-branch iteration — full
   coverage through GitLab's server-side-deduped single pass, with no in-memory
   dedup set.
4. **MR terminology, unprefixed stream names** in `bronze_gitlab` — source-native
   names (`merge_requests`); the namespace supplies the prefix, so table names
   carry no `gitlab_`.
5. **No full raw-payload `metadata` column** — explicit typed fields only;
   raw-blob storage doubles payload, and worse, re-introduces the untrimmed
   text the §5.3 caps just removed. Diverges from the CONNECTORS_REFERENCE
   `metadata` convention. Ratified.
6. **Add lifecycle event streams** (`*_state_events`, `discussions`) absent from
   the legacy spec — required for the event log.
7. **Explicit declared fields; no `additionalProperties` reliance** — the
   ClickHouse destination (v2.x, `enable_json=false`) persists only
   schema-declared fields; undeclared properties do not become bronze columns.
   Each stream therefore **projects** every record to exactly its declared field
   set, hoisting needed nested scalars to top-level (e.g.
   `namespace.full_path` → `namespace_full_path`) — field mapping per ADR-0002,
   not derivation. All-scalar output also sidesteps `enable_json=false`. Bonus:
   smaller in-flight records (memory) and a deterministic bronze contract.

## 9. Verified API Conventions (REST v4)

Confirmed against current GitLab docs. The connector codes to these, not to
assumptions.

- **Base / auth** — `{instance}/api/v4`; `PRIVATE-TOKEN: <pat>` header.
- **Encoding** — namespaced project paths and any path/branch/tag containing `/`
  are URL-encoded (`%2F`). A literal `+` in an ISO-8601 query value must be
  `%2B`; the connector emits UTC `Z` timestamps so `+` never arises.
- **`id` vs `iid`** — MRs and issues are addressed within a project by `iid`
  (per-project); `id` is instance-global. Both captured.
- **`null` booleans** — boolean fields may be `null` (GitLab treats as `false`);
  parsing must not assume `true`/`false`.
- **Status codes** — `401` unauthenticated, `403` forbidden, `404`
  not-found-or-unauthorized, `429` rate-limited, `5xx` server/overload. `401` and
  non-rate-limit `403` raise; `404` on an optional sub-resource is skipped+logged
  (§5.4); `429`/`5xx` retry with backoff (§5.6).
- **Pagination headers** — `Link` (`rel=prev/next/first/last`) on offset;
  `Link rel=next` + `X-NEXT-CURSOR`/`X-PREV-CURSOR` on keyset. `x-total`/
  `x-total-pages` are omitted above 10,000 results — never relied upon.
- **Commit list** — `?all=true` (all refs), `with_stats=true` (inline
  additions/deletions/total), `since`/`until` (ISO-8601). Per-file detail only
  via `/repository/commits/:sha/diff` — no per-file counts, parsed from hunks.
- **MR list** — `updated_after`/`updated_before`, `order_by=updated_at`,
  `sort=asc`; MR carries `sha`/`merge_commit_sha`/`squash_commit_sha`, `squash`,
  `changes_count`, `user_notes_count`.
- **Members** — `/groups/:id/members/all` returns inherited members and is
  rate-limited (200/min, 18.6+); enumerate at group level, not per-project, to
  minimize calls.
