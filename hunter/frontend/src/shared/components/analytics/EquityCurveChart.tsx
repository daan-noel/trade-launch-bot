import { memo, useEffect, useRef } from 'react';
import {
  BaselineSeries,
  ColorType,
  CrosshairMode,
  LineSeries,
  type IChartApi,
  type ISeriesApi,
  type UTCTimestamp,
  createChart,
} from 'lightweight-charts';
import { CHART_COLORS, chartPricePrefix } from 'components/token-price-chart/constants';
import { createChartTimeFormatters } from 'components/token-price-chart/chartTimezone';
import { formatDecimalTrim } from 'utils/format';
import type { EquityPoint } from './pnlSeries';

interface EquityCurveChartProps {
  points: EquityPoint[];
  timezone: string;
  height?: number;
  /** Overlay the running peak so the gap to the curve reads as drawdown. */
  showPeak?: boolean;
}

/**
 * Cumulative realized PnL over time — the single "is this working" glance chart.
 * `BaselineSeries` (not a plain line) so the fill flips green above zero / red
 * below it automatically, matching the candle up/down palette. The optional peak
 * line makes drawdown visible as the gap beneath it.
 *
 * The only chart in the analytics deck that pulls `lightweight-charts`: it is
 * the one with a real time axis worth panning/zooming. Lazy-load it at the call
 * site (same pattern as every other chart in this app) so the histogram /
 * calendar / heatmap don't drag the library in.
 */
export const EquityCurveChart = memo(function EquityCurveChart({
  points,
  timezone,
  height = 220,
  showPeak = true,
}: EquityCurveChartProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const chartRef = useRef<IChartApi | null>(null);
  const seriesRef = useRef<ISeriesApi<'Baseline'> | null>(null);
  const peakRef = useRef<ISeriesApi<'Line'> | null>(null);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const formatters = createChartTimeFormatters(timezone);

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
        priceFormatter: (v: number) => `${chartPricePrefix('SOL')}${formatDecimalTrim(v, 3)}`,
        timeFormatter: formatters.timeFormatter,
      },
    });
    const series = chart.addSeries(BaselineSeries, {
      baseValue: { type: 'price', price: 0 },
      topLineColor: CHART_COLORS.up,
      topFillColor1: `${CHART_COLORS.up}33`,
      topFillColor2: `${CHART_COLORS.up}05`,
      bottomLineColor: CHART_COLORS.down,
      bottomFillColor1: `${CHART_COLORS.down}05`,
      bottomFillColor2: `${CHART_COLORS.down}33`,
      lineWidth: 2,
      priceLineVisible: false,
      lastValueVisible: true,
    });
    chartRef.current = chart;
    seriesRef.current = series;
    if (showPeak) {
      peakRef.current = chart.addSeries(LineSeries, {
        color: CHART_COLORS.athLine,
        lineWidth: 1,
        lineStyle: 2,
        priceLineVisible: false,
        lastValueVisible: false,
        crosshairMarkerVisible: false,
      });
    }

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
      peakRef.current = null;
    };
  }, [height, timezone, showPeak]);

  useEffect(() => {
    const series = seriesRef.current;
    if (!series) return;
    series.setData(points.map((p) => ({ time: p.time as UTCTimestamp, value: p.cumPnlSol })));
    peakRef.current?.setData(
      points.map((p) => ({ time: p.time as UTCTimestamp, value: p.peakSol })),
    );
    chartRef.current?.timeScale().fitContent();
  }, [points]);

  return <div ref={containerRef} className="w-full" style={{ height }} />;
});
