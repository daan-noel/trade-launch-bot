import {
  CandlestickSeries,
  ColorType,
  CrosshairMode,
  LineSeries,
  type CandlestickSeriesOptions,
  type ChartOptions,
  type DeepPartial,
  type LineSeriesOptions,
} from 'lightweight-charts';
import { formatPrice } from '../../utils/format';
import type { PriceUnit } from '../../types';
import { createChartTimeFormatters } from './chartTimezone';
import type { ChartGroupMode, ChartInterval, ChartStyle } from './types';

/** Pump.fun total supply in raw token units (FDV market cap). */
export const TOKEN_TOTAL_SUPPLY = 1_000_000_000_000_000;

export function chartPricePrefix(unit: PriceUnit): string {
  return unit === 'USD' ? '$' : '◎';
}

/** Right-axis / crosshair labels for meme-token prices (uses e-notation when tiny). */
export function createChartPriceFormatter(unit: PriceUnit = 'SOL') {
  const prefix = chartPricePrefix(unit);
  return (price: number) => `${prefix} ${formatPrice(price)}`;
}

export const chartPriceFormatter = createChartPriceFormatter('SOL');

export function createChartPriceFormat(unit: PriceUnit = 'SOL') {
  return {
    type: 'custom' as const,
    formatter: createChartPriceFormatter(unit),
    minMove: 1e-18,
    base: 1e18,
  };
}

const CHART_PRICE_FORMAT = createChartPriceFormat('SOL');

export const CHART_INTERVALS: Record<ChartInterval, number> = {
  '1s': 1,
  '30s': 30,
  '1m': 60,
  '5m': 300,
};

export const CHART_INTERVAL_LABELS: ChartInterval[] = ['1s', '30s', '1m', '5m'];

export const CHART_STYLES: ChartStyle[] = ['candles', 'line'];

export const CHART_STYLE_LABELS: Record<ChartStyle, string> = {
  candles: 'Candles',
  line: 'Line',
};

export const CHART_COLORS = {
  up: '#089981',
  down: '#f23645',
  line: '#13ceaf',
  background: '#1a1a1a',
  grid: '#2a2a2a',
  text: '#999999',
  crosshair: '#13ceaf55',
  border: '#2a2a2a',
  panelText: '#e0e0e0',
  panelTextDim: '#888888',
  activePill: '#13ceaf',
  athLine: '#f0b429',
  migrationLine: '#5dade2',
  swingOverlay: '#e879f9',
  /** Swing path segments — distinct from candle up/down greens and reds */
  swingHigh: '#0eb5ff',
  swingLow: '#e879f9',
  /** Selected swing leg — high contrast on dark chart */
  swingSelected: '#13ceaf',
} as const;

/** Crosshair tooltip / toolbar — distinct hue per field on dark panels. */
export const CHART_OHLC_COLORS = {
  open: '#f0b429',
  high: '#34d399',
  low: '#f23645',
  close: '#c084fc',
  price: '#13ceaf',
  volume: '#fb923c',
  liquidity: '#5dade2',
} as const;

const SWING_OVERLAY_SERIES_BASE: DeepPartial<LineSeriesOptions> = {
  lineWidth: 3,
  crosshairMarkerVisible: false,
  priceLineVisible: false,
  lastValueVisible: false,
};

export const SWING_HIGH_OVERLAY_SERIES_OPTIONS: DeepPartial<LineSeriesOptions> = {
  ...SWING_OVERLAY_SERIES_BASE,
  color: CHART_COLORS.swingHigh,
};

export const SWING_LOW_OVERLAY_SERIES_OPTIONS: DeepPartial<LineSeriesOptions> = {
  ...SWING_OVERLAY_SERIES_BASE,
  color: CHART_COLORS.swingLow,
};

export const SWING_SELECTED_OVERLAY_SERIES_OPTIONS: DeepPartial<LineSeriesOptions> = {
  ...SWING_OVERLAY_SERIES_BASE,
  color: CHART_COLORS.swingSelected,
  lineWidth: 4,
};

/** @deprecated Use SWING_HIGH/LOW_OVERLAY_SERIES_OPTIONS — single magenta path */
export const SWING_OVERLAY_SERIES_OPTIONS: DeepPartial<LineSeriesOptions> = {
  ...SWING_OVERLAY_SERIES_BASE,
  color: CHART_COLORS.swingOverlay,
};

