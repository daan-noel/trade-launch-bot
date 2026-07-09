# Token History Chart — Functionalities Reference

> A feature-by-feature map of the token price/history chart: **what each control,
> overlay, marker, and interaction does**, how it's triggered, and where it lives in
> code. This is the *functionality* companion to
> [`chart-price-logic.md`](./chart-price-logic.md) (which covers the OHLC/price math)
> and [`swing-detection-logic.md`](./swing-detection-logic.md) (the swing algorithm).
> Reuse this file as a prompt to extend or re-implement the chart UI.
>
> **Key files** (all under `frontend-react/src/components/token-price-chart/`)
>
> - Main component: [`TokenPriceChart.tsx`](../../frontend-react/src/components/token-price-chart/TokenPriceChart.tsx)
> - Toolbar: [`ChartToolbar.tsx`](../../frontend-react/src/components/token-price-chart/ChartToolbar.tsx)
> - Bottom zoom/pan slider: [`ChartRangeSlider.tsx`](../../frontend-react/src/components/token-price-chart/ChartRangeSlider.tsx)
> - Canvas plugins: [`rangeSelectPlugin.ts`](../../frontend-react/src/components/token-price-chart/rangeSelectPlugin.ts), [`walletMarkersPlugin.ts`](../../frontend-react/src/components/token-price-chart/walletMarkersPlugin.ts), [`chainHighlightPlugin.ts`](../../frontend-react/src/components/token-price-chart/chainHighlightPlugin.ts)
> - Swing overlay geometry: [`swingOverlay.ts`](../../frontend-react/src/components/token-price-chart/swingOverlay.ts)
> - Tooltips: `BarCrosshairTooltip.tsx`, `SwingCrosshairTooltip.tsx`, `ChainHighlightTooltip.tsx`, `WalletMarkersTooltip.tsx`, `RangeSelectTooltip.tsx`, field renderers `BarCrosshairFields.tsx` / `BarFlowFields.tsx`
> - Viewport & time helpers: [`chartViewport.ts`](../../frontend-react/src/components/token-price-chart/chartViewport.ts), [`chartTimezone.ts`](../../frontend-react/src/components/token-price-chart/chartTimezone.ts)
> - Bar math: [`chartBars.ts`](../../frontend-react/src/components/token-price-chart/chartBars.ts) — see `chart-price-logic.md`
> - Shared types / constants: [`types.ts`](../../frontend-react/src/components/token-price-chart/types.ts), [`constants.ts`](../../frontend-react/src/components/token-price-chart/constants.ts)

---

## 1. Overview

`TokenPriceChart` renders a single token's trade history as a candlestick/line chart
built on [`lightweight-charts`](https://github.com/tradingview/lightweight-charts). It
takes a flat list of `Trade` rows (`trades` prop) and aggregates them **client-side** into
OHLC bars (no server-side candles). On top of the base series it layers several optional,
independently-toggleable features:

- **Trade-count markers** (per-bar buy/sell arrows)
- **Wallet markers** (per-tracked-wallet circles)
- **Swing overlay** (the swing-detection path) + **chain highlight** band
- **ATH / Migration** reference price lines
- **Range-select** mode (drag to summarize a time window)
- A bottom **range slider** for zoom/pan

Most boolean toggles are **persisted to `localStorage`** so the user's chart layout sticks
across reloads; a few (swing/chain/range modes, selections) are session-only UI state.

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
  reconstruction — dust filtering) is documented in `chart-price-logic.md` §3–§6.

---

## 3. Series style & metric

### 3a. Chart style (`style`: `'candles'` | `'line'`)

Toggled by the candle/line icon group in the toolbar (`handleStyleChange`, persisted).

- **Candles** (default): `CandlestickSeries` with `CANDLE_SERIES_OPTIONS`. Selected /
  swing-selected bars are repainted (filled highlight) via `barsToCandleData(bars, highlightBarTimes)`.
- **Line**: `LineSeries` (`LINE_SERIES_OPTIONS`) drawing the close price as a continuous teal line.

### 3b. Metric (`metric`: `'price'` | `'mc'`)

Only rendered when the parent passes an `onMetricChange` callback.

- **Price** — spot SOL/token.
- **MC** — market cap = `TOKEN_TOTAL_SUPPLY × spot`.

The metric is a **parent-controlled** prop (the chart calls `onMetricChange`), and it rescales
the Y-axis, the swing-overlay prices, and every price formatter.

### 3c. Price unit & formatting

`priceUnit` (`'SOL'` | `'USD'`) + `toValue()` converter + `priceLabel` come from the parent.
`createChartPriceFormatter(priceUnit)` (`constants.ts`) prefixes values with **◎** (SOL) or
**$** (USD) and is applied to right-axis labels and all tooltips.

---

## 4. Toolbar controls (`ChartToolbar.tsx`)

