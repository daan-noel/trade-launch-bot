# Position summary (Evidence / Simulate / Sweep)

> Shared "summary above positions table" chrome. Console History keeps its own
> charts deck + `hfocus`, and reuses the exit mix / Exits tiles via
> `HistoryExitSummary` (same `runSummarySections` fold) — do not mount this
> shell on History.

## Shell

`PositionSummarySection` (`shared/components/strategy/PositionSummarySection.tsx`):

```text
1. Hero KPIs + exit mix + focus chips
2. Details bands (Positions / Exits / Realized / MTM / Capital*) — always on
3. ▾ Charts — ONE toggle collapsing everything below (`useState`, like Console History)
4. Equity path | Return shape — `grid xl:grid-cols-2` (stack below xl)
5. Hold mix | Wall clock — `xl:grid-cols-2` (`TemporalSummary` `variant="deck"`)
6. Timing — one card, `xl:grid-cols-3`: Daily PnL calendar (1/3) | When it trades (2/3)
7. Hold vs PnL — full width
```

A collapsed deck returns the stable empty cohort/`EMPTY_PNL_DECK` from the fold memos, so
hiding the charts also skips the cohort walk — not just the paint.

The heatmap's 24 hour columns need the wide two thirds, which is why Timing is a full-width
card holding two panels rather than a heatmap cell in a 2-col grid (same split as Console).

\* Capital tiles (Evidence only) are display-only — never add a focus lens.

Chart cards use the same chrome as Console History (`border` + panel + caps title).
Hold / wall no longer use the legacy "Temporal pattern" band — DryRunDetail still can.

## Cohort

```text
Run scope → table filters → Focus chips (stacked)
         ↓
 one cohort → summary + charts + table page
```

- Summary + charts use the **full matching cohort**, never the current table page.
- **Timing charts** (calendar + heatmap) stay on the table-filter cohort (minus the other
  non-timing lenses) with a selection ring — same timing-vs-lens split as Console. Other
  charts fold the focused slice, timing lenses included.
- **Hold vs PnL** keeps its own `band` lens off the paint cohort (domain zooms instead of
  emptying the plot) and passes the table-filter cohort as `contextPoints` so an empty
  cross-chart focus keeps axes + Reset mounted — bare text only when the parent cohort
  has nothing to plot.
- Evidence / Simulate: page the positions/sim result endpoint for chart series
  (`fetchAllTablePages`); focus → table via `buildFocusTableFilters` (structured
  predicates + key-resolved `id in` / `mint_address in` from the chart cohort).
- Sweep: full combo rows in memory; pin the table-filter cohort when focus activates.

## Focus

`lib/strategy/positionFocus.ts` — stacked lenses (`status`, `exit`, `outcome`,
`migrated`, `hold`, `wall`, `pct`, `pos`, `band`, `heat`, `day`, `week`). Re-click clears
that kind; Clear all drops every lens. Independent of Console `hfocus`.

Parents call `buildFocusTableFilters(lenses, chartPoints, timeZone, mode)` where
`mode` is `positionId` (Evidence UUID) or `mint` (Simulate episode keys).

| Lens | Evidence (`positionId`) | Simulate (`mint`) |
| --- | --- | --- |
| `pct` | `pnl_pct` via `pctFocusFilter` (`lt`/`gte`/half-open `between`) | same |
| `status:open/closed` | `status` neq/eq `End` (text `Neq` must reach SQL) | open → `exit_reason eq Open`; closed → key-resolved |
| `status:fired` | `exit_reason neq NoEntry` | same |
| `exit:Metric±` | `Metrics` contains **and** `pnl_sol` cut | same |
| `exit:Other` | key-resolved `id in` | key-resolved mint set |
| `pos` | `id eq` UUID | `mint_address` + `entry_time` from `parseEpisodeRowKey` |
| `band` | key-resolved (no hold-seconds column) | `holding` + `pnl_pct` between |
| `heat`/`day`/`week` | key-resolved `id in` | key-resolved mint set |

`pct` open tails (`< -50%` / `≥ 500%`) use ±Infinity — never gate on
`Number.isFinite`. Wire column key **`pnl_pct`** (Evidence whitelist).

The three **time lenses** share `timeLensMatches` with the chart refold and chip
so a calendar cell and the rows it narrows never disagree. `week` is half-open
`[Sun, +7)` over day keys.

## Charts

Reuse `shared/components/analytics/{PnlDistribution,HoldPnlScatter,EquityCurveChart,PnlHeatmap,PnlCalendar}`
+ `foldPnlDeck`. Density preference shares `hunter.pnlDistDensity` with Console.
Open positions use unrealized PnL when present on the wire (`isOpen` on
`PositionChartPoint`). **No** rule-comparison strip here (a run is one rule).

The calendar renders `CALENDAR_WEEKS = 10` columns: past that, one third of the row
truncates the in-cell SOL labels to nothing.

## Wall clock vs Timing — why the numbers differ (locked)

They are **not** two views of the same fact, and reconciling them is a support question
that comes back. Two independent reasons:

| | Wall clock | Timing (calendar + heatmap) |
| --- | --- | --- |
| Bins on | `created_at` (default) or `entry_time` | the **decision instant** — exit time, or entry for open marks |
| Shows | count of fired positions (color = count or PnL) | net SOL |

A position created Mon 23:40 and exited Tue 00:20 is a **Monday** wall bar and a
**Tuesday** calendar square. That is by design; do not "fix" it by re-pointing one at the
other's timestamp.

**Both bin in the app timezone** (`useTimezone()`), and that part is a hard rule:

- Wall buckets are **civil** buckets. `floorToWallGrain` / Rust `floor_to_grain_in_zone`
  floor the *local* wall-clock, never the raw epoch — an epoch floor makes `day` a UTC day
  (start 19:00 the previous local day at UTC-5) and misaligns 2h/4h plus every grain in a
  half-hour zone (+05:30). Both are DST-safe in two passes; the vectors are duplicated in
  `sim_query::wall_buckets_floor_in_the_requested_zone` and the FE
  `floorToWallGrain` describe block, and the two folds must stay equal.
- A DST day is 23 h or 25 h ⇒ **never build the cell grid with `t += step`**: seed the real
  row keys (boundaries by construction) and let the walk fill only the gaps, or rows past
  a transition are dropped. Cell `end` is the *next* boundary, so cells stay contiguous.
- Simulate/dry-run fold on the **server** (`POST …/result/time-summary`), so the zone
  travels as the `tz` query param; absent/unknown `tz` ⇒ UTC (never the server's zone).
- Every wall label (`formatWallTick` / `formatWallClock` / `formatWallDate` /
  `isWallDayBreak`) takes the zone explicitly. A bare `toLocaleString()` renders the
  **browser** zone, which is not the app zone — the axis then disagrees with the calendar
  beside it.

## Surfaces

| Surface | Mount |
| --- | --- |
| Evidence | `RuleAnalyzePanel` |
| Simulate | `SimulatePage` → `RuleSimPositionsPanel` |
| Sweep drill-in | `GenericSweepView` → `ComboTokenResults` |
