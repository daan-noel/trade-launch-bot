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
import { STORAGE_KEYS } from 'lib/storage';
import { formatPrice } from 'utils/format';
import type { PriceUnit } from 'types';
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

/** The candle up/down pair — the one definition of "green" and "red" on the chart.
 *  Buy/sell direction aliases these (see `CHART_COLORS.buy`/`.sell`). */
const CANDLE_UP = '#089981';
const CANDLE_DOWN = '#f23645';

export const CHART_COLORS = {
  up: CANDLE_UP,
  down: CANDLE_DOWN,
  /** Trade DIRECTION (buy/sell) — ALIASES of the candle `up`/`down` pair, on
   *  purpose: a buy must always render the same green as an up-candle and a sell
   *  the same red as a down-candle. Referencing the consts (not re-typing the
   *  hexes) is what keeps them from drifting. Mirrors the `--color-buy` /
   *  `--color-sell` theme tokens in `index.css`, which alias `--color-green` /
   *  `--color-red` the same way. */
  buy: CANDLE_UP,
  sell: CANDLE_DOWN,
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
  /** Strategy entry/exit overlay (TPSL result inspection): bright green/red,
   *  distinct from the candle up/down greens & reds so they pop at a glance. */
  entry: '#02c076',
  exit: '#f6465d',
  /** Frontend metric-pane signal markers (first condition fire) — cool/amber so
   *  they never collide with the fill entry/exit greens & reds above. */
  signalEntry: '#5dade2',
  signalExit: '#f0b429',
  /** Selected OHLC bar / line point */
  barSelected: '#fddc3b',
  /** Fixed marker color for the user's own ("mine") tracked wallet — bypasses the
   *  rotating WALLET_MARKER_COLORS palette so it's recognizable at a glance among
   *  other tracked trader/whale/dev wallets. */
  mine: '#fbbf24',
  /** Fixed marker color for the token's dev/creator wallet — violet, the one gap
   *  in the palette (everything else is green/red/gold/amber/blue), so a dev
   *  first_buy/sell_all never collides with candle, direction, or wallet colors. */
  dev: '#a855f7',
  /** Glow + outer ring for the focused/highlighted wallet marker (Trader
   *  Analysis input wallet). Gold, distinct from `mine` so a wallet can be both
   *  the focus AND one of yours without the two signals colliding. */
  highlightRing: '#fde047',
  /** User-drawn range selection band (teal wash + solid label chip) */
  rangeBandFill: 'rgba(19, 206, 175, 0.12)',
  rangeBandBorder: 'rgba(19, 206, 175, 0.7)',
  rangeBandLabelBg: 'rgba(19, 206, 175, 0.92)',
  rangeBandLabelText: '#0a1a17',
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

/** Pump.fun bonding-curve initial reserves (SOL and raw token units). */
export const PUMP_INITIAL_VIRTUAL_SOL = 30;
const PUMP_INITIAL_VIRTUAL_TOKEN = 1_073_000_000_000_000;
const PUMP_INITIAL_REAL_TOKEN = 793_100_000_000_000;

/** Spot price (SOL per raw token) when the bonding curve completes. */
export const PUMP_MIGRATION_SPOT_PRICE_SOL = (() => {
  const finalVirtualToken = PUMP_INITIAL_VIRTUAL_TOKEN - PUMP_INITIAL_REAL_TOKEN;
  const finalVirtualSol =
    (PUMP_INITIAL_VIRTUAL_SOL * PUMP_INITIAL_VIRTUAL_TOKEN) / finalVirtualToken;
  return finalVirtualSol / finalVirtualToken;
})();

export const LS_CHART_PREFS_KEY = STORAGE_KEYS.chartPrefs;

/** Distinct colors cycled across tracked profile wallets. */
export const WALLET_MARKER_COLORS = [
  '#f59e0b', // amber
  '#06b6d4', // cyan
  '#8b5cf6', // violet
  '#ec4899', // pink
  '#10b981', // emerald
  '#f97316', // orange
  '#6366f1', // indigo
  '#a855f7', // purple
  '#14b8a6', // teal
  '#ef4444', // red
] as const;

export const CHART_GROUP_MODES: ChartGroupMode[] = ['time', 'slot'];

export const CHART_GROUP_MODE_LABELS: Record<ChartGroupMode, string> = {
  time: 'Time',
  slot: 'Slot',
};

/**
 * The chart's opening state, shared by both apps.
 *
 * Tuned for the read this chart is actually used for: what a token did in its
 * first seconds. Hence the **1s** interval (a 1m candle swallows the whole
 * window that decides an entry) and dev markers **on, boundaries-only** — the
 * dev's `first_buy`/`sell_all` are the signal, their manufactured mid-position
 * churn is not. Reference lines (ATH, migration, strategy entry/exit) and the
 * vol/non-vol flow overlay stay on; both are read every time and cost nothing
 * when their data is absent (the toolbar disables them).
 *
 * Off by design: `showTradeMarkers` is the per-bar buy/sell *count* badge, which
 * at a 1s interval is one badge per candle — it hides the price action it
 * annotates. `trimEmptyBars` would drop the no-trade intervals, but the gaps ARE
 * information here (a stalled token), and dropping them distorts the time axis.
 */
export const DEFAULT_CHART_PREFS = {
  groupMode: 'time' as ChartGroupMode,
  interval: '1s' as ChartInterval,
  style: 'candles' as ChartStyle,
  showTradeMarkers: false,
  showAthLine: true,
  showMigrationLine: true,
  trimEmptyBars: false,
  showWalletMarkers: true,
  showDevMarkers: true,
  devMarkersBoundariesOnly: true,
  showEventMarkers: true,
  showFlowLines: true,
};

/** Responsive chart height. The chart width fills its container (fluid), so on a
 *  wide monitor a fixed height renders as a wide-flat band that squashes price
 *  action and — in the flow view — collapses the vol vs non-vol separation. This
 *  grows the height with the width to keep a readable aspect ratio, clamped to
 *  [min, max]. Derived from WIDTH ONLY (never from an observed height) so there
 *  is no height->width->height feedback loop that could thrash a scrollbar
 *  gutter (see `TokenPriceChart`'s width-only ResizeObserver note). */
export const CHART_HEIGHT_MIN = 320;
export const CHART_HEIGHT_MAX = 560;
export const CHART_ASPECT_MAX = 2.2;

export function responsiveChartHeight(
  width: number,
  min = CHART_HEIGHT_MIN,
  max = CHART_HEIGHT_MAX,
  aspect = CHART_ASPECT_MAX,
): number {
  if (!(width > 0)) return min;
  return Math.round(Math.min(max, Math.max(min, width / aspect)));
}

/** Optional extras for {@link createChartOptions}. */
export interface CreateChartOptionsExtras {
  /**
   * Dual left+right price scales with independent units (e.g. flow overlay +
   * token price). Enables the left scale and omits the chart-level
   * `localization.priceFormatter` so each series' own `priceFormat` owns its
   * axis ticks — a single chart formatter would paint both scales the same.
   */
  dualPriceScale?: boolean;
}

const DUAL_PRICE_SCALE_MARGINS = { top: 0.1, bottom: 0.1 };

/** Shared by dual-axis charts so range-select teardown restores the same policy. */
export const DUAL_CHART_HANDLE_SCALE = {
  mouseWheel: true,
  pinch: true,
  axisPressedMouseMove: { time: true, price: true },
  axisDoubleClickReset: { time: true, price: true },
} as const;

export function createChartOptions(
  width: number,
  height: number,
  groupMode: ChartGroupMode = 'time',
  priceUnit: PriceUnit = 'SOL',
  timezone?: string,
  extras?: CreateChartOptionsExtras,
): DeepPartial<ChartOptions> {
  const slotTimeFormatter = (time: number) => String(time);
  const timeFormatters =
    groupMode === 'time' && timezone ? createChartTimeFormatters(timezone) : null;
  const dual = extras?.dualPriceScale === true;

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
    rightPriceScale: {
      borderColor: CHART_COLORS.border,
      ...(dual ? { scaleMargins: DUAL_PRICE_SCALE_MARGINS, autoScale: true } : {}),
    },
    ...(dual
      ? {
          leftPriceScale: {
            visible: true,
            borderColor: CHART_COLORS.border,
            scaleMargins: DUAL_PRICE_SCALE_MARGINS,
            autoScale: true,
          },
          handleScale: { ...DUAL_CHART_HANDLE_SCALE },
        }
      : {}),
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
      // Dual-axis charts must NOT set a chart-level priceFormatter — it overrides
      // every series' priceFormat and forces left+right through one unit.
      ...(dual ? {} : { priceFormatter: createChartPriceFormatter(priceUnit) }),
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