The toolbar has two rows. **Row 1**: title + status badges + live crosshair readout, then the
pill groups (group mode, interval, style, metric) and the marker/line toggles. **Row 2**
(right-aligned): the swing/chain/range controls.

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
| **Swings** | checkbox | swing-detection overlay (§8); disabled unless `swingOverlayAvailable` | session |
| **Connect** | icon button | connected vs. per-leg swing segments (§8); needs Swings on | parent/session |
| **Chain link** | icon toggle | longest-chain highlight band (§9); disabled unless `chainHighlightAvailable` | session |
| **Range select** | icon toggle | drag-to-select range mode (§6) | session |

**Status badges** (only shown when `isMigrated != null`): `Migrated ✓` / `Bonding Curve`,
plus optional `Mayhem` and `Cashback` badges, colored per `STATUS_BADGE_COLOR`.

**Live crosshair readout**: an `aria-live="polite"` line under the title showing the hovered
bar's O/H/L/C (candles) or price (line) + Vol/Liq, rendered by `BarCrosshairFields` in
`layout="inline"`. It mirrors the floating bar tooltip (§11) so the values are always visible
even when the pointer is deep in the chart.

Icon-only controls each have an instant dark `HoverTooltip` because their label lives only in
the tooltip; toggles expose `aria-pressed` for accessibility.

---

## 5. Trade-count markers (buy/sell per bar)

When **Buy/sell counts** is on (`showTradeMarkers`, default on), `buildTradeMarkers` emits a
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

## 7. Reference price lines (ATH & Migration)

Both are drawn with `series.createPriceLine()` and respect the active metric/unit:

- **ATH** (`showAthLine`): dashed golden (`#f0b429`) line at the all-time-high price, computed
  from `athPriceInSol` through the metric/unit converter (`athChartValue`). The checkbox is
  **disabled** (`athLineAvailable === false`) when the token has no recorded ATH. ATH itself is
  authoritative backend data (see `chart-price-logic.md` §8–§9), not recomputed here.
- **Migration** (`showMigrationLine`): dashed teal-blue (`#5dade2`) line at the fixed pump.fun
  bonding-curve graduation price `PUMP_MIGRATION_SPOT_PRICE_SOL` (`constants.ts`) — a constant,
  not token-specific.

---

## 8. Swing overlay

When **Swings** is on (`showSwingOverlay`, available only when the parent supplies a
`swingOverlay` prop), the detected swing legs are drawn as colored line series on top of the
price chart. Geometry comes from `swingOverlay.ts`; the algorithm that produces the legs is
`swing-detection-logic.md`.

### 8a. Segment modes (`segmentMode`)

Driven by the **Connect** button (`connectSwings`) and whether a visibility filter is active:

- **`connected`** — one continuous reversal path (first leg's start, then each leg's end),
  colored per leg. Default when "connect swings" is on.
- **`perLeg`** — each leg as an isolated start→end segment. Used when connect is off.
- **`connectedSequential`** — connected **only within runs of legs that are adjacent in the
  full ledger** (`groupSequentialLegChains`), so a visibility-filtered gap is not bridged with
  a false line.

### 8b. Colors & series

Swing-high = cyan (`#0eb5ff`), swing-low = magenta (`#e879f9`), 3 px lines with no crosshair
markers / price line. Each leg's stable key is
`swingLegKey(leg) = \`${type}-${start_at}-${end_at}\``.

### 8c. Selecting a leg

Clicking the swing path resolves the leg under the pointer
(`resolveSwingLegAtChartInteraction`) and toggles selection via `onSwingLegClick` /
`selectedSwingLegKey`. The selected leg's bars are highlighted on the main series.

### 8d. Leg tooltip

Hovering the swing path shows `SwingCrosshairTooltip`: a colored badge (`SWING HIGH` / `SWING
LOW`) + duration, start/end time & price, Δ%, inflow/outflow/net flow, and trade count.

---

## 9. Chain highlight (longest swing chain)

When **Chain link** is on (`showChainHighlight`, available only when the parent passes a
`highlightChain`), `ChainHighlightPlugin` (`chainHighlightPlugin.ts`) paints a full-height
**amber translucent band** across the longest swing chain's span, with solid amber edges and a
top-centered chip label (e.g. `Longest chain · N pairs`). A swing "chain" is defined in
`swing-detection-logic.md` §9b.

