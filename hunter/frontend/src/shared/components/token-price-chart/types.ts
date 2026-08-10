import type { UTCTimestamp } from 'lightweight-charts';
import type { PriceUnit } from 'types';

/** Minimal trade shape required by the chart — copy this folder as-is into other projects. */
export interface ChartTrade {
  block_time: string;
  price_per_token: number;
  trade_type: 'buy' | 'sell';
  amount_sol?: number;
  /** Raw token units swapped — used to reconstruct the pre-trade (genesis) open. */
  token_amount?: number;
  slot?: number;
  /** Position of this trade's transaction within its block — the real intra-slot
   *  ordering key. Part of the canonical trade order `slot → tx_index → leg_index`. */
  tx_index?: number;
  /** Transaction signature — chronological tiebreaker (keeps a tx's legs contiguous). */
  tx_signature?: string;
  /** Leg index within the transaction (0 = first) — chronological tiebreaker. */
  leg_index?: number;
  /** SOL side of the reserve pair this row prices from (curve virtual reserves on
   *  curve rows, pool real reserves on amm rows). Spot = reserve_sol / reserve_token. */
  reserve_sol?: number | null;
  reserve_token?: number | null;
  real_reserve_sol?: number | null;
  real_token_reserves?: number | null;
  venue?: 'curve' | 'amm';
  wallet_address?: string;
  /** Ordered instruction labels — drives vol/non-vol flow classification. */
  instruction_labels?: string[] | null;
}

/** A tag attached to a wallet's owning profile. */
export interface ProfileTagInfo {
  name: string;
  color: string;
}

/** A tracked profile wallet to render as chart markers. */
export interface ProfileWalletInfo {
  address: string;
  label?: string;
  color: string;
  profileName?: string;
  tags?: ProfileTagInfo[];
  /** True for wallets on the user's own `mine`-type profile — gets a fixed
   *  marker color/glyph instead of the rotating tracked-wallet palette, and
   *  drives the "my trade" row highlight in trade tables. */
  isMine?: boolean;
  /** True for the single wallet the current view is focused on (e.g. the input
   *  wallet on the Trader Analysis page) — its markers render larger with a
   *  glow + gold outer ring so they stand out among the other tracked wallets. */
  isHighlighted?: boolean;
  /** True for the token's dev/creator wallet — gets its own triangle silhouette
   *  + a fixed static color (`CHART_COLORS.dev`) so dev entries/exits (first_buy,
   *  sell_all) are unmistakable and never collide with the `mine` diamond. */
  isDev?: boolean;
}

/**
 * A strategy entry/exit point to overlay on the chart: an arrow marker pinned to
 * the matching bar plus a dashed horizontal line at the fill price. Used by the
 * TPSL paper/simulation/position result inspector.
 */
export interface ChartEventMarker {
  kind: 'entry' | 'exit';
  /** ISO block time of the entry/exit trade — bucketed to a bar. */
  time: string;
  /** Spot price in SOL at the fill — drives the dashed price line. */
  priceInSol: number;
  /** Tx signature; pins the marker to that trade's exact bar when present in `trades`. */
  txSignature?: string | null;
  /** Short label drawn on the marker / axis, e.g. "Entry" or "Exit · TP". */
  label?: string;
  /**
   * Axis title for this fill's dashed price line. Defaults to `Entry` / `Exit`.
   * A scale-out ladder sets it per leg (`Exit 50%`) — otherwise every leg draws an
   * identically-titled line and the axis reads as one exit repeated.
   */
  lineLabel?: string;
  /**
   * `fill` (default) = backend run/position entry·exit result.
   * `signal` = frontend-computed first metric fire (inspect metric panes).
   * Kept visually distinct so the two never read as the same pointer.
   */
  role?: 'fill' | 'signal';
}

export type ChartGroupMode = 'time' | 'slot';

export type ChartMetric = 'price' | 'mc';

export interface OhlcBar {
  time: UTCTimestamp;
  open: number;
  high: number;
  low: number;
  close: number;
  volume: number;
  /** Buy-side SOL volume in this bar (SOL into the curve/pool). */
  inflow: number;
  /** Sell-side SOL volume in this bar (SOL out of the curve/pool). */
  outflow: number;
  /** Bonding-curve liquidity (SOL) after the last trade in this bar. */
  liquiditySol: number | null;
}

