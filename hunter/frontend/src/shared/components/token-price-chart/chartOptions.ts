import {
  CandlestickSeries,
  ColorType,
  CrosshairMode,
  LineSeries,
  type ChartOptions,
  type DeepPartial,
} from 'lightweight-charts';
import type { PriceUnit } from 'types';
import { createChartTimeFormatters } from './chartTimezone';
import {
  CHART_COLORS,
  CHART_HANDLE_SCALE,
  createChartPriceFormatter,
} from './constants';
import type { ChartGroupMode } from './types';

/**
 * The ONE module that imports `lightweight-charts` *values* outside a chart
 * component. It is deliberately split off `constants.ts`: that file's colors,
 * storage key, and prefs defaults are read by dozens of non-chart modules
 * (theme tokens, PnL widgets, the timezone context), so a value import there
 * pulled the whole ~150 kB charting library into the eagerly-loaded app root
 * chunk. Keep `constants.ts` free of lightweight-charts VALUE imports (types
 * are erased and cost nothing) — anything needing a real enum or series
 * constructor belongs here, next to the lazily-loaded charts.
 */

export const PRICE_SCALE_MARGINS = { top: 0.1, bottom: 0.1 };

/**
 * One price axis (right). Every series on the token chart - the candles and the
 * `@tag` / `@!tag` flow lines, drawn as cohort curve prices - shares it, so one
 * zoom moves them all and no series can drift off the others' scale.
 */
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
    rightPriceScale: {
      borderColor: CHART_COLORS.border,
      scaleMargins: PRICE_SCALE_MARGINS,
      autoScale: true,
    },
    handleScale: { ...CHART_HANDLE_SCALE },
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

/** Extend with new series types here when adding chart styles. */
export const SERIES_BY_STYLE = {
  line: LineSeries,
  candles: CandlestickSeries,
} as const;
