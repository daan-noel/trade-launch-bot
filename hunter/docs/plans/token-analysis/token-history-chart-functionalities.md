# Token History Chart — Functionalities Reference

> A feature-by-feature map of the token price/history chart: **what each control,
> overlay, marker, and interaction does**, how it's triggered, and where it lives in
> code. The OHLC/bar math itself is client-side in `chartBars.ts`; the canonical price
> definition it renders is [`@plans/database/trades-storage.md`](../database/trades-storage.md).
> Reuse this file as a prompt to extend or re-implement the chart UI.
>
> **Key files** (all under `hunter/frontend/src/shared/components/token-price-chart/`)
>
> - Main component: [`TokenPriceChart.tsx`](../../frontend/src/shared/components/token-price-chart/TokenPriceChart.tsx)
> - Toolbar: [`ChartToolbar.tsx`](../../frontend/src/shared/components/token-price-chart/ChartToolbar.tsx)
> - Bottom zoom/pan slider: [`ChartRangeSlider.tsx`](../../frontend/src/shared/components/token-price-chart/ChartRangeSlider.tsx)
> - Canvas plugins: [`rangeSelectPlugin.ts`](../../frontend/src/shared/components/token-price-chart/rangeSelectPlugin.ts), [`walletMarkersPlugin.ts`](../../frontend/src/shared/components/token-price-chart/walletMarkersPlugin.ts)
> - Tooltips: `BarCrosshairTooltip.tsx`, `WalletMarkersTooltip.tsx`, `RangeSelectTooltip.tsx`, field renderers `BarCrosshairFields.tsx` / `BarFlowFields.tsx`
> - Viewport & time helpers: [`chartViewport.ts`](../../frontend/src/shared/components/token-price-chart/chartViewport.ts), [`chartTimezone.ts`](../../frontend/src/shared/components/token-price-chart/chartTimezone.ts)
> - Bar math: [`chartBars.ts`](../../frontend/src/shared/components/token-price-chart/chartBars.ts)
> - Shared types / constants: [`types.ts`](../../frontend/src/shared/components/token-price-chart/types.ts), [`constants.ts`](../../frontend/src/shared/components/token-price-chart/constants.ts)

---

## 1. Overview

