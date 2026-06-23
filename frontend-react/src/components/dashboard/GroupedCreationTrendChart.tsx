import { memo, useEffect, useMemo, useRef } from 'react';
import {
  ColorType,
  CrosshairMode,
  LineSeries,
  createChart,
  type IChartApi,
  type ISeriesApi,
  type UTCTimestamp,
} from 'lightweight-charts';
import { CHART_COLORS } from 'components/token-price-chart/constants';
import { createChartTimeFormatters } from 'components/token-price-chart/chartTimezone';
import { formatWithCommas } from 'utils/format';
import {
  groupColor,
  groupShortLabel,
  type GroupedCreationGroup,
  type GroupedCreationPoint,
} from './groupedCreationStats';

interface GroupedCreationTrendChartProps {
  points: GroupedCreationPoint[];
  groups: GroupedCreationGroup[];
  height?: number;
}

/** See `CreationTrendChart.bucketToTime` — buckets are pre-shifted naive
 *  wall-clock, parsed as UTC so a UTC-formatting axis renders them verbatim. */
function bucketToTime(bucket: string): UTCTimestamp {
  return Math.floor(Date.parse(`${bucket}Z`) / 1000) as UTCTimestamp;
}

/**
 * Multi-series calendar trend — one line per fingerprint group (rank-colored via
 * [`groupColor`] so it matches the heatmap cards + legend). Built like
 * `CreationTrendChart` (single histogram) but draws N line series; the chart is
 * created once and the series are rebuilt only when `points`/`groups` change, so
 * it never churns on unrelated SOL/USD or live-trade ticks.
 */
export const GroupedCreationTrendChart = memo(function GroupedCreationTrendChart({
  points,
  groups,
  height = 240,
}: GroupedCreationTrendChartProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const chartRef = useRef<IChartApi | null>(null);
  const seriesRef = useRef<ISeriesApi<'Line'>[]>([]);

  // UTC formatters: the buckets are pre-shifted, so format them as-is.
  const formatters = useMemo(() => createChartTimeFormatters('UTC'), []);

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
    chartRef.current = chart;

    const ro = new ResizeObserver((entries) => {
      const w = entries[0]?.contentRect.width ?? 0;
      if (w > 0) chart.applyOptions({ width: w });
    });
    ro.observe(el);

    return () => {
      ro.disconnect();
      chart.remove();
      chartRef.current = null;
      seriesRef.current = []; // series are disposed with the chart
    };
  }, [height, formatters]);

  // Rebuild one line series per group whenever the data changes (refetch only).
  useEffect(() => {
    const chart = chartRef.current;
    if (!chart) return;

    for (const s of seriesRef.current) chart.removeSeries(s);
    seriesRef.current = [];

    // Group points by rank index; SQL returns buckets ascending, so each
    // group's slice is already ordered + unique (lightweight-charts requirement).
    const byGroup = new Map<number, { time: UTCTimestamp; value: number }[]>();
    for (const p of points) {
      const arr = byGroup.get(p.g) ?? [];
      arr.push({ time: bucketToTime(p.bucket), value: p.count });
      byGroup.set(p.g, arr);
    }

    for (const grp of groups) {
      const data = byGroup.get(grp.g);
      if (!data || data.length === 0) continue;
      const series = chart.addSeries(LineSeries, {
        color: groupColor(grp.g),
        lineWidth: 2,
        priceLineVisible: false,
        lastValueVisible: false,
        title: groupShortLabel(grp),
        priceFormat: { type: 'volume' },
      });
      series.setData(data);
      seriesRef.current.push(series);
    }
    chart.timeScale().fitContent();
  }, [points, groups]);

  return <div ref={containerRef} className="w-full" style={{ height }} />;
});