export type ChartInterval = '1s' | '30s' | '1m' | '5m';

export type ChartStyle = 'line' | 'candles';

export interface ChartCrosshairInfo {
  open: number;
  high: number;
  low: number;
  close: number;
  volume: number;
  /** Buy-side SOL volume in the hovered bar. */
  inflow: number;
  /** Sell-side SOL volume in the hovered bar. */
  outflow: number;
  liquiditySol: number | null;
  /** Cumulative volume-maker cohort at this bar (null when flow overlay has no point). */
  flowVol: number | null;
  /** Cumulative non-volume cohort at this bar (null when flow overlay has no point). */
  flowNonVol: number | null;
}

export interface ChartBarSelection {
  barTime: UTCTimestamp;
  groupMode: ChartGroupMode;
  /** Set when groupMode is `time`. */
  intervalSec?: number;
  /** Set when groupMode is `slot`. */
  slot?: number;
}

/** Aggregate stats for a user-drawn time range (snapped to bar bounds). */
export interface ChartRangeStats {
  /** Buy-side SOL flow over the range. */
  inflow: number;
  /** Sell-side SOL flow over the range. */
  outflow: number;
  /** inflow − outflow. */
  netFlow: number;
  tradeCount: number;
  buyCount: number;
  sellCount: number;
  /** Distinct wallets that traded in the range. */
  uniqueWallets: number;
  uniqueBuyers: number;
  uniqueSellers: number;
  /** Largest single buy `amount_sol` in the range (0 if none). */
  maxBuySol: number;
  /** Largest single sell `amount_sol` in the range (0 if none). */
  maxSellSol: number;
  /** Wall-clock span from first to last trade in the range (ms). */
  durationMs: number;
  /** Price change (priceInSol) across the range: last − first trade price. */
  priceDelta: number;
  /** `priceDelta` as a percentage of the first trade price; null if unknown. */
  priceDeltaPct: number | null;
}

/** User-drawn analysis range, in chart-time units (bucket-start sec, or slot). */
export interface ChartRangeSelection {
  lo: number;
  hi: number;
}

/** A committed range selection plus the grouping context needed to map it back
 *  to trades: `lo`/`hi` are bucket-start seconds in time mode, slot numbers in
 *  slot mode. Emitted by {@link TokenPriceChartProps.onRangeChange}. */
export interface ChartRangeSelectionDetail extends ChartRangeSelection {
  groupMode: ChartGroupMode;
  /** Bar interval (seconds) when `groupMode` is `time`; unused in slot mode. */
  intervalSec: number;
}

/** Visible wall-clock window from the price chart (unix seconds). */
export interface ChartVisibleTimeRange {
  from: number;
  to: number;
}

/** A closed wall-clock interval (unix seconds), `from <= to`. */
export interface ChartTimeSpan {
  from: number;
  to: number;
}

/**
 * One bottom-pane lane: the stretches over which some named thing was true.
 *
 * Kept vocabulary-free on purpose — the chart is shared by both apps and has no
 * business knowing that the live app draws rule conditions here.
 */
export interface ChartTimeBand {
  key: string;
  /** Drawn at the left edge of the lane; keep it short, it renders at 8px. */
  label: string;
  /** CSS color for the satisfied stretches. */
  color: string;
  spans: ChartTimeSpan[];
}

/** Tooltip shown when the crosshair hovers the range-selection label chip. */
export interface ChartRangeTooltipState {
  stats: ChartRangeStats;
  point: { x: number; y: number };
}

export interface WalletBarActivity {
  wallet: ProfileWalletInfo;
  buyCount: number;
  sellCount: number;
  buySol: number;
  sellSol: number;
}

export interface ChartWalletMarkersTooltipState {
  point: { x: number; y: number };
  wallets: WalletBarActivity[];
}

