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
> - Tooltips: `BarCrosshairTooltip.tsx`, `LensLaneTooltip.tsx`, `WalletMarkersTooltip.tsx`, `RangeSelectTooltip.tsx`, field renderers `BarCrosshairFields.tsx` / `BarFlowFields.tsx`
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
| `showWalletMarkers`, `showEventMarkers`, `showAthLine`, `showMigrationLine`, `showFlowTagged`, `showFlowUntagged` | on | read every time; the toolbar disables each when its data is absent, so they cost nothing |
| `showTradeMarkers` | **off** | the per-bar buy/sell count badge is one badge per candle at `1s` — it hides the price action it annotates |
| `trimEmptyBars` | **off** | no-trade gaps ARE information (a stalled token); dropping them distorts the time axis |

Both apps share this default — it is not split per app. Flow Discovery uses the same
`TokenTradeChart` host and the same prefs key.

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

### 6b. Adding a trade to a fingerprint tag from the trades table

The panel's **`@tag`** column is the editing control for one tag of the target
fingerprint's `tags` document: the badge says which half of the split the chart put the
row on (`@tag`, `@!tag`, or `neither` for a creation-slot buyer the tag excludes), and a
click adds that row's value to the tag - or removes it when listed - and **saves
immediately**. The value is the row's exact ix shape (plus any fee fields pinned in the
strip), its ix template, its program or its wallet, whichever matcher the strip
(`IxPatternBar`, `TagStageControls`) has selected; a tag name the fingerprint does not
define yet is created by the first click. There is no staging step on a fingerprint - a
draft copy would be a second answer to "what carries the tag", and the surfaces reading
the two copies then disagree on screen while both look authoritative. The write
invalidates the `Fingerprint` cache tag, so the chart lines, the metric panes and the
badge all redraw from the row that was just written; the engine picks it up on its next
rules reload. `withStageValue` (`hooks/useIxPatternTarget.ts`, through the tags
document's writers `withTagShape` / `withTagListValue` in `lib/strategy/tagsDoc.ts`) is
the ONE write every add-to-tag click makes; Flow Discovery's draft tape stages into its
own draft instead, saved by Apply.

**Which row it writes to is `useIxPatternTarget`, and it is never guessed while a fact
is available.** `resolveIxPatternTarget` ranks: an explicit pick from the bar's select,
then the host's own `flowFingerprintId`, then a lone shape-set match. The order is the
whole point. Matching by SET cannot identify a row - `tags` is not part of fingerprint
match identity, so any number of rows may carry the same shapes, and every row without
shapes carries the same empty set, which is exactly the state authoring starts
from. A set-first resolver therefore fails precisely when the feature is first used: the
badge goes dead when several rows match, and writes to whichever unrelated row happens to be
the only empty one when just one does. Hence hosts pass `flowFingerprintId` alongside
`flowPatternKeys` all the way down (`hooks/useFlowPatternKeys` resolves both as one
`FlowPatternSource`), a match is taken only when exactly one row carries the set and is
labelled `matched by patterns — confirm`, and picking away from the host is labelled too,
since the badges then answer for a different row than the lines above them.

Three further rules the surface exists to enforce:

- **The badge says why.** The verdict comes from the same classification the lines draw
  from (`tradeFlowReasons`, `useFlowReasons`), run over the host's **full** history - a
  `sticky` wallet, a `cluster` and the creation slot are forward-only, so a single bar's
  rows cannot reconstruct them - and the cell appends `via <matcher>`, the registry's
  name for the matcher that held (`via creator`). Without that marker a click that "does nothing" (the row already
  carries the tag through another matcher) looks like a bug; a listed value reads
  `listed`.
- **The first matcher reveals the overlay.** `flowLinesAvailable` is false until the tag
  can classify, and the per-curve flags are persisted prefs - so the chart auto-enables
  BOTH lines on the transition to classifiable. Turning them back off stays the user's call.
- **A run snapshot is not editable.** `flowReadOnly` marks a subtree whose tags are a
  stored fact - the grouped-sweep drill-in, whose numbers were computed under the run's own
  `tags`. It shows `run snapshot` instead of the edit control and skips the
  fingerprint/rule fetches entirely.

`IxPatternBar` states the target fingerprint, the tag and how many **active** rules use it
before any click. That count is the whole warning: `tags` is not part of fingerprint match
identity, so a write does not fork the row - it lands on the same id and every rule bound
to it starts reading that tag differently.

### 6c. Highlight lenses (where did these wallets / these ix structures appear)

**Ephemeral** lenses over one token's history: *when did these wallets trade* and *when did
these exact ordered ix structures appear*. Up to four wallets and four structures are armed at
once, each in its own color. Each is armed from the trades table's target button (the Wallet
cell for a wallet, the `ix_labels` cell for a structure) and disarmed by the same button or its
chip's `x`.

