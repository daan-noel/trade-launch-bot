import { memo, useEffect, useRef } from 'react';
import {
  BaselineSeries,
  ColorType,
  CrosshairMode,
  type IChartApi,
  type ISeriesApi,
  type UTCTimestamp,
  createChart,
} from 'lightweight-charts';
import { CHART_COLORS, chartPricePrefix } from 'components/token-price-chart/constants';
import { createChartTimeFormatters } from 'components/token-price-chart/chartTimezone';
import { formatDecimalTrim } from 'utils/format';
import type { EquityPoint } from './walletPnlStats';

interface WalletEquityCurveChartProps {
  points: EquityPoint[];
  timezone: string;
  height?: number;
}

/**
 * Cumulative mark-to-market PnL across every mint the wallet touched, ordered
 * by each mint's most-recent trade — the single "is this wallet good" glance
 * chart. `BaselineSeries` (not a plain line) so the fill flips green above zero
 * / red below it automatically, matching the candle up/down palette. Lazy-
 * loaded by the caller (pulls `lightweight-charts`, same pattern as every other
 * chart in this app).
 */
export const WalletEquityCurveChart = memo(function WalletEquityCurveChart({
  points,
  timezone,
  height = 220,
}: WalletEquityCurveChartProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const chartRef = useRef<IChartApi | null>(null);
  const seriesRef = useRef<ISeriesApi<'Baseline'> | null>(null);

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
  }, [height, timezone]);

  useEffect(() => {
    const series = seriesRef.current;
    if (!series) return;
    series.setData(points.map((p) => ({ time: p.time as UTCTimestamp, value: p.cumPnlSol })));
    chartRef.current?.timeScale().fitContent();
  }, [points]);

  return <div ref={containerRef} className="w-full" style={{ height }} />;
});