export interface ChartBarTooltipState {
  open: number;
  high: number;
  low: number;
  close: number;
  volume: number;
  inflow: number;
  outflow: number;
  liquiditySol: number | null;
  flowVol: number | null;
  flowNonVol: number | null;
  barTime: UTCTimestamp;
  /** Age of the bar's earliest trade since token creation (seconds); null when unknown. */
  ageSec: number | null;
  style: ChartStyle;
  point: { x: number; y: number };
}

/** Toolbar density. `compact` collapses controls behind a Tools toggle — for
 *  narrow hosts (Console manual-trade column) where the full toolbar wraps
 *  taller than the plot. */
export type ChartChrome = 'full' | 'compact';

export interface TokenPriceChartProps {
  symbol: string;
  /** Token id (mint, address, etc.) — empty shows placeholder. */
  id?: string;
  trades: ChartTrade[];
  loading?: boolean;
  error?: string | null;
  /** Convert raw SOL price to display value (e.g. multiply by USD rate). Default: identity. */
  toValue?: (priceInSol: number) => number;
  /** Shown in header, e.g. "SOL" or "USD". */
  priceLabel?: string;
  /** Prefix on right-axis prices: ◎ for SOL, $ for USD. */
  priceUnit?: PriceUnit;
  metric?: ChartMetric;
  onMetricChange?: (metric: ChartMetric) => void;
  className?: string;
  /**
   * `full` (default) — always show the control cluster.
   * `compact` — one thin title row + Tools toggle; expand to the full toolbar.
   * Expand state persists per `id` under `STORAGE_KEYS.chartToolbarOpen`.
   */
  chrome?: ChartChrome;
  /** Fixed pixel height. Omit (the default) to size the height to the chart's
   *  measured width for a readable aspect ratio instead of a wide-flat band. */
  height?: number;
  /** Fired when a chart bar/candle is clicked; null clears selection. */
  onBarClick?: (selection: ChartBarSelection | null) => void;
  /** Highlights the clicked bar/candle on the main series. */
  selectedBar?: ChartBarSelection | null;
  /** Fired when the drag-selected time range changes; null clears it. Carries
   *  the grouping context so the consumer can filter trades to the range. */
  onRangeChange?: (range: ChartRangeSelectionDetail | null) => void;
  /** Wall-clock unix seconds under the crosshair (null when leaving the chart).
   *  Slot-mode charts resolve the hovered bar's last trade time. */
  onCrosshairTimeChange?: (timeSec: number | null) => void;
  /** Drive the chart crosshair from a sibling (e.g. metric panes). Wall-clock
   *  unix seconds; null clears a previously applied external crosshair. */
  externalCrosshairTimeSec?: number | null;
  /** Visible wall-clock window (unix seconds) after pan/zoom. Only emitted in
   *  time-grouping mode (slot mode uses slot indices on the time scale). */
  onVisibleTimeRangeChange?: (range: ChartVisibleTimeRange | null) => void;
  /** On/off lanes drawn along the bottom of the price pane, one row per lane.
   *  Time-grouping mode only — slot mode's scale is slot indices, so wall-clock
   *  spans have nowhere to land. */
  timeBands?: ChartTimeBand[] | null;
  /** The stretch {@link timeBands} speaks for, as a dim track behind every lane.
   *  Without it an unsatisfied lane and an uncovered one look identical. */
  timeBandCoverage?: ChartTimeSpan | null;
  /** Token ATH spot price in SOL (from tokens_info). */
  athPriceInSol?: number | null;
  isMigrated?: boolean;
  isMayhemMode?: boolean;
  isCashbackEnabled?: boolean;
  /** Tracked profile wallets to render as colored buy/sell markers. OMIT to get
   *  the tracked wallets automatically (the chart falls back to `useProfileWallets`
   *  internally) — every token trade chart shows tracked-wallet markers by
   *  default. Pass an explicit list to override (e.g. `TokenTradeChart` adds the
   *  highlighted/synthetic input wallet); pass `[]` to force no markers. */
  profileWallets?: ProfileWalletInfo[];
  /** Token dev/creator wallet address. When set, the toolbar's Dev toggle is
   *  enabled and folds this wallet into the marker pipeline with its own triangle
   *  silhouette + first_buy/sell_all lifecycle markers. Omit/null disables it. */
  creatorWallet?: string | null;
  /** Token creation time (ISO string) — used to show per-bar tx age in the crosshair tooltip. */
  tokenCreatedAt?: string;
  /** Strategy entry/exit points to overlay as arrows + dashed price lines. */
  eventMarkers?: ChartEventMarker[] | null;
  /**
   * `JSON.stringify(labels)` keys of fingerprint `volume_ix_patterns` for the
   * vol/non-vol overlay. Omit/empty is NOT a blank chart — the structural test
   * simply never fires and the split degrades to creator-vs-rest, which the
   * toolbar tooltip names. Only a token with neither patterns nor a creator
   * wallet disables the toggle. (The trades-table Vol badge keeps the stricter
   * gate: it marks a per-trade STRUCTURAL match, which needs patterns.)
   */
  flowPatternKeys?: ReadonlySet<string> | null;
  /** Cumulative flow-line basis (default `cost_sol`). */
  flowBasis?: 'cost_sol' | 'token' | 'value_sol';
}

