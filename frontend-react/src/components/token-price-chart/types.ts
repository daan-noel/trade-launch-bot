import type { UTCTimestamp } from 'lightweight-charts';
import type { PriceUnit } from '../../types';

/** Minimal trade shape required by the chart — copy this folder as-is into other projects. */
export interface ChartTrade {
  block_time: string;
  price_per_token: number;
  trade_type: 'buy' | 'sell';
  sol_amount?: number;
  /** Raw token units swapped — used to reconstruct the pre-trade (genesis) open. */
  token_amount?: number;
  slot?: number;
  /** Transaction signature — chronological tiebreaker (keeps a tx's legs contiguous). */
  tx_signature?: string;
  /** Leg index within the transaction (0 = first) — chronological tiebreaker. */
  leg_index?: number;
  virtual_sol_reserves?: number | null;
  virtual_token_reserves?: number | null;
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
  /** Swing detection legs to draw as an overlay line. */
  swingOverlay?: ChartSwingOverlay | null;
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
  /** Tracked profile wallets to render as colored buy/sell markers. */
  profileWallets?: ProfileWalletInfo[];
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
  showAthLine: boolean;
  athLineAvailable: boolean;
  showMigrationLine: boolean;
  trimEmptyBars: boolean;
  swingOverlayAvailable: boolean;
  showSwingOverlay: boolean;
  connectSwings: boolean;
  crosshair: ChartCrosshairInfo | null;
  isMigrated?: boolean;
  isMayhemMode?: boolean;
  isCashbackEnabled?: boolean;
  onGroupModeChange: (mode: ChartGroupMode) => void;
  onIntervalChange: (interval: ChartInterval) => void;
  onStyleChange: (style: ChartStyle) => void;
  onMetricChange?: (metric: ChartMetric) => void;
  onShowTradeMarkersChange: (show: boolean) => void;
  onShowAthLineChange: (show: boolean) => void;
  onShowMigrationLineChange: (show: boolean) => void;
  onTrimEmptyBarsChange: (trim: boolean) => void;
  onShowSwingOverlayChange: (show: boolean) => void;
  onConnectSwingsChange: (connected: boolean) => void;
}
