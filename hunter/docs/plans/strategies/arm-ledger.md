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
  `end_reason`, `waited_sec`
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
shipped.

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
