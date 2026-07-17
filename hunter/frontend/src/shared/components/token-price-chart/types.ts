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
  real_sol_reserves?: number | null;
  real_token_reserves?: number | null;
  venue?: 'curve' | 'amm';
  wallet_address?: string;
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
}

export interface ChartBarSelection {
  barTime: UTCTimestamp;
  groupMode: ChartGroupMode;
  /** Set when groupMode is `time`. */
  intervalSec?: number;
  /** Set when groupMode is `slot`. */
  slot?: number;
}

/** Swing leg overlay — matches backend `SwingLegRecord` shape. */
export interface ChartSwingLeg {
  type: 'swing_high' | 'swing_low';
  start_at: number;
  end_at: number;
  start_price: number;
  end_price: number;
  /** Terminal pivot drawn as the leg's end point (last big tx / price extreme).
   *  Falls back to `end_at`/`end_price` when absent. */
  pivot_end_at?: number;
  pivot_end_price?: number;
  duration_ms?: number;
  inflow?: number;
  outflow?: number;
  net_flow?: number;
  trade_count?: number;
}

export interface ChartSwingTooltipState {
  leg: ChartSwingLeg;
  point: { x: number; y: number };
}

/** Longest swing chain to highlight as a translucent full-height band. */
export interface ChartChainHighlight {
  /** Chain start (ms epoch) — first pair's high start. */
  startAt: number;
  /** Chain end (ms epoch) — last pair's low end. */
  endAt: number;
  /** Pairs linked in the chain (for the label). */
  pairCount: number;
  /** Total SOL inflow across every leg in the chain. */
  inflow: number;
  /** Total SOL outflow across every leg in the chain. */
  outflow: number;
  /** Net SOL flow (inflow − outflow) across the chain. */
  netFlow: number;
  /** Chain wall-clock span (ms). */
  durationMs: number;
  /** Price change across the chain (last end price − first start price). */
  priceDelta: number;
  /** `priceDelta` as a percentage of the first start price; null if unknown. */
  priceDeltaPct: number | null;
  /** Total trades across every leg in the chain. */
  tradeCount: number;
}

/** Tooltip shown when the crosshair hovers the chain label chip. */
export interface ChartChainTooltipState {
  highlight: ChartChainHighlight;
  point: { x: number; y: number };
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
  barTime: UTCTimestamp;
  /** Age of the bar's earliest trade since token creation (seconds); null when unknown. */
  ageSec: number | null;
  style: ChartStyle;
  point: { x: number; y: number };
}

export interface ChartSwingOverlay {
  legs: ChartSwingLeg[];
  /**
   * `connected` — full path through legs.
   * `perLeg` — isolated start→end per leg.
   * `connectedSequential` — connected within consecutive legs in `allLegs` (filtered subset).
   */
  segmentMode?: 'connected' | 'perLeg' | 'connectedSequential';
  /** Full detection order; required for `connectedSequential`. */
  allLegs?: ChartSwingLeg[];
  /**
   * Breakage gap (ms). When > 0, a `connected` path is split wherever a leg starts
   * more than this many ms after the previous leg ended, so the overlay shows a
   * visible break across a dust/idle breakage instead of a misleading diagonal.
   * `undefined`/`0` = legacy bridging. Only applies to `connected`/`connectedSequential`.
   */
  gapBreakMs?: number;
  /**
   * `perLeg` only: end each leg's segment at its full-span `end_at`/`end_price`
   * instead of its terminal pivot, so the line aligns with the full-span candle
   * highlight. Default `false` (segments end at the pivot).
   */
  perLegFullSpanEnd?: boolean;
}

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
  /** Swing detection legs to draw as an overlay line. */
  swingOverlay?: ChartSwingOverlay | null;
  /** Longest swing chain to highlight as a translucent band (after batch detection). */
  highlightChain?: ChartChainHighlight | null;
  /** `${type}-${start_at}-${end_at}` — highlights that leg on the overlay. */
  selectedSwingLegKey?: string | null;
  /** Fired when a swing path segment is clicked; null clears selection. */
  onSwingLegClick?: (leg: ChartSwingLeg | null) => void;
  /** Token ATH spot price in SOL (from tokens_info). */
  athPriceInSol?: number | null;
  isMigrated?: boolean;
  isMayhemMode?: boolean;
  isCashbackEnabled?: boolean;
  /** Chain swing reversal points (default true). */
  connectSwings?: boolean;
  onConnectSwingsChange?: (connected: boolean) => void;
  /** Tracked profile wallets to render as colored buy/sell markers. OMIT to get
   *  the tracked wallets automatically (the chart falls back to `useProfileWallets`
   *  internally) — every token trade chart shows tracked-wallet markers by
   *  default. Pass an explicit list to override (e.g. `TokenTradeChart` adds the
   *  highlighted/synthetic input wallet); pass `[]` to force no markers. */
  profileWallets?: ProfileWalletInfo[];
  /** Token creation time (ISO string) — used to show per-bar tx age in the crosshair tooltip. */
  tokenCreatedAt?: string;
  /** Strategy entry/exit points to overlay as arrows + dashed price lines. */
  eventMarkers?: ChartEventMarker[] | null;
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
  showTradeMarkers: boolean;
  showWalletMarkers: boolean;
  showAthLine: boolean;
  athLineAvailable: boolean;
  showMigrationLine: boolean;
  trimEmptyBars: boolean;
  swingOverlayAvailable: boolean;
  showSwingOverlay: boolean;
  connectSwings: boolean;
  chainHighlightAvailable: boolean;
  showChainHighlight: boolean;
  /** Range-select (drag-to-highlight) mode is active. */
  rangeSelectMode: boolean;
  crosshair: ChartCrosshairInfo | null;
  isMigrated?: boolean;
  isMayhemMode?: boolean;
  isCashbackEnabled?: boolean;
  onGroupModeChange: (mode: ChartGroupMode) => void;
  onIntervalChange: (interval: ChartInterval) => void;
  onStyleChange: (style: ChartStyle) => void;
  onMetricChange?: (metric: ChartMetric) => void;
  onShowTradeMarkersChange: (show: boolean) => void;
  onShowWalletMarkersChange: (show: boolean) => void;
  onShowAthLineChange: (show: boolean) => void;
  onShowMigrationLineChange: (show: boolean) => void;
  onTrimEmptyBarsChange: (trim: boolean) => void;
  onShowSwingOverlayChange: (show: boolean) => void;
  onConnectSwingsChange: (connected: boolean) => void;
  onShowChainHighlightChange: (show: boolean) => void;
  onRangeSelectModeChange: (active: boolean) => void;
}
