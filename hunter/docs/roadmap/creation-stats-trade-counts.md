# Creation-stats: trade counts alongside token counts

**Status:** planned, not started
**Owner surface:** `/creation-stats` (lab page) + `GET /api/tokens/creation-stats[/grouped]`
**Scope rule:** token count stays the default everywhere. Trade metrics are strictly
additive - a new toggle option and extra payload columns, never a change of default.

## 0. What this is (and what it is not)

"Trade counts" on this page can mean two different things. This plan covers **A only**.

| | A - cohort trade activity (THIS PLAN) | B - market-clock trade activity (NOT in scope) |
| --- | --- | --- |
| Question | Of the tokens *created* in this bucket/group, how much did they get traded? | How many trades *happened* at Tue 15:00, whatever launched when? |
| Corpus | `tokens` LEFT JOIN `tokens_info` - already joined by every query on this page | `trades` hypertable / `trades_ohlcv_1h` CAgg |
| Cost | Extra aggregate columns on an existing GROUP BY. No new join, no new scan, no migration. | New per-mint-per-hour scan across the whole universe |
| Fits the page thesis ("when tokens launch vs. how they end up") | Yes | No - it is a different dashboard |

If B is ever wanted: build it as its own section over `trades_ohlcv_1h`
(`hunter/core/src/storage/timescale.rs` already maintains `trade_count` +
`volume_lamports` per mint per hour). Do NOT scan `trades` directly, and do not fold it
into the panels below - it cannot be segmented by mayhem/cashback or grouped by
fingerprint without a join back to `tokens`, which turns it into a hybrid of A and B.

## 1. Why A is cheap