| Piece | Where | Job |
| --- | --- | --- |
| `useTokenHighlight` | `components/tokens/` | holds the armed items and their colors, disarms all when the mint changes, exposes `colorsOf` / `walletColor` / `structureColor` |
| `buildLensMatch` | `token-price-chart/lensTint.ts` | buckets one item's matched trades against the bars already drawn, keeping each bar's matched trades |
| `LensLanePlugin` | `token-price-chart/lensLanePlugin.ts` | the highlight lane under the candles, row names in the price-axis gutter |
| `BarTintPlugin` | `token-price-chart/barTintPlugin.ts` | faint washes behind the candles, `zOrder: 'bottom'` |
| `LensChips` | inside `BarTradesPanel` | states what is armed, what it matched, the size-label switch, and turns items off |

**The lane is where a highlight is found.** Under the candles, one row per armed item
(wallets first, then structures, each in arming order), with a solid mark on every bar the
item appears in, so where it shows up across the whole token reads at a glance. A row with
no marks means the item never appeared on this token, which is why every armed item keeps
its row. The row's name sits in the price-axis gutter, level with the row. The host reserves
the lane's height (`lensLaneHeight`) through the price scale's bottom margin, so no candle or
flow line is ever drawn under a row, and the condition time bands (`timeBandsPlugin`) move up over it. Rows thin down
when many are armed and the lane never takes more than 40 % of the pane. The range slider
under the chart repeats the rows as thin bands of ticks over the whole token, so the overview
survives zooming in.

**Colors come in two families** (`LENS_COLORS`): wallets warm (gold, orange, lime, pink),
structures cool (cyan, blue, violet, green). Each hue is a clear step from the next and the
lane draws them solid, so two items read apart without a legend. An item takes the first free
slot of its family when armed and keeps it until disarmed, so arming or disarming one never
repaints the others. Arming a fifth of one kind drops the oldest of that kind. The row
backgrounds split the same way (`LENS_TRACK_COLORS`): a warm tint behind wallet rows and a
cool one behind structure rows, in the lane and behind the row names, so which rows are
wallets reads before any mark or label.

**An armed wallet is also a diamond on the marker layer** (section 8): the lane says when, the
diamond says which leg and which side. It is drawn in its lens color, the same as its lane row.

**The wash behind the candles is faint** (alpha 0.06 to 0.22) and stops at the lane's top: it only ties a lane mark to its candle.
It is share-weighted, never binary: its alpha tracks matched SOL over `OhlcBar.volume`, so
one dust leg in a busy slot renders faintest and a slot the target owns renders strongest. A binary tint would overstate every bar it paints, and in time mode — where a
60s candle holds many wallets — it would be actively misleading. `buildLensMatch` therefore
mirrors `collectTradeBuckets`' dust and validity guards exactly: counting a trade the bar
itself dropped puts the share above 1 on a candle that never held it.