/** Pump.fun bonding-curve initial reserves (SOL and raw token units). */
const PUMP_INITIAL_VIRTUAL_SOL = 30;
const PUMP_INITIAL_VIRTUAL_TOKEN = 1_073_000_000_000_000;
const PUMP_INITIAL_REAL_TOKEN = 793_100_000_000_000;

/** Spot price (SOL per raw token) when the bonding curve completes. */
export const PUMP_MIGRATION_SPOT_PRICE_SOL = (() => {
  const finalVirtualToken = PUMP_INITIAL_VIRTUAL_TOKEN - PUMP_INITIAL_REAL_TOKEN;
  const finalVirtualSol =
    (PUMP_INITIAL_VIRTUAL_SOL * PUMP_INITIAL_VIRTUAL_TOKEN) / finalVirtualToken;
  return finalVirtualSol / finalVirtualToken;
})();

export const LS_CHART_PREFS_KEY = 'token_price_chart_prefs';

export const CHART_GROUP_MODES: ChartGroupMode[] = ['time', 'slot'];

export const CHART_GROUP_MODE_LABELS: Record<ChartGroupMode, string> = {
  time: 'Time',
  slot: 'Slot',
};

export const DEFAULT_CHART_PREFS = {
  groupMode: 'time' as ChartGroupMode,
  interval: '1m' as ChartInterval,
  style: 'candles' as ChartStyle,
  showTradeMarkers: true,
  showAthLine: true,
  showMigrationLine: true,
};

export function createChartOptions(
  width: number,
  height: number,
  groupMode: ChartGroupMode = 'time',
  priceUnit: PriceUnit = 'SOL',
  timezone?: string,
): DeepPartial<ChartOptions> {
  const slotTimeFormatter = (time: number) => String(time);
  const timeFormatters =
    groupMode === 'time' && timezone ? createChartTimeFormatters(timezone) : null;

  return {
    width,
    height,
    layout: {
      background: { type: ColorType.Solid, color: CHART_COLORS.background },
      textColor: CHART_COLORS.text,
    },
    grid: {
      vertLines: { color: CHART_COLORS.grid },
      horzLines: { color: CHART_COLORS.grid },
    },
    rightPriceScale: { borderColor: CHART_COLORS.border },
    timeScale: {
      borderColor: CHART_COLORS.border,
      timeVisible: groupMode === 'time',
      secondsVisible: groupMode === 'time',
      ...(groupMode === 'slot'
        ? { tickMarkFormatter: slotTimeFormatter }
        : timeFormatters
          ? { tickMarkFormatter: timeFormatters.tickMarkFormatter }
          : {}),
    },
    crosshair: {
      mode: CrosshairMode.Normal,
      vertLine: { color: CHART_COLORS.crosshair },
      horzLine: { color: CHART_COLORS.crosshair },
    },
    localization: {
      priceFormatter: createChartPriceFormatter(priceUnit),
      ...(groupMode === 'slot'
        ? { timeFormatter: slotTimeFormatter }
        : timeFormatters
          ? { timeFormatter: timeFormatters.timeFormatter }
          : {}),
    },
  };
}

export const LINE_SERIES_OPTIONS: DeepPartial<LineSeriesOptions> = {
  color: CHART_COLORS.line,
  lineWidth: 2,
  crosshairMarkerVisible: true,
  priceLineVisible: true,
  lastValueVisible: true,
  priceFormat: CHART_PRICE_FORMAT,
};

export const CANDLE_SERIES_OPTIONS: DeepPartial<CandlestickSeriesOptions> = {
  upColor: CHART_COLORS.up,
  downColor: CHART_COLORS.down,
  borderUpColor: CHART_COLORS.up,
  borderDownColor: CHART_COLORS.down,
  wickUpColor: CHART_COLORS.up,
  wickDownColor: CHART_COLORS.down,
  priceLineVisible: true,
  lastValueVisible: true,
  priceFormat: CHART_PRICE_FORMAT,
};

/** Extend with new series types here when adding chart styles. */
export const SERIES_BY_STYLE = {
  line: LineSeries,
  candles: CandlestickSeries,
} as const;