All four query paths in `hunter/core/src/storage/repositories/creation_stats_repo.rs`
already `LEFT JOIN tokens_info` (`heatmap`, `trend`, `grouped` -> alias `ti`;
`grouped_scoped` -> alias `ti`; the drill-down builders -> alias `i`, via
`token_repo`'s `LIST_FROM`). `tokens_info` already carries:

- `trade_count` `i64` NOT NULL - lifetime trades for that mint
- `volume_sol` `f64` - lifetime SOL volume

So every metric below is a `SUM(...)` / `percentile_cont(...)` added to a GROUP BY that
already runs. No schema change, no migration, no new index.

## 2. The one real trap: age bias

`tokens_info.trade_count` is **lifetime-to-last-sync**, not "trades in the first N
minutes". A token created 25h ago has had one day to accumulate; one created 29 days ago
has had a month. Consequences:

1. **The trend view slopes down toward `now`** for a reason that has nothing to do with
   launch quality. This is the same class of bug the existing `maturity_secs` censoring
   fixes for migrate/dead, but censoring alone does NOT fully fix it (a matured token can
   still be 1d or 29d old).
2. `trade_count` freshness varies per token by sync state
   (`token_sync_state.last_synced_at`), so cross-token sums are approximate.

Decisions for this plan:

- **Censor trade metrics with the existing `maturity_secs` filter**, exactly like
  `migrated`/`dead`. Same `FILTER (WHERE t.created_at < now() - make_interval(secs => $2))`.
- **Lead with the median (`trades_p50`), not the sum**, for the per-cell readout. The
  heatmap folds day x hour across the whole window, so age averages out there; the median
  is also robust to a single 100K-trade outlier dominating a cell.
- **Label the metric honestly as lifetime trades** in the panel caption and tooltip, and
  add a one-line footnote about sync freshness.
- **Do NOT attempt true age normalization ("trades in the first 30m") here.** That needs a
  per-token windowed scan of `trades`/the lake, and it belongs in the sweep, not in a
  dashboard aggregate. Explicitly out of scope; note it here so it is not re-litigated.

## 3. Phase 1 - backend aggregate endpoint

File: `hunter/core/src/storage/repositories/creation_stats_repo.rs`

Add to `HeatCellRow` and `TrendPointRow` (identical expressions in both `heatmap()` and
`trend()`; they are already near-duplicates, so keep the new columns byte-identical):

```sql
COALESCE(SUM(ti.trade_count) FILTER (WHERE <matured>), 0)::bigint AS trades,
COALESCE(SUM(ti.volume_sol)  FILTER (WHERE <matured>), 0)::float8 AS volume_sol,
percentile_cont(0.5) WITHIN GROUP (ORDER BY ti.trade_count)
    FILTER (WHERE <matured> AND ti.mint_address IS NOT NULL) AS trades_p50
```

where `<matured>` is the existing `t.created_at < now() - make_interval(secs => $2)`
predicate, reused verbatim (do not re-spell it - if it ever changes it must change once).

Notes:
- `trades_p50` is `Option<f64>` (NULL when the cell has no matured+known token) and maps
  to `null` on the wire, so the UI renders the existing "no data" wash instead of a
  misleading 0 - same contract `metricValue` already has for the rate metrics.
- `SUM` needs `COALESCE(..., 0)` because the LEFT JOIN can null the whole group.
- `volume_sol` is optional in Phase 1; ship it if it costs nothing, but the UI work below
  only wires `trades` + `trades_p50`.

File: `hunter/core/src/api/handlers/tokens/creation_stats.rs`

- Add `trades: i64`, `trades_p50: Option<f64>` (+ `volume_sol: f64` if shipped) to
  `HeatCell` and `TrendPoint`, and wire them in `to_cell` / `to_point`.
- Add window totals `trades` / `trades_p50` to `CreationStatsResponse` next to
  `total`/`matured`/`known`. The per-bucket sum accumulates the same way the existing
  loop does; the window-level p50 cannot be summed from buckets - either compute it in a
  separate scalar query or drop the window-level p50 and show only the window sum in the
  StatCard. **Prefer dropping it** - one more query for one tile is not worth it.

No new query params. Everything ships in the existing payload so the metric toggle stays
a pure client-side re-color (the page's documented "all metrics in one payload" contract).

## 4. Phase 2 - shared frontend metric plumbing

File: `hunter/frontend/src/shared/components/creation-stats/creationStats.ts`

1. Extend the union:
   `export type CreationMetric = 'count' | 'migrate_rate' | 'dead_rate' | 'trades' | 'trades_per_token';`
2. Add `trades` + `trades_p50` to `CreationHeatCell`, `CreationTrendPoint`,
   `CreationStatsResponse`.
3. Append to `METRIC_OPTIONS` **after** the existing three:
   `{ value: 'trades', label: 'Trades' }`, `{ value: 'trades_per_token', label: 'Trades/token' }`.
   `count` remains first and remains the `useLocalStorage` default in
   `CreationStatsPage.tsx` - do not touch `STORAGE_KEYS.dashboardMetric`'s default.
4. Introduce a metric **kind** and drive normalization + label formatting off it, instead
   of the current `metric === 'count' ? ... : ...` ternaries (there are three of them:
   two in `CreationHeatmap.tsx`, one in the `CreationStatsPage.tsx` caption). Without this
   the two new metrics silently get the rate branch and render as a bogus percent.

   ```ts
   export type MetricKind = 'magnitude' | 'rate' | 'ratio';
   export const METRIC_KIND: Record<CreationMetric, MetricKind> = {
     count: 'magnitude',
     trades: 'magnitude',
     migrate_rate: 'rate',
     dead_rate: 'rate',
     trades_per_token: 'ratio',
   };
   ```

   - `magnitude` -> normalize `value / max(value)`, label = `formatCompact`
   - `rate` -> contrast-stretch across cells (existing behavior), label = `NN%`
   - `ratio` -> contrast-stretch across cells (unbounded, so stretch not divide), label =
     `formatCompact` (NOT a percent)
5. Widen `metricValue`'s input type to include `trades` / `trades_p50` / `matured`, and
   add the two arms: `trades` -> `d.trades`; `trades_per_token` -> `d.trades_p50` (already
   `null` when there is no coverage, so the existing null-handling applies unchanged).
6. Add `METRIC_RGB` entries. Existing: count teal `19,206,175`, migrate green
   `34,197,94`, dead red `239,68,68`. New: `trades` amber `245,158,11`,
   `trades_per_token` violet `167,139,250`. Do not reuse the buy/sell candle colors -
   those are reserved by the buy/sell color convention.

File: `hunter/frontend/src/shared/components/creation-stats/CreationHeatmap.tsx`

- Replace the two `metric === 'count'` ternaries (normalization at ~L149, in-cell label at
  ~L161) with the `METRIC_KIND` switch. The `magnitude` branch needs a per-metric max, so
  the `useMemo` that computes `maxCount` becomes "max of the active metric's value".
- Extend `EMPTY_CELL` with `trades: 0, trades_p50: null`.
- Extend the tooltip with a line: `Trades: <sum> (median <p50>/token, lifetime)`.
  This is the cheapest high-value part of the whole change - do it even if nothing else
  in Phase 2 lands.

File: `hunter/frontend/src/lab/pages/creation-stats/CreationStatsPage.tsx`

- The metric caption at ~L161 becomes kind-driven (`magnitude` -> "shade = share of max",
  `rate` -> existing text, `ratio` -> "shade = median per token, scaled across cells").
- StatCards: the row is currently 4-up (`Tokens created`, `Matured`, `Outcome coverage`,
  `Maturity window`). Add a 5th `Trades` tile and move the grid to `sm:grid-cols-5`, or
  fold trades into the existing `Tokens created` tile as a sub-line. Prefer the 5th tile.
  `Tokens created` stays first.

File: `hunter/frontend/src/shared/components/creation-stats/CreationTrendChart.tsx`

- Currently a single fixed histogram of `p.count` with a hardcoded teal. Make the plotted
  field + color metric-driven for the two `magnitude` metrics, and leave the `rate` /
  `ratio` metrics plotting `count` (a stretched rate is meaningless as a histogram).
  This means threading the active `metric` down as a prop from `CreationStatsPage`.
  Optional in Phase 2 - if deferred, the trend panel keeps plotting `count` and that is
  correct, just not new.

## 5. Phase 3 - grouped section (per-fingerprint cards)

This is the biggest analytical win: today `top` picks the top-N groups **by token count
only**, so a group of 40 tokens averaging 3K trades each can never reach the top 8 while
4,000 dead-on-arrival launches always do.

File: `hunter/core/src/storage/repositories/creation_stats_repo.rs` (`grouped`)

- Add `SUM(ti.trade_count)` to the `base` CTE selection and to the `ranked` aggregate, so
  `GroupedGroupRow` gains `trades: i64` beside `total`.
- Add a `rank_by` parameter threaded from the handler: `"count"` (DEFAULT, current
  `ORDER BY COUNT(*) DESC, gkey::text`) or `"trades"`
  (`ORDER BY SUM(ti.trade_count) DESC, gkey::text`). Whitelisted enum, never interpolated
  free text - same discipline as `normalize_bucket`.
- `GroupedGroup.total` keeps meaning **token count**. Do not repurpose it.
- Per-cell / per-point trades (`GroupedCreationCell.trades`) are **deferred**. Group
  totals drive the ranking and the card label; the small-multiple heatmaps only need
  per-cell trades if a trades shading mode is added there, which is not in this plan.

File: `hunter/core/src/api/handlers/tokens/creation_stats.rs`

- `GroupedCreationQuery`: add `rank_by: Option<String>` -> `normalize_rank_by()`
  defaulting to `"count"`, echoed back on `GroupedCreationResponse`.
- `GroupedGroup`: add `trades: i64`.
- `grouped_scoped` (the saved-fingerprint path): add the same `trades` sum. There is only
  ever one group there, so `rank_by` is inert - accept and ignore it, matching how that
  path already ignores `group_by`/`top`/`field_filters`.

File: `hunter/frontend/src/lab/components/creation-stats/groupedCreationStats.ts`

- `GroupedCreationGroup` gains `trades: number`; `GroupedCreationArgs` gains
  `rankBy?: 'count' | 'trades'` (omitted when `count`, so the default keeps a stable RTK
  cache key - same trick `bucketWidth` already uses at
  `GroupedCreationSection.tsx` L236).
- `toHeatCell` zero-fill gains the new `CreationHeatCell` fields.

File: `hunter/frontend/src/lab/components/creation-stats/GroupedCreationSection.tsx`

- New `Select` next to the existing `Top N` select: "Rank by: Tokens / Trades", stored in
  a new `STORAGE_KEYS.groupedRankBy` **defaulting to `'count'`**. It joins `draftArgs`, so
  it participates in the existing dirty-check / Analyze snapshot like every other control.
- Card header at L575: `1,234 tokens` becomes `1,234 tokens - 89K trades`.
- Legend chip at L530 (`- {formatWithCommas(g.total)}`): append the trades count.
- Drill-down table: **nothing to do.** It reuses `tokenColumns()`, which already carries a
  sortable + numerically-filterable `trade_count` column.

## 6. Explicit non-goals

- Age-normalized "trades in first N minutes" (needs a per-token windowed trade scan -
  belongs in the sweep).
- Market-clock trade seasonality (variant B above).
- Per-cell trades in the grouped small-multiple heatmaps.
- Any change to the default metric (`count`) or the default group ranking (token count).
- Any change to `MAX_TRADES_RETAINED`, cache TTLs, or anything that touches the live box.
  This is a lab-page change; the `live` bin serves the same core handler but the page is
  lab-only.

## 7. Definition of done

- `cargo check -p hunter-live` + `cargo check -p hunter-lab` clean; clippy on touched code.
- New repo/handler unit tests, in the style already in these two files:
  - `normalize_rank_by` whitelists + defaults to `count` (mirrors `bucket_defaults_and_whitelists`).
  - The trade-metric SQL reuses the same maturity predicate as the outcome columns - assert
    the emitted SQL contains it exactly once per metric, so a future edit cannot drift the
    censoring between `migrated`/`dead` and `trades`.
  - Grouped `rank_by=trades` emits `ORDER BY SUM(ti.trade_count) DESC` and `rank_by`
    absent emits the existing `COUNT(*) DESC` (guards the "default does not change" rule).
- `npm run build:live` + `npm run lint` clean.
- No extra re-render on a SOL/USD or live-trade tick: the new heatmap `useMemo` still keys
  only on `cells`/`metric`/`total`.
- Manual smoke on `/creation-stats`: default load is byte-identical to today (count metric,
  count ranking); toggling to Trades / Trades-per-token re-colors with **no refetch**
  (verify in the network tab - this is the payload contract, not an incidental).
- Docs: fold the shipped behavior into `docs/arch/frontend.md` (page/section description)
  and `docs/arch/database.md` (the creation-stats repo's returned columns), then delete
  this file per the roadmap discipline.
