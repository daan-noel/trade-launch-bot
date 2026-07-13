import type { UTCTimestamp } from 'lightweight-charts';

/** Right-axis / header price unit prefix. */
export type PriceUnit = 'SOL' | 'USD';

/** Minimal trade shape required by the chart — forge maps `TradePriced` onto this
 *  at the consumer boundary (see `PriceChart.tsx`). Kept in hunter's field vocab
 *  (`*_sol`, raw token units) so the ported aggregation/constants work unchanged. */
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

/** A tracked wallet to render as chart markers (forge: a managed-pool wallet). */
export interface ProfileWalletInfo {
  address: string;
  label?: string;
  color: string;
  profileName?: string;
  /** True for the launcher's own wallet class — gets a fixed marker glyph/shape. */
  isMine?: boolean;
  /** True for a single focused wallet — larger marker with a glow + ring. */
  isHighlighted?: boolean;
}

/**
 * A strategy entry/exit point to overlay on the chart: an arrow marker pinned to
 * the matching bar plus a dashed horizontal line at the fill price.
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
  isMigrated?: boolean;
  /** Tracked wallets to render as colored buy/sell markers. Pass `[]` for none. */
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
  showMigrationLine: boolean;
  trimEmptyBars: boolean;
  /** Range-select (drag-to-highlight) mode is active. */
  rangeSelectMode: boolean;
  crosshair: ChartCrosshairInfo | null;
  isMigrated?: boolean;
  onGroupModeChange: (mode: ChartGroupMode) => void;
  onIntervalChange: (interval: ChartInterval) => void;
  onStyleChange: (style: ChartStyle) => void;
  onMetricChange?: (metric: ChartMetric) => void;
  onShowTradeMarkersChange: (show: boolean) => void;
  onShowMigrationLineChange: (show: boolean) => void;
  onTrimEmptyBarsChange: (trim: boolean) => void;
  onRangeSelectModeChange: (active: boolean) => void;
}
