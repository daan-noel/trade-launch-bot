# The arm ledger — `strategy_arms`

An arming episode is a decision the bot makes and then throws away. `ArmedRegistry`
holds it in RAM while it lives, the `strategy_armed_changed` SSE announces its end, and
nothing writes it down — so "which tokens did this rule look at and pass on last
Tuesday" has no answer, and a rule's selectivity is unmeasurable.

`strategy_arms` is the durable record: **one row per `(rule, mint)` arming episode**,
from the arm to whatever ended it. A position is what happens when an episode ends in
`entered`; every other ending is a token the rule watched and did not buy.

## The table

```
strategy_arms
  rule_id       UUID NOT NULL
  mint_address  TEXT NOT NULL          -- the one token-data key
  mode          TEXT NOT NULL          -- 'real' | 'paper', frozen at arm time
  armed_at      TIMESTAMPTZ NOT NULL   -- hypertable range column
  ended_at      TIMESTAMPTZ            -- NULL while the episode is live
  end_reason    TEXT                   -- NULL while live; see below
  position_id   UUID                   -- set only when end_reason = 'entered'
  end_detail    JSONB                  -- set only when end_reason = 'unsatisfiable'
  PRIMARY KEY (armed_at, rule_id, mint_address)
```

There is no surrogate `id`: an episode IS its `(rule, mint, armed_at)` triple, and a
hypertable rejects a unique index that omits the partition column anyway. `position_id`
carries **no FK** — this table has a retention policy and `strategy_positions` does not,
so neither may pin the other's rows alive.

`end_reason` is the `DisarmReason` vocabulary plus one synthetic member:

| `end_reason` | Meaning |
| --- | --- |
| `entered` | The rule bought — `position_id` points at the position |
| `dead` | Dead-token verdict (liquidity gone + silent) |
| `migrated` | Left the curve before entry |
| `unsatisfiable` | A monotonic entry bound was permanently crossed |
| `paused` | The rule went inactive under it |
| `duplicate_identity` | The copycat guard blocked the mint |

Both the enum and the string come from `disarm_reason_str` — the engine's vocabulary is
the SSOT, and `entered` is the sink's own member (the engine's Enter path emits no
`ArmedChanged`, see [strategies.md](../../arch/strategies.md)).

## `end_detail` — the one reason that does not explain itself

`unsatisfiable` names a mechanism, not a cause: it means a monotonic entry bound was
permanently crossed, and `time` is the only monotonic metric, so it always reads "the
token aged past the entry window". Which condition the entry was still *failing* when
the clock ran out — the thing a rule gets tuned on — is nowhere in the word.

`end_detail JSONB` (0003) carries it, set on `unsatisfiable` alone:

```json
{ "blocked_by": "m_flow_window.gross_flow",
  "killed_by":  { "metric": "m_state.time", "threshold": 50.0, "operator": "<" },
  "unmet": [{ "metric": "m_flow_window.gross_flow", "window_size_sec": 60.0,
              "value": 24.71, "conditions": [{ "operator": ">=", "value": 40.0 }] }] }
```

**The fold captures it, at the instant it gives up.** `decide_arm` holds the
`TokenTrack` there and `CompiledRule::entry_blockers` walks the entry reqs once —
without short-circuiting, because the answer is the whole failing set. It rides
`ArmedDelta::detail` (boxed: every other transition carries `None`) to the sink, which
renders the wire shape in `entry_blockers_json`. Cold path by construction: an arm
disarms once, and `entry_satisfied` remains the hot-path test.

Capturing beats reconstructing, and the modal's replay strip is the proof — it answers
the same question from stored trades, but only while the trade-retention window holds
the episode, and the armed path takes the rule **live** (no `params_snapshot`), so a
rule edited afterwards redraws thresholds that never applied.

Three rules the shape encodes:

- **The mono-killed req is excluded from `unmet`.** At the disarm instant it is failing
  by construction; listing it beside the real blockers makes one problem read as two.
- **An empty `unmet` is an answer**, not a gap: every other condition held and the token
  qualified a moment too late. `blocked_by` is then `null`, and the column reads
  `clock only` rather than borrowing the deadline's name.
- **`blocked_by` is a representative, not a ranking** — the first unmet req in the
  rule's own compiled order. Entry is an AND, so every member of `unmet` was equally
  binding; the scalar exists so the column can filter, sort and group without the
  client parsing the document, and `unmet` ships in full beside it.

Metric names are the registry **paths** (`group.metric`), never the `MetricId` ordinal:
this is a stored row that outlives the build that wrote it, and an ordinal would
silently re-point every historical row the day a metric is inserted.

`arm_blocked_by` groups the cohort on `end_detail ->> 'blocked_by'` — one expression,
shared with the sort and filter whitelists, so a breakdown bar and the rows its lens
produces can never describe different populations. It is deliberately **not**
`COALESCE`d: "no blocker recorded" is absence, and naming it would file every `dead`
episode under one invented bucket.

`armed_at` carries the hypertable range and every read's time window. A row is written
at the arm and updated at the end, so a live episode is a `NULL ended_at` — the ledger
survives a restart, which a write-only-at-the-end design cannot.

**Volume is unbounded by design** — an arm costs nothing on chain, so a loose
fingerprint arms on most launches. The table is a hypertable with compression and
retention on the same footing as `trades`; treat it as a rolling buffer, never as
permanent history.

## Writes never touch the hot path

