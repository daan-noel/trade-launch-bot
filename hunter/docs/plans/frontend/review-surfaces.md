# Live review surfaces — Console History, Portfolio scoreboard, Home digest

Reference for the live app's **review-first** half (shipped 2026-08-06). The cockpit
answers *what needs doing now*; these three surfaces answer *what happened, and is any
rule decaying*. Structure/nav lives in [`docs/arch/frontend.md`](../../arch/frontend.md);
this file holds the contracts and the decisions behind them.

## The governing principle: one cohort, many views

A **cohort** is a set of positions: date range · rule · mode · status · exit reason. On the
Console it lives in the URL (the `h*` keys of `OPS_PARAMS`, see `lib/strategy/nav.ts`) and
is read through one hook, `live/pages/console/historyCohort.ts`.

**Exit-reason filter needles** (`historyExitFilter.ts`) match what live persists: system
labels (`TakeProfit`, `StopLoss`, `Dead`, `Manual`, `Migrated`) plus **metric names**
(`stall`, `trail`, `pnl`, …) as `contains` substrings — so `stall` matches `stall >= 300`.
Do not use the retired ladder aliases (`Trailing`, bare `Stall`, `TimeStop`); legacy URL
values are canonicalized onto the metric names.

The charts deck and the table below it are driven by that same cohort. This is the whole
point: **a chart must never be computed from a different query than the rows under it**.
Concretely, that means no per-chart aggregate endpoints — the four charts fold one payload
(B2) client-side, and the table pages the matching population (B1) server-side.

### Chart focus (drill-down lens)

Clicking a calendar day or week, heat cell, distribution bar, rule-comparison row,
hold point/band, or a Metric± exit tile sets `hfocus` (see
`live/pages/console/historyFocus.ts`). That is a **shared lens** on top of the parent
cohort — same predicate for the table and for the charts that refold
(`filterClosesForFocus`):