export interface ChartToolbarProps {
  symbol: string;
  groupMode: ChartGroupMode;
  interval: ChartInterval;
  style: ChartStyle;
  priceLabel: string;
  priceUnit?: PriceUnit;
  metric?: ChartMetric;
  tradeCount: number;
  /** See {@link ChartChrome}. Default `full`. */
  chrome?: ChartChrome;
  /** When `chrome="compact"`, whether the full control cluster is expanded. */
  toolsOpen?: boolean;
  onToolsOpenChange?: (open: boolean) => void;
  showTradeMarkers: boolean;
  showWalletMarkers: boolean;
  /** Dev/creator marker toggle state. */
  showDevMarkers: boolean;
  /** False when no creator wallet is known — the Dev toggle renders disabled. */
  devMarkersAvailable: boolean;
  /** Show only the dev's first_buy/sell_all boundaries (hide mid-position trades). */
  devMarkersBoundariesOnly: boolean;
  /** Strategy entry/exit overlay (arrow markers + dashed fill-price lines) toggle. */
  showEventMarkers: boolean;
  /** False when the chart has no entry/exit overlay — the toggle renders disabled. */
  eventMarkersAvailable: boolean;
  showAthLine: boolean;
  athLineAvailable: boolean;
  showMigrationLine: boolean;
  trimEmptyBars: boolean;
  /** Vol/non-vol cumulative overlay lines (left price scale). */
  showFlowLines: boolean;
  /** False only when nothing can classify (no patterns AND no creator wallet)
   *  — toggle disabled. */
  flowLinesAvailable: boolean;
  /** True when the split comes from fingerprint `volume_ix_patterns`; false ⇒ the
   *  lines are the creator-vs-rest degradation (labelled as such in the tooltip). */
  flowPatternsConfigured: boolean;
  /** Range-select (drag-to-highlight) mode is active. */
  rangeSelectMode: boolean;
  crosshair: ChartCrosshairInfo | null;
  /** Formatter for cumulative vol/non-vol amounts in the toolbar readout. */
  formatFlow: (value: number) => string;
  isMigrated?: boolean;
  isMayhemMode?: boolean;
  isCashbackEnabled?: boolean;
  onGroupModeChange: (mode: ChartGroupMode) => void;
  onIntervalChange: (interval: ChartInterval) => void;
  onStyleChange: (style: ChartStyle) => void;
  onMetricChange?: (metric: ChartMetric) => void;
  onShowTradeMarkersChange: (show: boolean) => void;
  onShowWalletMarkersChange: (show: boolean) => void;
  onShowDevMarkersChange: (show: boolean) => void;
  onDevMarkersBoundariesOnlyChange: (only: boolean) => void;
  onShowEventMarkersChange: (show: boolean) => void;
  onShowAthLineChange: (show: boolean) => void;
  onShowMigrationLineChange: (show: boolean) => void;
  onTrimEmptyBarsChange: (trim: boolean) => void;
  onShowFlowLinesChange: (show: boolean) => void;
  onRangeSelectModeChange: (active: boolean) => void;
}