Hovering the chip (hit-tested via `containsLabelPoint`) opens `ChainHighlightTooltip`: pair
count, in/out/net flow, price Δ + %, and trade counts. The in-band trade counts are computed by
`computeChainTradeCounts` (trades whose `block_time` falls within the chain's `[startAt,
endAt]`).

---

## 10. Wallet markers (tracked profile wallets)

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

## 11. Crosshair tooltips & priority

Hovering the chart can surface one of several floating tooltips. On every crosshair-move the
component decides **which single tooltip to show** (others are cleared), roughly in this
priority:

1. **Chain label** hovered → `ChainHighlightTooltip` (§9)
2. **Range label** hovered → `RangeSelectTooltip` (§6)
3. **Wallet marker** hovered → `WalletMarkersTooltip` (§10)
4. **Swing path** hovered → `SwingCrosshairTooltip` (§8d)
5. Otherwise, over the main series → **bar tooltip** `BarCrosshairTooltip`

The **bar tooltip** shows the bar time (timezone/slot-formatted) + age, then fields from
`BarCrosshairFields`: for candles a 2×2 **O/H/L/C** grid (colors from `CHART_OHLC_COLORS`) plus
Vol/Liq; for line, Price + Vol/Liq. `BarFlowFields` renders the flow variant (Net / In / Out /
Δ%) used for the per-bar volume readout (see `chart-price-logic.md` §7).

---

## 12. Zoom, pan & the bottom range slider

### 12a. Viewport preservation (`chartViewport.ts`)

Because bars are rebuilt whenever interval/group/metric/trades change, the chart must not
"jump" back to fit-content on every update. The helpers:

- `captureChartViewport` snapshots the current logical/visible range.
- `restoreChartViewport` / `reapplyChartViewport` re-apply it (the latter double-applies across
  a `requestAnimationFrame` because lightweight-charts re-lays-out on the next frame).
- `barsSignature` (`"count:firstTime:lastTime"`) detects when the bar array's **shape** changed;
  on a shape change the viewport is restored by **time** rather than logical index so it lands
  correctly. On first mount the chart `fitContent()`s once, then preserves the user's view.

### 12b. Bottom range slider (`ChartRangeSlider.tsx`)

Shown when there is more than one bar. It's a miniature scrollbar over the full data span with
a teal "window" marking the visible range. Three drag modes:

- **left handle** (`from`) / **right handle** (`to`) — resize the visible window edge
- **middle** (`pan`) — slide the window

It enforces a minimum window (`MIN_WINDOW_RATIO`) and calls
`chart.timeScale().setVisibleRange(from, to)` on change; conversely it syncs back from the
chart's visible range so dragging on the chart updates the slider.

---

## 13. Timezone & time formatting (`chartTimezone.ts`)

Time-mode axis labels and tooltips are timezone-aware via a `useTimezone()` context.
`createChartTimeFormatters(timezone)` builds:

- `timeFormatter` — full `YYYY-MM-DD HH:mm:ss` for the crosshair/tooltip,
- `tickMarkFormatter` — compact `MMM D HH:mm` for axis ticks,

both backed by `Intl.DateTimeFormat(undefined, { timeZone })`. In slot mode times are rendered
as plain `Slot N` strings instead.

---

## 14. Props & state quick reference

**Notable props** (`TokenPriceChartProps` in `types.ts`): `trades`, `loading`/`error`,
`toValue`/`priceUnit`/`priceLabel`, `metric` + `onMetricChange`, `height`, `onBarClick` /
`selectedBar`, `onRangeChange`, `swingOverlay` / `highlightChain` / `selectedSwingLegKey` /
`onSwingLegClick` / `connectSwings` / `onConnectSwingsChange`, `athPriceInSol`,
`isMigrated` / `isMayhemMode` / `isCashbackEnabled`, `profileWallets`, `tokenCreatedAt`,
`eventMarkers`.

**Persisted (localStorage) state**: `groupMode`, `interval`, `style`, `showTradeMarkers`,
`showAthLine`, `showMigrationLine`, `trimEmptyBars`.

**Session-only UI state**: `showSwingOverlay`, `showChainHighlight`, `connectSwings`,
`rangeSelectMode`, `selectedRange`, `selectedBar`, and the hover/tooltip states
(`crosshair`, `barTooltip`, `swingTooltip`, `chainTooltip`, `rangeTooltip`,
`walletMarkersTooltip`), plus `sliderWindow`.

---

## 15. Extending the chart (checklist)

1. **New per-bar marker** → build a `SeriesMarker[]` like `buildTradeMarkers`, or a canvas
   `ISeriesPrimitive` plugin (as `walletMarkersPlugin.ts`) when you need custom geometry/hit-testing.
2. **New overlay line** → create a `LineSeries`, feed it `{ time, value, color? }` points, and
   key it so the recreate-effect can diff it (mirror `swingOverlay.ts`).
3. **New full-height band** → follow `chainHighlightPlugin.ts` / `rangeSelectPlugin.ts`
   (`ISeriesPrimitive` paint + a hit-testable label chip + a React tooltip).
4. **New toggle** → add a control in `ChartToolbar.tsx`, thread the prop/handler, and decide
   persisted (localStorage) vs. session state. Keep `aria-pressed`/tooltips for icon-only buttons.
5. **Respect the viewport** — never `fitContent()` on data refresh; capture/restore via
   `chartViewport.ts` so the user's zoom/pan survives.
6. **Honor metric/unit** — route every displayed price through the metric converter and
   `createChartPriceFormatter(priceUnit)`.