**Items cannot share a channel.** A candlestick carries exactly one `borderColor`, so the
plugin splits the bar slot among the items that hit THAT bar, in arming order — full width when
one item owns the bar, however many are armed. The overlap is the cell a reader is hunting for,
so it must stay visible rather than resolve to whichever layer draws last. The table follows the
same colors: a matched row takes the background and left edge of the first item it matches
(`--row-lens`, set through `DataTable`'s `rowStyle`), and each armed target button lights in its
item's color. The page's own `highlightWallet` keeps the focus gold and outranks a lens. Every
armed wallet also gets the spotlit marker treatment; one with no profile entry gets a synthetic
marker in its lens color.

**Sizes live in the lane.** A mark's height grows with the square root of the SOL it moved in
that bar, on one scale for every row (the biggest bar of any row over the whole token), so
the same SOL draws the same height in any row, the big hits stand out, and a pan never changes
a mark's height. With sizes on, the number sits beside its mark, horizontal
(`laneText`): one trade prints its SOL, several print the sum and count, `0.700 (2)`. The chip
strip's switch picks `buys` (default), `all` (`+buy −sell`) or `off`; the choice persists in
`UiToggles.lensSizeLabels`. A number sits in the gap between its mark and the next one and is
dropped when it does not fit, since a number drawn across another mark cannot be read; an
`N sizes hidden · zoom in` hint counts the dropped ones. Hovering a lane mark opens
`LensLaneTooltip` with that row's trades in that bar and nothing else: side, SOL, network fee
(`fee_sol`) and wallet, largest first. Another row's trades in the same bar show on its own
mark, and the candle's numbers show over the candles (section 9). Lane, wash and lane tooltip all
read the same `LensBarTint.trades`, so they cannot disagree.

**The counts on the chips come from the chart**, via `onHighlightLensMatch` — not from a
second pass over the rows. They are bar-aligned by construction, so a chip can never quote a
number the wash beside it disagrees with. With a structure armed, the strip also reports the token's
**unlabeled** trades: a structure lens can say nothing about a row whose `instruction_labels`
were never captured, and `0 matches` over a pile of them means "not recorded", not "unique".

**A lens is not the `@tag` badge, deliberately.** 6b's badge writes a fingerprint tag and
the engine acts on it; a lens writes nothing and no rule reads it. They sit one column apart on
the same row and answer questions that differ only in wording, so the separation is the
feature: asking *where else did this shape appear* must not change how a live rule classifies
flow. The identity is shared, though — both match on `patternKey`, ordered and exact, so "the
same structure" means one thing across the app.

The chip strip renders even with **no candle selected**, which is the only reason the panel
draws at all in that state: the control that disarms a lens must not hide behind the table it
is washing.

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
- The silhouette carries the wallet's class (`walletShape`): arrow = `mine` (up on a buy, down on a sell, no letter), triangle = dev,
  hexagon = the page's focused wallet (largest, gold glow and ring), diamond = a wallet armed as a
  highlight lens (6c; ~1.8x, its lens color, no glow or ring, so it stays below the focus),
  square = the comparison set, circle = everyone else. Nearest the bar stack the focus, then
  lens diamonds, then the comparison set, then the rest.
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
2. **Highlight lane** hovered → `LensLaneTooltip` on a mark (the row's trades in that bar, §6c),
   nothing on an empty spot; the lane strip never shows the bar tooltip
   (`LensLanePlugin.containsY` / `hitAt`)
3. **Wallet marker** hovered → `WalletMarkersTooltip` (§8)
4. Otherwise, over the main series → **bar tooltip** `BarCrosshairTooltip`, the candle's own
   numbers only

The **bar tooltip** and the toolbar readout carry **disjoint** facts — never the same ones
twice, since both are on screen simultaneously:

- **Toolbar readout** (`BarCrosshairFields`, `layout="inline"`) = the *price* view: for candles
  **O/H/L/C** (colors from `CHART_OHLC_COLORS`) plus Vol/Liq; for line, Price + Vol/Liq. Plus
  the cumulative `@tag` / `@!tag` pair when flow lines are *available* - both values, whichever
  curves are drawn, with a hidden curve's value dimmed. The numbers cost nothing to read and
  losing one on toggle-off is the annoying part.
- **Bar tooltip** (`BarCrosshairTooltip`) = what the toolbar *cannot* say — **which** bar is
  hovered (timezone/slot-formatted bar time + `+age` since token creation) and its per-bar
  **order flow** via `BarFlowFields`: Net / In / Out / Δ%, then `@tag∑net` / `@!tag∑net`.

A chart that repeats the O/H/L/C block inside its own tooltip is the bug.

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

### 10b. Vertical (price) scale - one axis for every series

The chart has **one** price axis (right). The candles and the `@tag` / `@!tag` flow
lines all sit on it, so one zoom moves every series and no line can drift off the
candles' scale.

A flow line is drawn at its **cohort curve price** (`cohortCurvePriceSol`,
`lib/flow/flowChartData.ts`): the spot the bonding curve would sit at if, from the chart's
first trade, only that cohort had traded. On the curve price is `vsol / vtoken` with
`vsol * vtoken = k`, so a SOL net moves the SOL reserve (`(anchor.sol + netSol)^2 / k`)
and a token net moves the token reserve (`k / (anchor.token - netToken)^2`; `value_sol`
maps through its token count). `amount_sol` is the curve-side leg, so the SOL mapping is
exact. The anchor is the curve before the chart's first trade (`preTradeReserves`), so
net 0 draws on the first candle's open. Example: anchor 30 SOL, a cohort net +10 SOL draws
at (40/30)^2 = 1.78x the start price. The point then goes through `priceSolChartValue`,
the same price/MC + unit conversion the bars use. A line's axis label is therefore a price
in the candles' unit; the cohort's net itself is in the toolbar readout and the bar
tooltip (section 9).

The lines live on the **curve only**: they end at the first AMM trade (`FlowLines.endTime`),
because the curve mapping does not hold on a pool.

Hiding a line only hides it (`flowLineVisibility.ts`). Autoscale fits every visible series,
so when a hidden line was the tallest the axis refits - candles and the other line together,
never one relative to the other.

Y zoom is lightweight-charts' own: an axis drag turns that axis' `autoScale` off and it stays
off through live trades until the axis is double-clicked. The chart re-arms `autoScale` only
when the axis means something else - a change of token, grouping, unit, metric or style
(`TokenPriceChart`'s refit effect). Never on a data update or a line toggle.

The axis keeps `PRICE_SCALE_MARGINS` (10 % top and bottom) plus, while highlights are armed,
the highlight lane's height (6c) added to the bottom margin. It is set again whenever the lane's
row count, the pane's height or the chart itself changes.

### 10c. Bottom range slider (`ChartRangeSlider.tsx`)

Shown when there is more than one bar. It's a miniature scrollbar over the full data span with
a teal "window" marking the visible range. Three drag modes:

- **left handle** (`from`) / **right handle** (`to`) — resize the visible window edge
- **middle** (`pan`) — slide the window

It enforces a minimum window (`MIN_WINDOW_RATIO`) and calls
`chart.timeScale().setVisibleRange(from, to)` on change; conversely it syncs back from the
chart's visible range so dragging on the chart updates the slider.

While highlights are armed (6c) the track also carries the highlight lane's rows as thin bands
of colored ticks over the full span (`marks`), so where each highlight sits in the whole token
stays visible while the chart is zoomed in. The track grows with the rows (16 px, or 7 px per row when
that is taller), so every row keeps a readable band.

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
`showDevMarkers`, `devMarkersBoundariesOnly`, `showEventMarkers`, `showFlowVol`, `showFlowNonVol`.

**Session-only UI state**: `rangeSelectMode`, `selectedRange`, `selectedBar`, and the
hover/tooltip states (`crosshair`, `barTooltip`, `rangeTooltip`,
`walletMarkersTooltip`, `laneTooltip`), plus `sliderWindow`.

---

## 13. Extending the chart (checklist)

1. **New per-bar marker** → build a `SeriesMarker[]` like `buildTradeMarkers`, or a canvas
   `ISeriesPrimitive` plugin (as `walletMarkersPlugin.ts`) when you need custom geometry/hit-testing.
2. **New overlay line** → create a `LineSeries` on the one price axis, feed it
   `{ time, value, color? }` points converted through `priceSolChartValue`, and key it so the
   recreate-effect can diff it. A quantity that is not a price needs its mapping onto price
   first (as `cohortCurvePriceSol`), never a second axis.
3. **New full-height band** → follow `rangeSelectPlugin.ts`
   (`ISeriesPrimitive` paint + a hit-testable label chip + a React tooltip).
4. **New toggle** → add a control in `ChartToolbar.tsx`, thread the prop/handler, and decide
   persisted (localStorage) vs. session state. Keep `aria-pressed`/tooltips for icon-only buttons.
5. **Respect the viewport — both axes.** Horizontally: never `fitContent()` on data
   refresh; capture/restore via `chartViewport.ts`. Vertically: never re-arm `autoScale`
   from an effect whose deps include trade/bar data - only when what the axis means changes
   (see §10b).
6. **Honor metric/unit** — route every displayed price through the metric converter and
   `createChartPriceFormatter(priceUnit)`.