`TokenPriceChart` renders a single token's trade history as a candlestick/line chart
built on [`lightweight-charts`](https://github.com/tradingview/lightweight-charts). It
takes a flat list of `Trade` rows (`trades` prop) and aggregates them **client-side** into
OHLC bars (no server-side candles). On top of the base series it layers several optional,
independently-toggleable features:

- **Trade-count markers** (per-bar buy/sell arrows)
- **Wallet markers** (per-tracked-wallet circles)
- **ATH / Migration** reference price lines
- **Range-select** mode (drag to summarize a time window)
- A bottom **range slider** for zoom/pan

Most boolean toggles are **persisted to `localStorage`** so the user's chart layout sticks
across reloads; a few (range mode, selections) are session-only UI state.

---

## 2. Bar grouping: Time mode vs. Slot mode

The chart aggregates trades two ways, chosen by the **`groupMode`** toggle (`'time'` |
`'slot'`):

| Mode | Bucket key | Aggregator | Notes |
| ------ | ----------- | ----------- | ------- |
| **Time** (default) | `floor(block_time_sec / intervalSec) * intervalSec` | `aggregateTradesToBars` | interval selector active |
| **Slot** | the raw Solana `slot` number | `aggregateTradesToBarsBySlot` | interval selector **disabled**; one bar ≈ one block (~400 ms) |

- **Interval selector** (`1s` / `30s` / `1m` / `5m`, from `CHART_INTERVALS` in `constants.ts`)
  only applies in time mode; the toolbar greys it out in slot mode (`intervalsDisabled`).
- The computed `bars` array depends on `groupMode`, `intervalSec`, the sorted trades, and the
  active `metric`. Time-axis labels are timezone-aware in time mode and a plain `Slot N` in
  slot mode (see §10).
- OHLC construction itself (continuous bars, canonical `slot → tx_index → leg_index`
  trade ordering — `tx_index` is the authoritative intra-slot key, no reserve-chain
  reconstruction — dust filtering) lives in `chartBars.ts`.

---

## 3. Series style & metric

### 3a. Chart style (`style`: `'candles'` | `'line'`)

Toggled by the candle/line icon group in the toolbar (`handleStyleChange`, persisted).

- **Candles** (default): `CandlestickSeries` with `CANDLE_SERIES_OPTIONS`. Selected bars
  are repainted (filled highlight) via `barsToCandleData(bars, highlightBarTimes)`.
- **Line**: `LineSeries` (`LINE_SERIES_OPTIONS`) drawing the close price as a continuous teal line.

### 3b. Metric (`metric`: `'price'` | `'mc'`)

Only rendered when the parent passes an `onMetricChange` callback.

- **Price** — spot SOL/token.
- **MC** — market cap = `TOKEN_TOTAL_SUPPLY × spot`.

The metric is a **parent-controlled** prop (the chart calls `onMetricChange`), and it rescales
the Y-axis and every price formatter.

### 3c. Price unit & formatting

`priceUnit` (`'SOL'` | `'USD'`) + `toValue()` converter + `priceLabel` come from the parent.
`createChartPriceFormatter(priceUnit)` (`constants.ts`) prefixes values with **◎** (SOL) or
**$** (USD) and is applied to right-axis labels and all tooltips.

---

## 4. Toolbar controls (`ChartToolbar.tsx`)

The toolbar has two rows. **Row 1**: title + status badges + live crosshair readout, then the
pill groups (group mode, interval, style, metric) and the marker/line toggles. **Row 2**
(right-aligned): the range controls.

| Control | Type | Effect | Persisted? |
| --------- | ------ | -------- | :---------: |
| **Time / Slot** | pill group | `groupMode` (§2) | ✓ |
| **1s/30s/1m/5m** | pill group | `intervalSec`; disabled in slot mode | ✓ |
| **Candles / Line** | icon group | `style` (§3a) | ✓ |
| **Price / MC** | pill group | `metric` (§3b); only if `onMetricChange` set | parent |
| **Buy/sell counts** | icon toggle | per-bar trade-count markers (§5) | ✓ |
| **Trim gaps** | icon toggle | drop flat/empty bars (`dropEmptyBars`) | ✓ |
| **ATH** | checkbox | ATH reference price line (§7); disabled if no ATH data | ✓ |
| **Migration** | checkbox | bonding-curve graduation price line (§7) | ✓ |
| **Range select** | icon toggle | drag-to-select range mode (§6) | session |

### 4a. Opening state (`DEFAULT_CHART_PREFS`)

The persisted toggles start from one shared default in `constants.ts`, tuned for the read
this chart is used for — **what a token did in its first seconds**:

| Pref | Default | Why |
| --- | --- | --- |
| `interval` | `1s` | a `1m` candle swallows the entire window that decides an entry |
| `groupMode` / `style` | `time` / `candles` | — |
| `showDevMarkers` + `devMarkersBoundariesOnly` | both **on** | the dev's `first_buy`/`sell_all` are the signal; their manufactured mid-position churn is noise |
| `showWalletMarkers`, `showEventMarkers`, `showAthLine`, `showMigrationLine`, `showFlowLines` | on | read every time; the toolbar disables each when its data is absent, so they cost nothing |
| `showTradeMarkers` | **off** | the per-bar buy/sell count badge is one badge per candle at `1s` — it hides the price action it annotates |
| `trimEmptyBars` | **off** | no-trade gaps ARE information (a stalled token); dropping them distorts the time axis |

Both apps share this default — it is not split per app. `FlowPreviewChart` keeps its own
`DEFAULT_FLOW_CHART_PREFS` (different toolbar, own storage key).

The toolbar **wraps**: the control cluster is shrinkable (no `shrink-0`) and both levels
carry `flex-wrap`, so in a narrow host — the Console's 380px manual-trade column, the
Portfolio/Floor row details — it drops to its own full-width line and re-flows into rows.
Pinning the cluster at its ~600px max-content width overflowed the panel and gave the whole
page a horizontal scrollbar. The title keeps a `min-w` floor so the break happens before the
symbol is crushed away.

**Status badges** (only shown when `isMigrated != null`): `Migrated ✓` / `Bonding Curve`,
plus optional `Mayhem` and `Cashback` badges, colored per `STATUS_BADGE_COLOR`.

**Live crosshair readout**: an `aria-live="polite"` line under the title showing the hovered
bar's O/H/L/C (candles) or price (line) + Vol/Liq, rendered by `BarCrosshairFields` in
`layout="inline"`. It mirrors the floating bar tooltip (§9) so the values are always visible
even when the pointer is deep in the chart.

Icon-only controls each have an instant dark `HoverTooltip` because their label lives only in
the tooltip; toggles expose `aria-pressed` for accessibility.

---

## 5. Trade-count markers (buy/sell per bar)

When **Buy/sell counts** is on (`showTradeMarkers`, default **off** — see §4a),
`buildTradeMarkers` emits a
lightweight-charts marker per bar:

- Counts buys vs. sells in the bar; text like `↑3 ↓2`.
- **Green** arrow below bar for buy-dominant, **red** above for sell-dominant, **gray** in-bar
  when mixed.
- Purely informational; does not change bar geometry.

---

## 6. Range-select mode (drag-to-summarize)

Toggled by the **Range select** icon. While active (`rangeSelectMode`):

- Chart **pan/zoom is disabled** and the cursor becomes a crosshair.
- **Left-drag** draws a band; each edge **snaps to the nearest bar** via logical coordinates.
  A live dashed preview follows the drag.
- **Release**: if the drag exceeds a ~4 px threshold it **commits** the range (solid border);
  a shorter drag clears it. **Escape** clears any committed range.
- Rendering is a canvas overlay, `RangeSelectPlugin` (`rangeSelectPlugin.ts`): translucent
  teal fill, a top-centered **label chip** showing the duration (`formatRangeDuration`), and a
  hit-testable label (`containsLabelPoint`).

**Range stats** (`computeRangeStats` in `chartBars.ts`) summarize the selected window and are
shown by `RangeSelectTooltip` when you hover the chip:

- Flow: `inflow`, `outflow`, `netFlow` (in the chart's display unit)
- Counts: `tradeCount`, `buyCount`, `sellCount`
- Wallets: `uniqueWallets`, `uniqueBuyers`, `uniqueSellers`
- Extremes: `maxBuySol`, `maxSellSol`
- `durationMs`, `priceDelta`, `priceDeltaPct`

The selection is also surfaced to the parent via `onRangeChange` (if provided).

---

## 6a. Selected-trades panel (what a candle is made of)

`TokenPriceChart` owns no trades table — it only *emits* the pick. A click on a bar fires
`onBarClick` (clicking the same bar again, or empty space, clears it) and a committed drag
fires `onRangeChange`. A host that wires neither leaves both interactions inert, which is
what a chart too narrow for a table wants.

Three pieces, all shared, so every chart lists trades the same way:

| Piece | Where | Job |
| --- | --- | --- |
| `useBarTradesSelection` | `components/tokens/` | holds the bar + range pick (**mutually exclusive** — one table at a time), returns `chartProps` to spread onto the chart |
| `tradesInBar` / `tradesInRange` | `token-price-chart/barTrades.ts` | the ONE bucket matcher — same key the chart bars by (`tradeBarTime` / slot), so the table can't list a different set than the candle drawn |
| `BarTradesPanel` | `components/tokens/` | the heading + count + Clear + `DataTable`; tints entry/exit fill rows from `eventMarkers` and accents our own wallets. Renders nothing when nothing is picked |

Hosts: `TokenTradeChart` (Tokens / Sync / MyWallet / Replay / Lab inspect) renders the panel
directly under the chart and can hand the panel to an outside pick via `externalSelection`
(a swing leg chosen in a sibling table). `FloorPositionDetail` (live Console, Portfolio,
Floor, Rules Evidence) uses `MintBarTradesPanel`, which reads the mint's trades from the
same RTK Query cache the chart already filled — listing a bar costs no extra request — and
places the table **below** the chart ∥ fills grid, where it has the full width.

A host outside `token-price-chart` must deep-import (`components/token-price-chart/barTrades`,
`.../types`) rather than the barrel: the barrel re-exports `TokenPriceChart`, and a
statically-mounted host must not pull `lightweight-charts` into its chunk (see
[`@arch/frontend.md`](../../arch/frontend.md) chart code-split).

### 6b. Editing `volume_ix_patterns` from the trades table

The panel's **Vol** badge is the editing control for `m_flow_split.volume_ix_patterns`:
clicking it adds/removes that row's ordered `instruction_labels` on the target fingerprint
and **saves immediately**. There is no staging step — a draft copy would be a second answer
to "what counts as volume", and the surfaces reading the two copies then disagree on screen
while both look authoritative. The write invalidates the `Fingerprint` tag, so the chart
lines, the metric panes and the badge all redraw from the row that was just written; the
engine picks it up on its next rules reload. `togglePattern` in `lib/flow/volumePatterns.ts`
is the ONE toggler — Flow Discovery's structure checkboxes call it too.

**Which row it writes to is `useVolumePatternTarget`, and it is never guessed while a fact
is available.** `resolveVolumePatternTarget` ranks: an explicit pick from the bar's select,
then the host's own `flowFingerprintId`, then a lone pattern-set match. The order is the
whole point. Matching by SET cannot identify a row — `metric_config` is not part of
fingerprint identity, so any number of rows may carry the same patterns, and every
*unconfigured* row carries the same empty set, which is exactly the state authoring starts
from. A set-first resolver therefore fails precisely when the feature is first used: the
badge goes dead when several rows match, and writes to whichever unrelated row happens to be
the only empty one when just one does. Hence hosts pass `flowFingerprintId` alongside
`flowPatternKeys` all the way down (`hooks/useFlowPatternKeys` resolves both as one
`FlowPatternSource`), a match is taken only when exactly one row carries the set and is
labelled `matched by patterns — confirm`, and picking away from the host is labelled too,
since the badges then answer for a different row than the lines above them.

Three further rules the surface exists to enforce:

- **The badge tests structure; the lines apply contagion.** A row reads `Non-vol` while its
  SOL sits on the vol line whenever the wallet was already tagged. `useFlowReasons` runs
  `flowReasonsById` over the host's **full** history (contagion is forward-only, so a
  single bar's rows cannot reconstruct it) and the cell appends `via creator` / `via
  wallet`. Without that marker a toggle that "does nothing" looks like a bug.
- **The first pattern reveals the overlay.** `flowLinesAvailable` is false with no patterns
  and no creator wallet, and `showFlowLines` is a persisted pref — so the chart auto-enables
  the lines on the transition to classifiable. Turning them back off stays the user's call.
- **A run snapshot is not editable.** `flowReadOnly` marks a subtree whose patterns are a
  stored fact — the grouped-sweep drill-in, whose numbers were computed under the run's own
  `volume_ix_patterns`. It shows `run snapshot` instead of the edit control and skips the
  fingerprint/rule fetches entirely.

`VolumePatternBar` states the target and how many **active** rules use it before any click.
That count is the whole warning: `metric_config` is not part of fingerprint identity, so a
write does not fork the row — it lands on the same id and every rule bound to it starts
classifying flow differently.

---

## 7. Reference price lines (ATH & Migration)

Both are drawn with `series.createPriceLine()` and respect the active metric/unit:

- **ATH** (`showAthLine`): dashed golden (`#f0b429`) line at the all-time-high price, computed
  from `athPriceInSol` through the metric/unit converter (`athChartValue`). The checkbox is
  **disabled** (`athLineAvailable === false`) when the token has no recorded ATH. ATH itself is
  authoritative backend data, not recomputed here.
- **Migration** (`showMigrationLine`): dashed teal-blue (`#5dade2`) line at the fixed pump.fun
  bonding-curve graduation price `PUMP_MIGRATION_SPOT_PRICE_SOL` (`constants.ts`) — a constant,
  not token-specific.

---

## 8. Wallet markers (tracked profile wallets)

When the parent passes `profileWallets` (array of `ProfileWalletInfo`), each tracked wallet's
trades are marked with a colored **circle** drawn by `WalletMarkersPlugin`
(`walletMarkersPlugin.ts`):

- `buildWalletMarkerDefs` groups a wallet's trades by bar + side, de-duplicates to one marker
  per wallet per bar per side, and **stacks** buy markers below / sell markers above the bar so
  they never overlap.
- Each circle is filled with the wallet's palette color (`WALLET_MARKER_COLORS`, cycled), bears
  the first letter of the profile/wallet name, and uses a green (buy) / red (sell) border.
- Hovering a circle (`containsPoint`, distance < radius) opens `WalletMarkersTooltip`, listing
  each wallet at that bar: profile name / shortened address, optional tags, and buy/sell counts
  - total SOL. The per-bar summary comes from `buildWalletBarActivityMap`
  (`WalletBarActivity`: counts + buy/sell SOL per wallet per bar).

---

## 9. Crosshair tooltips & priority

Hovering the chart can surface one of several floating tooltips. On every crosshair-move the
component decides **which single tooltip to show** (others are cleared), roughly in this
priority:

1. **Range label** hovered → `RangeSelectTooltip` (§6)
2. **Wallet marker** hovered → `WalletMarkersTooltip` (§8)
3. Otherwise, over the main series → **bar tooltip** `BarCrosshairTooltip`

The **bar tooltip** and the toolbar readout carry **disjoint** facts — never the same ones
twice, since both are on screen simultaneously:

- **Toolbar readout** (`BarCrosshairFields`, `layout="inline"`) = the *price* view: for candles
  **O/H/L/C** (colors from `CHART_OHLC_COLORS`) plus Vol/Liq; for line, Price + Vol/Liq. Plus
  the cumulative VolMk/NonVol pair when flow lines are available.
- **Bar tooltip** (`BarCrosshairTooltip`) = what the toolbar *cannot* say — **which** bar is
  hovered (timezone/slot-formatted bar time + `+age` since token creation) and its per-bar
  **order flow** via `BarFlowFields`: Net / In / Out / Δ%, then VolMk / NonVol.

A chart that repeats the O/H/L/C block inside its own tooltip is the bug — `FlowPreviewChart`
did until it was switched onto the shared `BarCrosshairTooltip`.

Both boxes place horizontally through `tooltipHorizontalStyle` (flip to the cursor's left near
the panel's right edge), so every chart must pass the live `containerWidth`.

Bar age is single-sourced in `chartBars.ts`: `tokenCreatedAtSec` + `buildBarEarliestTradeSec`
+ `barAgeSec` (null on an empty bar in slot mode — a slot number is not a wall clock).

---

## 10. Zoom, pan & the bottom range slider

### 10a. Viewport preservation (`chartViewport.ts`)

Because bars are rebuilt whenever interval/group/metric/trades change, the chart must not
"jump" back to fit-content on every update. The helpers:

- `captureChartViewport` snapshots the current **logical** range together with the
  `barsShape` (`{length, first, last}`) it was measured against — the baseline needed to
  translate it onto the next bar array.
- `shiftLogicalRange` does that translation. Restoring by **time** is wrong:
  `timeScale.setVisibleRange` snaps its endpoints onto bar boundaries, so a tight zoom drifts
  a little on *every* trade until it no longer looks like the window the user set. Logical
  indices are exact once the array shift is known.
- The shift is anchored on the **last** bar of the old array, not the first. Bars are appended
  on the right by live trades but can also be dropped from the left (rolling window,
  `trimEmptyBars`), and only the last-bar anchor gets that second case right — a first-bar
  anchor cannot tell "two bars trimmed off the front" from "no change".
- A window that already sat at the live edge (within `LIVE_EDGE_SLACK_BARS`) is shifted by the
  appended count so it keeps following new trades; a scrolled-back window stays exactly put.
- `restoreChartViewport` / `reapplyChartViewport` apply it (the latter double-applies across
  a `requestAnimationFrame` because lightweight-charts re-lays-out on the next frame).
- On first mount the chart `fitContent()`s once, then preserves the user's view.

### 10b. Vertical (price) scale — manual Y zoom is sticky (`dualPriceScaleSync.ts`)

The chart runs **two price scales**: right = token price/MC, left = the vol/non-vol flow
overlay. `attachDualPriceScaleSync` keeps their Y zoom in lockstep — a drag on one axis
mirrors the *relative* zoom onto the other via `setVisibleRange`, which implicitly turns
that scale's `autoScale` off.

Re-arming `autoScale` is therefore a normal part of the dance, but it must **never** be
driven by a data update. `subscribeVisibleLogicalRangeChange` fires for programmatic range
changes too — a live trade means `setData` **plus** the §10a viewport restore, i.e. two
range changes — so an unconditional re-arm there wiped a hand-set price zoom on the next
trade. The same held for the flow-overlay effect, whose `alignedFlowLines`/`toValue` deps
churn on every trade and every SOL/USD tick.

The rule, mirroring lightweight-charts' own semantics:

- **The library's own `autoScale` option is the authority.** lightweight-charts clears it
  inside `PriceScale.scaleTo` (its axis scale gesture) and sets it in `PriceScale.reset`
  (axis double-click), so `priceScale(id).options().autoScale === false` *is* the record of
  who owns the axis. `syncManualFromChart` latches `manualPriceZoom` from it before every
  re-arm. The one thing it must discount is our **own** mirror write: `setVisibleRange` also
  clears `autoScale`, so the mirrored scale id goes into `ourAutoScaleOff` and its `false`
  is not read as user intent.
- A pointer-down inside an axis gutter (hit-tested against `priceScale(id).width()`)
  followed by a drag latches the same flag. Kept as a second signal because the hit-test
  catches the gesture a frame earlier — but it is **not** sufficient on its own: it sees only
  a drag that *starts* in a gutter, so it missed pinch/touch scaling and any chart whose
  container rect does not line up with the axis (the position-detail modal). The gesture's
  **origin** is still the right signal for the hit-test — inferring it from "only one scale's
  range moved" misfires whenever a body pan leaves the flow line flat, which would freeze Y
  on an ordinary horizontal drag.
- The flag lives in a **ref owned by the component**, passed in as `opts.manualZoom`, not in
  the sync closure. The chart is destroyed and rebuilt on any `loading`/`error`/empty flip
  (`showChart` is a dep of the create effect), and a closure-local flag handed the axis back
  to autoScale on the way through. It is cleared only when `id`/`groupingKey` changes, i.e.
  when the axis means something else.
- `attachDualPriceScaleSync` returns a handle, not a bare disposer:
  **`rearm()`** re-fits unless the user holds manual control (use for any data-driven
  refit), **`reset()`** drops manual control and re-fits (only for changes to what an axis
  *means* — overlay toggled, unit/basis switched), `detach()` disposes.
  Call `rearmDualAutoScale` directly only from inside the sync module.
- Double-click **on a price axis** always resets Y (the library's native gesture);
  double-click on the chart **body** re-fits only when the user has not taken manual
  control, so resetting the time zoom can't silently drop the price zoom.

Consumers key their `reset()` on a string of the non-data inputs
(`flowLinesVisible|flowBasis|priceUnit|style|groupingKey`) held in a ref, so a data update
can never reach it.

### 10c. Bottom range slider (`ChartRangeSlider.tsx`)

Shown when there is more than one bar. It's a miniature scrollbar over the full data span with
a teal "window" marking the visible range. Three drag modes:

- **left handle** (`from`) / **right handle** (`to`) — resize the visible window edge
- **middle** (`pan`) — slide the window

It enforces a minimum window (`MIN_WINDOW_RATIO`) and calls
`chart.timeScale().setVisibleRange(from, to)` on change; conversely it syncs back from the
chart's visible range so dragging on the chart updates the slider.

---

## 11. Timezone & time formatting (`chartTimezone.ts`)

Time-mode axis labels and tooltips are timezone-aware via a `useTimezone()` context.
`createChartTimeFormatters(timezone)` builds:

- `timeFormatter` — full `YYYY-MM-DD HH:mm:ss` for the crosshair/tooltip,
- `tickMarkFormatter` — compact `MMM D HH:mm` for axis ticks,

both backed by `Intl.DateTimeFormat(undefined, { timeZone })`. In slot mode times are rendered
as plain `Slot N` strings instead.

---

## 12. Props & state quick reference

**Notable props** (`TokenPriceChartProps` in `types.ts`): `trades`, `loading`/`error`,
`toValue`/`priceUnit`/`priceLabel`, `metric` + `onMetricChange`, `height`, `onBarClick` /
`selectedBar`, `onRangeChange`, `athPriceInSol`,
`isMigrated` / `isMayhemMode` / `isCashbackEnabled`, `profileWallets`, `tokenCreatedAt`,
`eventMarkers`.

**Persisted (localStorage) state**: `groupMode`, `interval`, `style`, `showTradeMarkers`,
`showAthLine`, `showMigrationLine`, `trimEmptyBars`, `showWalletMarkers`,
`showDevMarkers`, `devMarkersBoundariesOnly`, `showEventMarkers`, `showFlowLines`.

**Session-only UI state**: `rangeSelectMode`, `selectedRange`, `selectedBar`, and the
hover/tooltip states (`crosshair`, `barTooltip`, `rangeTooltip`,
`walletMarkersTooltip`), plus `sliderWindow`.

---

## 13. Extending the chart (checklist)

1. **New per-bar marker** → build a `SeriesMarker[]` like `buildTradeMarkers`, or a canvas
   `ISeriesPrimitive` plugin (as `walletMarkersPlugin.ts`) when you need custom geometry/hit-testing.
2. **New overlay line** → create a `LineSeries`, feed it `{ time, value, color? }` points, and
   key it so the recreate-effect can diff it.
3. **New full-height band** → follow `rangeSelectPlugin.ts`
   (`ISeriesPrimitive` paint + a hit-testable label chip + a React tooltip).
4. **New toggle** → add a control in `ChartToolbar.tsx`, thread the prop/handler, and decide
   persisted (localStorage) vs. session state. Keep `aria-pressed`/tooltips for icon-only buttons.
5. **Respect the viewport — both axes.** Horizontally: never `fitContent()` on data
   refresh; capture/restore via `chartViewport.ts`. Vertically: never call
   `rearmDualAutoScale` from an effect whose deps include trade/bar data — go through the
   sync handle's `rearm()` (see §10b), and reserve `reset()` for changes to what an axis
   means.
6. **Honor metric/unit** — route every displayed price through the metric converter and
   `createChartPriceFormatter(priceUnit)`.