| Wire | Meaning | Table + lens charts apply |
| --- | --- | --- |
| `day:YYYY-MM-DD` | civil day in the UI timezone | `range` intersected with that day's UTC `[from,to)` |
| `week:YYYY-MM-DD` | calendar week, keyed by its **Sunday** | same, over a 7-day span |
| `heat:<dow>:<hour>` | recurring weekday×hour | client filter on `exit_time` (scan capped at 1000) |
| `pct:<lo>:<hi>` | histogram bucket (adjacent pair from any `PNL_DIST_EDGE_SETS` density) | `pnl_pct` numeric filter |
| `rule:<uuid>` | one rule | `rule_id` eq (does **not** change the bar's rule select) |
| `pos:<uuid>` | one close (Hold vs PnL point) | `id` eq |
| `band:<holdLo>:<holdHi>:<pctLo>:<pctHi>` | Hold vs PnL drag-zoom | client filter on hold + PnL% |
| `exit:metric_win` / `metric_loss` / `metric` | legacy Metric± deep-link | client filter (new clicks use `hexit` instead) |

**Exit mix strip** (`HistoryExitSummary`) sits between the filter bar and the charts
deck. It folds the same B2 closes payload through `runSummaryFromRows` /
`runSummarySections`, then **refolds on chart `hfocus`** (day / heat / pct / …) so
the counts match the focused slice equity/table use. Exit-tile clicks write `hexit`
and leave `hfocus` alone — the two compose. Metric± use synthetic needles
(`metric_win` / `metric_loss` / `metric`) — client-only, not SQL `contains` — because
a substring cannot express the win/loss split. Legacy `hfocus=exit:…` still highlights.

**Hybrid rendering:** calendar + heatmap keep the **parent** cohort (selection ring on
the active cell) so the timing grid stays readable; equity curve, PnL distribution,
and rule comparison **refold on the focused slice**. Hold scatter refolds too, except
its own `holdBand` zoom — that keeps parent dots and zooms via `domain`, and
`contextPoints` keeps axes mounted when another lens has no scatter rows. A Focus chip
in the filter bar clears the lens. Clicking the same cell again toggles it off.
Equity-curve brush focus is intentionally out of scope.

`day:` and `week:` are the same derivation at two widths — both go through
`spanBoundsUtcIso(startDay, days, tz)`, so a week lens is exactly the union of its
seven day lenses and neither can round a DST edge differently (locked by
`weekBoundsUtcIso` starting where day 1 starts and ending where day 7 ends). The civil
date arithmetic underneath is the shared `pnlSeries::shiftDayKey`, which the calendar
grid also walks its columns with — a focus window cannot address a date the cell that
produced it wasn't showing.

### What the calendar encodes

Three facts share one square, chosen because each answers a question the others can't:

| Channel | Carries | Why |
| --- | --- | --- |
| Fill hue | sign of the day's PnL | the primary read |
| Fill alpha | \|PnL\| relative to the window max | magnitude |
| **Border alpha** | trade count (√-scaled vs the window max) | one lucky close and a forty-trade grind are otherwise identical squares — the same conflation the wallet work kept hitting |

The column axis is **months**, not weekdays: the day×hour heatmap sits directly beside
it and already owns "which weekday", so a M/W/F gutter spent width without ever telling
you which dates you were looking at. Rows still run Sun→Sat; weekend cells carry a
dashed outline, today a white ring.

The summary strip under the grid (`summarizeDailyPnl`) is the part colour cannot show:
green-day rate, the two extreme days, and the longest consecutive red run. Two decisions
in that fold are load-bearing — a **no-trade day is not a flat day** (it's excluded from
every count, since an absence isn't a loss), and a no-trade gap **does not break a red
streak**, because a quiet weekend is not a recovery. It summarizes only the window the
grid actually draws, so the strip and the squares can never disagree.

PnL distribution density (`sparse` / `default` / `dense`) is a **view preference**
(`localStorage` key `hunter.pnlDistDensity`), not a cohort key — see
`PNL_DIST_EDGE_SETS` in `pnlSeries.ts`. All presets share the open win tail
(`50…100` / `100…200` / `200…500` / `≥ 500`); they only change how finely the zone
around 0% is sliced. Changing density clears a `pct:` focus whose bucket is not on
the new grid.

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

A compact per-close array —
`{id, exit_time, rule_id, mint_address, pnl_sol, entry_sol, win, hold_secs, exit_reason}`
per `End` row — plus an `entry_failed` count. `hold_secs` is
`EXTRACT(EPOCH FROM exit_time − entry_time)` (null when `entry_time` is missing) and feeds
the Hold-vs-PnL scatter. `exit_reason` lets the charts deck apply the same exit-reason
`contains` cohort filter as the table (metric needles like `stall` match `stall >= 300`).
Not pre-bucketed: one fetch feeds the equity curve, the histogram, the calendar, the
day×hour heatmap, the hold scatter, and the per-rule comparison. The client folds that
payload once via `foldPnlDeck` (`shared/components/analytics/pnlSeries.ts`) — not six
independent walks. Per-chart endpoints would have been the obvious alternative and are
exactly how aggregation drift starts.

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

The same window and verdict are used in three places — the Portfolio **Form** column, the
Console rule-comparison card, and the Home rule alerts — from the one `groupTrends` fold.

## Portfolio keep/kill review board

`/portfolio` is a **calendar-window keep/kill board** (not a second History). Default
range is `7d` (aligned with History/Home digest). Numbers are calendar-window closes —
not Rules Control current-run / all-time scores.

| Control | Behavior |
| --- | --- |
| Window strip | Portfolio spark + realized ◎ + closed/rules counts; entry-failed hint; link to all-trades History |
| Rule alerts | Named decaying rules (same `groupTrends` verdict as Home) → Rules Evidence; "Show only decaying" toggles `?decay=1` |
| `RankedPnlBars` | Hero rank by realized PnL; click toggles `?rule=` highlight (synced with table) |
| Compact table | Rule · PnL · Exp (◎/trade) · Form · N · History — row click highlights only |
| Rule name | → Rules (keep/kill); History link → Console History |
| Form column | Δ win pp + Δ expectancy ◎; ▼ only when `decaying` |

Trade browsing and the charts deck remain on Console History. Pause/Activate stays on Rules.

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
| Exit-reason filter needles + series trim | `live/pages/console/historyExitFilter.ts` |
| Exit-mix fold + tile → cohort mapping | `live/pages/console/historyExitSummary.ts` |
| Chart focus parse/serialize + TZ bounds | `live/pages/console/historyFocus.ts` |
| Section composition + SSE refetch | `live/components/history/ConsoleHistorySection.tsx` |
| Filter bar | `live/components/history/HistoryFilterBar.tsx` |
| Exit mix + counts strip | `live/components/history/HistoryExitSummary.tsx` |
| Charts deck | `live/components/history/HistoryChartsDeck.tsx` |
| Server-paged table + detail modal | `live/components/history/HistoryTable.tsx` |
| Shared folds + renderers | `shared/components/analytics/` |
| Home digest + rule alerts | `live/components/home/ReviewDigest.tsx` |
| B1 handler | `core/src/api/handlers/strategies/rule_positions.rs::portfolio_positions_page` |
| B1 repo | `core/src/storage/repositories/strategy_repo.rs::{find_positions_all_paged,count_positions_all}` |
| B2 repo | `core/src/storage/repositories/strategy_repo.rs::{closes_series,entry_failed_count}` |
| B2 service | `live/src/services/portfolio.rs::closes_series` (+ `range_since`, the ONE range grammar) |
