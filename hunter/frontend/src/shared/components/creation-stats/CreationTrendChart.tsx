import { memo, useEffect, useMemo, useRef } from 'react';
import {
  ColorType,
  CrosshairMode,
  HistogramSeries,
  createChart,
  type IChartApi,
  type ISeriesApi,
  type UTCTimestamp,
} from 'lightweight-charts';
import { CHART_COLORS } from 'components/token-price-chart/constants';
import { createChartTimeFormatters } from 'components/token-price-chart/chartTimezone';
import { formatWithCommas } from 'utils/format';
import {
  METRIC_KIND,
  METRIC_RGB,
  metricValue,
  type CreationMetric,
  type CreationTrendPoint,
} from './creationStats';

interface CreationTrendChartProps {
  points: CreationTrendPoint[];
  /** Drives the plotted field + bar color for the two `magnitude` metrics
   *  (`count`/`trades`/`trades_per_day`); `rate`/`ratio` metrics keep plotting
   *  `count` (a stretched rate/ratio is meaningless as a histogram). Defaults
   *  to `count` (today's behavior) when omitted. */
  metric?: CreationMetric;
  height?: number;
}

/**
 * Parse a backend bucket (`2026-06-01T00:00:00`, already shifted into the
 * selected tz = a naive local wall-clock) as a UTC instant. Feeding it to a
 * UTC-formatting axis renders the local wall-clock back verbatim — no double
 * tz shift. Backend guarantees ascending, unique buckets.
 */
function bucketToTime(bucket: string): UTCTimestamp {
  return Math.floor(Date.parse(`${bucket}Z`) / 1000) as UTCTimestamp;
}

/**
 * Panel A — absolute-calendar creation histogram (lightweight-charts). Plots
 * `count` by default; when `metric` is a `magnitude` metric (`trades` /
 * `trades_per_day`) it plots that instead (and re-colors to match) — `rate`/
 * `ratio` metrics (migrate %, dead %, trades/token) always fall back to
 * `count` (a stretched rate has no meaningful histogram bar height). Memoized;
 * the chart instance is created once and only the series color/data are
 * swapped when `points`/`metric` change, so it never churns on unrelated ticks.
 */
export const CreationTrendChart = memo(function CreationTrendChart({
  points,
  metric = 'count',
  height = 220,
}: CreationTrendChartProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const chartRef = useRef<IChartApi | null>(null);
  const seriesRef = useRef<ISeriesApi<'Histogram'> | null>(null);

  // UTC formatters: the buckets are pre-shifted, so format them as-is.
  const formatters = useMemo(() => createChartTimeFormatters('UTC'), []);

  // `rate`/`ratio` metrics (migrate %, dead %, trades/token) fall back to
  // plotting `count` — a contrast-stretched rate has no meaningful histogram
  // bar height. Only the two other `magnitude` metrics (trades, trades_per_day)
  // actually swap the plotted field.
  const plottedMetric: CreationMetric = METRIC_KIND[metric] === 'magnitude' ? metric : 'count';
  const seriesColor = useMemo(() => `rgba(${METRIC_RGB[plottedMetric]},0.7)`, [plottedMetric]);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;

    const chart = createChart(el, {
      width: el.clientWidth || 600,
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
        timeVisible: true,
        secondsVisible: false,
        tickMarkFormatter: formatters.tickMarkFormatter,
      },
      crosshair: {
        mode: CrosshairMode.Normal,
        vertLine: { color: CHART_COLORS.crosshair },
        horzLine: { color: CHART_COLORS.crosshair },
      },
      localization: {
        priceFormatter: (v: number) => formatWithCommas(Math.round(v)),
        timeFormatter: formatters.timeFormatter,
      },
    });
    const series = chart.addSeries(HistogramSeries, {
      color: `rgba(19,206,175,0.7)`,
      priceFormat: { type: 'volume' },
      priceLineVisible: false,
      lastValueVisible: false,
    });
    chartRef.current = chart;
    seriesRef.current = series;

    const ro = new ResizeObserver((entries) => {
      const w = entries[0]?.contentRect.width ?? 0;
      if (w > 0) chart.applyOptions({ width: w });
    });
    ro.observe(el);

    return () => {
      ro.disconnect();
      chart.remove();
      chartRef.current = null;
      seriesRef.current = null;
    };
  }, [height, formatters]);

  useEffect(() => {
    const series = seriesRef.current;
    if (!series) return;
    series.applyOptions({ color: seriesColor });
    series.setData(
      points.map((p) => ({
        time: bucketToTime(p.bucket),
        value: metricValue(p, plottedMetric) ?? 0,
      })),
    );
    chartRef.current?.timeScale().fitContent();
  }, [points, plottedMetric, seriesColor]);

  return <div ref={containerRef} className="w-full" style={{ height }} />;
});
