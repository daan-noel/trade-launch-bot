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

The summary strip, the exit mix, the charts deck, and the table are all driven by that same
cohort. This is the whole point: **a chart must never be computed from a different query
than the rows under it**. Concretely, that means no per-chart aggregate endpoints — every
chart folds one payload client-side, and the table pages the matching population (B1)
server-side.

### Two chart reads: the cohort, and the row

- **The deck** (`HistoryChartsDeck`) is the **per-cohort** read — equity curve, PnL
  distribution, hold scatter, rule comparison, calendar, hour heatmap.
- **The table's toolbar Charts toggle** is the **per-position** read — one token price
  chart per row on the current page, with that position's own fill markers and a PnL/hold
  card header. `HistoryTable` rides `TokenTable` rather than the raw `DataTable` for
  exactly this: the toggle and its lazy grid already live there.

The grid starts closed (no `chartsDefaultOn`) — a card is a per-row trade fetch, and the
deck is this page's primary read. `existingKeys` is `ALL_TOKEN_INFO_KEYS`, appending
**nothing**: an appended token column would offer a sort/filter key B1's whitelist rejects.

### The cohort includes the table's own filters

Three inputs compose into **one** request body (`console/historyRequest.ts`): the URL
cohort, the chart focus lens, and the table's search + per-column filters. That body is
then read three ways — paged for the table (B1), aggregated server-side for the strip (B4),
walked in full for the charts. The table and the strip differ *only* in pagination and
sort, which is exactly what an aggregate is allowed to ignore, and `historyRequest.test.ts`
locks that.