`EffectSink::on_armed_changed` is synchronous and sits inside the decision fold's
effect drain. It hands the episode to a **bounded** `mpsc` with `try_send` and returns;
the `arm_ledger` writer task drains, batches a flush window, and issues one multi-row
`INSERT` and one `UPDATE … FROM (VALUES …)`. Bounded and non-blocking together are the
point: a wedged writer must never become backpressure on trade decisions. A full queue
drops the write **loudly** — a silent drop is an invisible hole in the ledger, and reads
exactly like "the rule never armed on that token" (see
[backpressure-watchdog.md](../ingest/backpressure-watchdog.md)).

A flush issues its arms **before** its ends: an episode can arm and end inside one
window (a token that dies on its creation slot), and the UPDATE keys on a row the
INSERT is about to write. The end write is `WHERE ended_at IS NULL`, so it is idempotent
and keeps the FIRST ending when two reach the same episode.

`ArmedRegistry` carries `armed_at` so the end write can key its episode without a read:
the key is `(rule_id, mint_address, armed_at)`, and the writer resolves an end whose
insert has not flushed yet by coalescing both sides inside one flush.

## Live vs. durable — two readers, one fact

| Question | Read |
| --- | --- |
| What is armed **right now** | `ArmedRegistry` via `GET /api/strategies/armed` + the `strategy_armed_changed` SSE — the Console **Waiting** lane |
| What was armed **over a window** | `strategy_arms` via `POST /api/strategies/arms/query` — the Console **Arms** section |

They overlap on live episodes on purpose. Waiting is a cockpit lane that must patch
per-event with no round trip; Arms is a review surface with a date range. Folding
either into the other trades a latency budget for a history feature or vice versa.

## Read API

`POST /api/strategies/arms/query` takes the unified `TableRequest` (paging, sorting,
search, per-column filters, `range`) and returns a bare array plus `X-Total-Count`,
exactly like `/api/portfolio/positions/query`. The whitelists live in `arm_repo`:

- **sort**: `mint_address`, `symbol`, `rule_id`, `mode`, `armed_at`, `ended_at`,
  `end_reason`, `blocked_by`, `waited_sec`
- **filter**: the same set plus `position_id`

`range` applies to `armed_at`, not `ended_at`: the question is "what did the bot look at
during this window", and keying on the end would drop every episode still waiting. The
`end_reason` filter column is `COALESCE(end_reason, 'waiting')`, so a live episode is
filterable — the same trap `exit_reason` has on the positions whitelist. There is no
token-enrichment fallthrough: the table appends no enrichment columns, so the JOIN
carries `symbol` for the search and nothing else.

`waited_sec` = `EXTRACT(EPOCH FROM (COALESCE(ended_at, now()) - armed_at))`, defined
once and shared by the sort and filter whitelists so the column can't sort by one fact
and filter by another.

`POST /api/strategies/arms/summary` returns the funnel over the same cohort —
`armed`, `entered`, and a count per `end_reason` — aggregated in Postgres, no rows
shipped. It carries `blocked_by` beside it: the `unsatisfiable` count broken down by
the condition that held each episode out. That is a second statement (it groups by a
per-row value, which a fixed-shape aggregate cannot carry), issued concurrently on the
same pool under the same JOIN and WHERE, so it counts exactly the population the funnel
describes. The funnel is `#[serde(flatten)]`ed into `ArmSummary`, so the addition is a
key, not a re-nesting.

## Console surfaces

Lanes top to bottom, the last three collapsible with their open state in `useUiToggle`:
Attention · Open (+ manual trade) · Waiting · History · **Arms**.

- **Waiting**, **Open** and **Arms** ride `TokenTable`, not `DataTable`, for the
  toolbar's **Charts** toggle. Open draws the row's entry marker through the one
  `InspectTarget` adapter. Waiting and Arms draw **no markers**: a chart marker needs a
  price and an arming episode has none, so marking `armed_at` would put a dashed line at
  a price the token never traded at. Their episode facts (armed at, waited, outcome)
  ride the card header instead.
- Every row on those tables keys its mint as `mint_address` — `TokenTable`'s accessor is
  fixed to the one token-data key, and the live rows are on a token-data path.
- **A collapsed section fetches nothing.** History mounts a summary aggregate and a
  cohort walk of up to 20 000 rows; Arms mounts a page plus a funnel. The body is
  unmounted while collapsed, and a deep link that targets a collapsed section
  (`scroll=history`, any `h*` cohort param, any `a*` arms param) forces it open before
  scrolling.
- Arms owns its own cohort params (`a`-prefixed) and its own date range. It does not
  share History's, so narrowing a PnL review does not silently narrow the arm funnel.
- The table's **Blocked by** column and the strip's breakdown read `end_detail`; the
  column filters through the `blockedBy` cohort key (`ablocked`), its own query-string
  channel because it narrows *within* `unsatisfiable` and the two compose.
- **Clicking an episode opens `ArmDetailModal`** (←/→ walks the page, as on History).
  The table owns it for the reason History owns the closed-position modal: the row it
  opens lives on the current page, not in the live registry the lanes above read.
  Selection stays section-local — an episode has no position id, so it cannot ride the
  page's `position` param.
- That modal rides `FloorPositionDetail`'s `header` slot. Chart, crosshair ↔
  condition-band wiring and the bar-trades panel are the shared part; the hero and the
  money strip are not, because an episode has no fill — the default header renders a
  `—%` over a row of dashes. The header leads with `end_detail`'s one-line verdict — what the fold recorded, above
  the strip that reconstructs it — then states outcome, armed/ended and waited,
  and an `entered` row's `position_id` is **shown, not linked** (the closed-position
  modal only opens a row on History's current page).
- The modal's condition strip pins at **`ended_at`** for a finished episode —
  `ArmedRuleConditions` takes `endedAt`, skips the live readout and reconstructs from
  the series instead. A disarmed pair has no engine state, so the live pin is a
  permanent 404 that still costs the decision loop a round trip per second.
