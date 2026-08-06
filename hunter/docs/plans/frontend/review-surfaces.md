# Live review surfaces — Console History, Portfolio scoreboard, Home digest

Reference for the live app's **review-first** half (shipped 2026-08-06). The cockpit
answers *what needs doing now*; these three surfaces answer *what happened, and is any
rule decaying*. Structure/nav lives in [`docs/arch/frontend.md`](../../arch/frontend.md);
this file holds the contracts and the decisions behind them.

## The governing principle: one cohort, many views

A **cohort** is a set of positions: date range · rule · mode · status · exit reason. On the
Console it lives in the URL (the `h*` keys of `OPS_PARAMS`, see `lib/strategy/nav.ts`) and
is read through one hook, `live/pages/console/historyCohort.ts`.

The charts deck and the table below it are driven by that same cohort. This is the whole
point: **a chart must never be computed from a different query than the rows under it**.
Concretely, that means no per-chart aggregate endpoints — the four charts fold one payload
(B2) client-side, and the table pages the matching population (B1) server-side.

Two consequences worth knowing:

- The cohort hook takes `nowMs` from the caller and freezes it per mount. A preset window
  whose `from` bound is recomputed each render produces a new request body on every
  keystroke elsewhere on the page.
- `mode: 'all'` (real + paper) has no series equivalent — the B2 endpoint takes one mode.
  The deck charts the real book while the table pages both. That asymmetry is deliberate
  (mixing paper money into an equity curve would misreport), and the filter bar says which
  mode is charted.

## Backend contracts

### B1 — `POST /api/portfolio/positions/query`

One page of positions across **all rules and runs**, under the same `TableRequest` wire
contract as `holdings/query` and the per-rule `rules/{id}/positions`. `X-Total-Count`
carries the filtered total.

It is **not a second SQL path**. `find_positions_paged` / `count_positions` took a required
`(scope_col, scope_id)`; they now take `Option<(&str, Uuid)>` and push `TRUE` when it is
`None`. Everything else — the `tokens`/`tokens_info` LEFT JOINs, the whitelist resolvers,
the ordering and its `sp.id` tiebreak — is shared verbatim with the per-rule read. If you
change one, you change both, which is the property worth preserving.

Cohort narrowing rides the ordinary filter machinery, so three whitelist entries were added
to `position_filter_sql`:

| Key | SQL | Note |
| --- | --- | --- |
| `mode` | `sp.mode` | `real` / `paper` |
| `rule_id` | `sp.rule_id::text` | cast so `Eq`/`In` use the text ops |
| `entry_sol` | `sp.entry_lamports * 0.000000001` | **multiply**, never `/ 1e9` (the divide is truncated in the exact SQL path) |

`entry_sol` also became a wire field on `PositionResponse`, and the shared `positionColumns`
entry leg now prefers it over the `price × tokens` reconstruction — so what the Entry Size
column *shows* is what the server *sorts and filters on*.

The time window is not a filter but `TableRequest.range`, lowered onto
`POSITION_WHEN_SQL` = `COALESCE(sp.exit_time, sp.entry_time, sp.created_at)`: a closed row
filters by its close, an open or never-filled row by when it appeared. `from` is inclusive
and `to` exclusive, so adjacent windows can't double-count a close on the boundary
(locked by `range_bounds_are_half_open_over_the_when_instant`).

### B2 — `GET /api/portfolio/closes-series?range=&mode=&rule_id=`

A compact per-close array — `{exit_time, rule_id, pnl_sol, entry_sol, win}` per `End` row —
plus an `entry_failed` count. Not pre-bucketed: one fetch feeds the equity curve, the
histogram, the calendar, the day×hour heatmap, and the per-rule comparison. Per-chart
endpoints would have been the obvious alternative and are exactly how aggregation drift
starts.

`pnl_sol` uses `models::strategy::realized_exit_sol` — the ONE decider of which exit figure
counts (`exit_sol_total` once any sell leg landed, else the stamped single-leg `exit_sol`),
shared with `StrategyPosition::realized_pnl_sol` so the series and the table agree on a
scale-out position.

`EntryFailed` rows are counted separately, never folded into the series: no SOL was
deployed, so they have no PnL and no entry basis. They are still worth surfacing — a rule
that stopped losing money because it stopped *filling* is not a rule that got better.

Volume is closes, not trades, so a bounded window scan is fine. Note there is no index on
`(mode, status, exit_time)`; if an all-time series ever gets slow, that is the fix.

### B3 — armed history: deleted, not built

`ArmedHistoryPanel` called `/api/strategies/{s}/rules/{id}/armed-history`, which **never
existed** in the Rust server — the panel 404'd on every render. Arms that never fire are
held in the in-memory runtime cache and dropped, never persisted, so there was no data for
a route to serve. The panel and its RTK endpoint are gone. Reviving the feature means
designing durable arm storage first; adding a route would only move the emptiness.

## Decay — what "▼" means

`groupTrends(points, labelOf, window = 20)` compares a rule's last `window` closes against
the `window` before them, and flags `decaying` only when **both** win rate and expectancy
(mean SOL/trade) fell.

Requiring both is the load-bearing choice. Win rate alone flips on a single tail trade —
and the wallet-analysis work found repeatedly that hit rate is the *wrong* ranking signal
(`docs/plans/strategies/wallet-analysis.md`); expectancy alone flips on one outlier.

A rule with fewer than `2 × window` closes is reported with `decaying: false`, never
omitted: a rule silently vanishing from a decay board reads as "healthy". The UI shows `—`
with a tooltip saying how many closes it needs.

The same window and verdict are used in three places — the Portfolio `Δ Win%` column, the
Console rule-comparison card, and the Home rule alerts — from the one `groupTrends` fold.

## Why the Recent-closed lane is gone

It was a 50-row ring: hydrated from `GET /api/portfolio/recent-closes` at boot, then
prepended to by SSE. It could only ever answer "what closed while this tab was open", and
its rows carried no exit fill (the terminal SSE frame doesn't include one), so PnL showed
blank until a refetch.

History replaces it with the DB population, and the slice no longer keeps `recentClosed` at
all — a terminal frame just deletes the row from `open`. That also removed a per-close array
rebuild on the SSE path and one boot fetch.

The consequence to keep in mind: **the closed-position detail modal belongs to
`HistoryTable`**, not `ConsolePage`. History holds the full DB record, so a position from
any date opens; the Console page only ever had rows still in the session's live lane.

## File map

| Concern | File |
| --- | --- |
| Cohort state (URL-backed) | `live/pages/console/historyCohort.ts` |
| Section composition + SSE refetch | `live/components/history/ConsoleHistorySection.tsx` |
| Filter bar | `live/components/history/HistoryFilterBar.tsx` |
| Charts deck | `live/components/history/HistoryChartsDeck.tsx` |
| Server-paged table + detail modal | `live/components/history/HistoryTable.tsx` |
| Shared folds + renderers | `shared/components/analytics/` |
| Home digest + rule alerts | `live/components/home/ReviewDigest.tsx` |
| B1 handler | `core/src/api/handlers/strategies/rule_positions.rs::portfolio_positions_page` |
| B1 repo | `core/src/storage/repositories/strategy_repo.rs::{find_positions_all_paged,count_positions_all}` |
| B2 repo | `core/src/storage/repositories/strategy_repo.rs::{closes_series,entry_failed_count}` |
| B2 service | `live/src/services/portfolio.rs::closes_series` (+ `range_since`, the ONE range grammar) |