The chart walk is the deliberate exception: it passes `includeFocus: false` and fetches the
**parent** cohort, because the deck lenses itself and does so asymmetrically (see "Hybrid
rendering" below — the timing grids stay on the parent and draw a selection ring). Hand the
deck a pre-focused cohort and clicking a day empties the calendar that produced the click,
with nothing failing to say so. A useful side effect: clicking through chart cells
re-fetches a single aggregate row rather than re-walking the cohort.

This was not true originally: the cohort narrowed the table only, so typing `>0.1` into
Entry ◎ moved the rows while the charts above them kept describing the unfiltered book. A
summary that answers a different question than the rows under it is worse than no summary —
it looks authoritative while being wrong. The builder makes the divergence unrepresentable
rather than merely fixed.

The deck also stopped folding B2 for the same reason: that endpoint is closed-only,
single-mode, and cannot see a table filter, so it was a second population by construction.
It still exists — Home's review digest is its consumer — but Console History now derives
its close points from the same positions query the table pages
(`console/historyPositions.ts`).

### Two summary sources, one shape

The strip normally renders the **server aggregate** (B4): exact over the whole filtered
cohort, past the page and past the chart-walk cap, with no rows shipped. Under a lens SQL
can't express — heat, hold band, legacy exit focus, a synthetic Metric± needle — the
aggregate would count rows the surfaces have lensed away, so the section folds the focused
slice client-side instead and that wins. Both paths meet at `RunSummary`, so only a
provenance line and the Migrated tile differ.

The client fold is **closes-only**, which is a property of those lenses rather than a
shortcut: heat keys on `exit_time`, hold band on hold + PnL%, exit focus and Metric± on
`exit_reason` — an open position matches none of them, so it belongs in no such slice. The
Migrated tile is omitted there (close points carry no migration flag, and a `0` would read
as "none migrated" rather than "not measured on this path" — `runSummarySections` hides a
tile whose count is `undefined`).

Exit tiles stay in their own panel (`HistoryExitSummary`) rather than the strip: the strip
renders the wire counters, which collapse metric exits to Metric±, while the panel folds
per-position reasons and can keep the stored detail (`stall > 300`). Rendering both would
be two different answers to the same question, so the strip drops its `Exits` band.

### Tiles are lenses

| Tile | Channel | Filter |
| --- | --- | --- |
| Fired | `hlane=fired` | `entry_price > 0` — the aggregate's "entered" predicate |
| Closed | `hlane=closed` | `entry_price > 0` + `status = End` |
| Open | `hlane=open` | `entry_price > 0` + `status ≠ End` |
| Win% / Worst% | `houtcome=win\|loss` | `pnl_sol > 0` / `≤ 0` — realized SOL, matching the server's `is_win` |
| Migrated | `hmigrated=1\|0` | `is_migrated` eq |

`hlane` is its own channel rather than a value of `hstatus` because **Open spans several DB
statuses** (Holding, ExitPending, ExitStuck, ExitUnconfirmed) — no single status string
means it, and `entry_price > 0` is what separates an entered position from an `EntryFailed`
row. The two channels are mutually exclusive on write: letting both stand would intersect
to an empty cohort ("Open" ∩ "End"), which reads as *no trades* rather than as a
contradiction.

`hmigrated` is tri-state and serialized `1`/`0`, never blank-for-false: "not migrated" is a
real cohort, and folding it into "no filter" would silently drop it.

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

**Exit mix strip** (`HistoryExitSummary`) sits between the summary strip and the charts
deck. It takes the same parent close points the deck plots and folds them through
`runSummaryFromRows` / `runSummarySections`, **refolding on chart `hfocus`** (day / heat /
pct / …) so the counts match the focused slice equity/table use. Exit-tile clicks write `hexit`
and leave `hfocus` alone — the two compose. Metric± use synthetic needles
(`metric_win` / `metric_loss` / `metric`) — client-only, not SQL `contains` — because
a substring cannot express the win/loss split. Legacy `hfocus=exit:…` still highlights —
and is the **one** lens this panel deliberately leaves unapplied, so a selected tile
highlights inside the full mix instead of collapsing the chart to the single slice it just
selected (which would destroy the comparison that made the tile worth clicking).

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
- `mode: 'all'` (real + paper) has no series equivalent — B2 takes one mode, so a
  series-backed deck charts the real book while the table pages both. Because every
  surface reads the positions query, `all` charts what it says: the mode is an ordinary filter, absent
  when `all`. Mixing paper money into an equity curve is still a real hazard — the mode
  chip in the filter bar is what keeps it honest.
- The charts walk is capped at 20 000 rows (`CHART_SCAN_MAX`). The strip is unaffected
  (it aggregates in Postgres), so a larger cohort gets exact totals over partial charts —
  and says so, rather than drawing a curve that silently stops.

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

### B4 — `POST /api/portfolio/positions/summary`

The aggregate twin of B1: same `TableRequest` body, same `PositionQuery`, no run-scope
semantics — so the summary strip totals exactly the population the table pages, past the
page size and without shipping rows. Backs the strip's server path.

Like B1, it is **not a second SQL path**. `positions_summary` took a required
`(scope_col, scope_id)`; it now takes `Option<(&str, Uuid)>` and pushes `TRUE` when
`None`, the same shape `find_positions_paged` already had — so the aggregate and the page
cannot scope differently. `positions_summary_all` is the cross-rule entry point.

Open positions are marked to market from the live in-memory token cache (`price_of`), as
the per-rule summary does — no DB or RPC round-trip on the read.

One whitelist entry was added for the Migrated tile:

| Key | SQL | Note |
| --- | --- | --- |
| `is_migrated` / `migrated` | `COALESCE(i.is_migrated, false)::text` | `Text` so the `Eq` (ILIKE) arm matches `'true'`/`'false'` |

The `COALESCE` is load-bearing: the read LEFT-JOINs `tokens_info`, and `NULL ILIKE 'false'`
is NULL, so an un-enriched token would vanish from a "not migrated" cohort instead of
counting as *not known to have migrated*.

This entry also fixed a **silent** pre-existing bug on Evidence / Simulate / Sweep:
`buildFocusTableFilters` has always emitted an `is_migrated` spec for its Migrated lens,
but the boolean columns were sort-only, so the spec hit the filter builder's "unknown key →
ignored" contract. The lens narrowed the charts (a client-side fold) and never the
server-paged table under them — the same failure shape as the sentinel bugs in
`hunter/CLAUDE.md`: a filter that looks applied and isn't. Locked by
`migrated_is_filterable_by_both_spellings` + `migrated_filter_survives_a_missing_tokens_info_row`.

### B3 — armed history: deleted, not built

`ArmedHistoryPanel` called `/api/strategies/{s}/rules/{id}/armed-history`, which **never
existed** in the Rust server — the panel 404'd on every render. Arms that never fire are
held in the in-memory runtime cache and dropped, never persisted, so there was no data for
a route to serve. The panel and its RTK endpoint are gone. Reviving the feature means
designing durable arm storage first; adding a route would only move the emptiness.

## Decay — what "▼" means

`foldPnlDeck(..., { labelOf, window = 20 })` emits a `trends` entry per group: it compares a
rule's last `window` closes against the `window` before them, and flags `decaying` only when
**both** win rate and expectancy (mean SOL/trade) fell.

Requiring both is the load-bearing choice. Win rate alone flips on a single tail trade —
and the wallet-analysis work found repeatedly that hit rate is the *wrong* ranking signal
(`docs/plans/strategies/wallet-analysis.md`); expectancy alone flips on one outlier.

A rule with fewer than `2 × window` closes is reported with `decaying: false`, never
omitted: a rule silently vanishing from a decay board reads as "healthy". The UI shows `—`
with a tooltip saying how many closes it needs.

The same window and verdict are used in three places — the Portfolio **Form** column, the
Console rule-comparison card, and the Home rule alerts — from the one `foldPnlDeck` walk.

## Portfolio keep/kill review board

`/portfolio` is a **calendar-window keep/kill board** (not a second History). Default
range is `7d` (aligned with History/Home digest). Numbers are calendar-window closes —
not Rules Control current-run / all-time scores.

| Control | Behavior |
| --- | --- |
| Window strip | Portfolio spark + realized ◎ + closed/rules counts; entry-failed hint; link to all-trades History |
| Rule alerts | Named decaying rules (same `foldPnlDeck` verdict as Home) → Rules Evidence; "Show only decaying" toggles `?decay=1` |
| `RankedPnlBars` | Hero rank by realized PnL; click toggles `?rule=` selection (synced with table) |
| Compact table | Rule · PnL · Return% · Exp (◎/trade) · Form · N · History — row click selects |
| Rule drill-down | `?rule=` opens `PortfolioRulePositions` below the table (see next section) |
| Rule name | → Rules (keep/kill); History link → Console History |
| Form column | Δ win pp + Δ expectancy ◎; ▼ only when `decaying` |

Cross-rule trade browsing and the charts **deck** remain on Console History. Pause/Activate
stays on Rules.

### The `?rule=` drill-down — `PortfolioRulePositions`

Selecting a rule opens its **closed** trades for the same window, directly under the
scoreboard: a server-paged `TokenTable` (B1) with the per-row Charts grid and the position
inspect modal on `?pos=`. It answers "which trades produced that number?" in place, where
the board's own atom is a rule.

**It must reconcile with the row it drilled from**, so it is scoped to exactly the
population `rule_period_pnl` aggregates — entered and `status = 'End'`, in the same window.
The row count is the row's **N**; the PnL ◎ column sums to the row's **PnL**. A drill-down
that quietly showed a different population (open bags, entry-failed buys) would look
authoritative while answering a different question — the same failure the one-request-body
rule exists to prevent above.

That scope is **not a second definition**. The panel builds a `HistoryCohort` with
`lane: 'closed'` and serializes it through `historyRequest`'s `historyTableBody` — the same
builder Console History's table, strip, and deck share — so both surfaces agree on what
"closed in this window" means, and the panel matches the row's own **History** deep link.

| Decision | Why |
| --- | --- |
| Columns are `historyColumns` minus `rule` + `status` | Both are constant under this cohort; a column that renders one value everywhere spends width to say nothing and offers a sort that can't reorder |
| Charts toggle starts **off** (persists per `tableId`) | A card is a per-token trade fetch; selecting a rule must not fire a page of them. Same choice as History, opposite of Rules Evidence, where the chart *is* the read |
| `nowMs` frozen per mount | A preset window whose `from` slides on every render refetches continuously; the panel unmounts on deselect, so it re-freezes on re-open |
| `?pos=` dropped when rule / window / mode changes | The id belongs to one population; a modal that silently fails to open reads as a bug |
| `resetKey` = cohort only (not the table's search/filters) | Changing the population snaps to page 1; a keystroke must not reset the table that produced it |

## Why the Recent-closed lane is gone

It was a 50-row ring: hydrated from `GET /api/portfolio/recent-closes` at boot, then
prepended to by SSE. It could only ever answer "what closed while this tab was open", and
its rows carried no exit fill (the terminal SSE frame doesn't include one), so PnL showed
blank until a refetch.

History uses the DB population instead, and the slice keeps no `recentClosed` at all —
a terminal frame just deletes the row from `open`. That also avoids a per-close array
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
| Server-paged table + per-row charts + detail modal | `live/components/history/HistoryTable.tsx` |
| Shared folds + renderers | `shared/components/analytics/` |
| Home digest + rule alerts | `live/components/home/ReviewDigest.tsx` |
| B1 handler | `core/src/api/handlers/strategies/rule_positions.rs::portfolio_positions_page` |
| B1 repo | `core/src/storage/repositories/strategy_repo.rs::{find_positions_all_paged,count_positions_all}` |
| B2 repo | `core/src/storage/repositories/strategy_repo.rs::{closes_series,entry_failed_count}` |
| B2 service | `live/src/services/portfolio.rs::closes_series` (+ `range_since`, the ONE range grammar) |
