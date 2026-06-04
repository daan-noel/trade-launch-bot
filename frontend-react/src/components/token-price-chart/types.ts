import type { UTCTimestamp } from 'lightweight-charts';
import type { PriceUnit } from '../../types';

/** Minimal trade shape required by the chart — copy this folder as-is into other projects. */
export interface ChartTrade {
  block_time: string;
  price_per_token: number;
  trade_type: 'buy' | 'sell';
  sol_amount?: number;
  slot?: number;
  virtual_sol_reserves?: number | null;
  virtual_token_reserves?: number | null;
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
}

export interface ChartSwingOverlay {
  legs: ChartSwingLeg[];
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
  /** Swing detection legs to draw as an overlay line. */
  swingOverlay?: ChartSwingOverlay | null;
  /** Token ATH spot price in SOL (from tokens_info). */
  athPriceInSol?: number | null;
  isMigrated?: boolean;
  isMayhemMode?: boolean;
  isCashbackEnabled?: boolean;
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
  swingOverlayAvailable: boolean;
  showSwingOverlay: boolean;
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
  onShowSwingOverlayChange: (show: boolean) => void;
}
