import { useEffect, useRef } from 'react';
import {
  createChart,
  ColorType,
  LineSeries,
  type IChartApi,
  type ISeriesApi,
  type UTCTimestamp,
} from 'lightweight-charts';
import type { TradePriced } from '@shared/types';

export interface PricePoint {
  time: UTCTimestamp;
  value: number;
}

/**
 * De-duplicated, time-ascending spot-price series from a token's priced trades.
 * Uses `spot_price_quote` (the canonical curve/pool spot per the project's price
 * convention), falling back to `exec_price_quote`. The value is the raw quote
 * ratio — monotonic and correct in shape; absolute USD lives in TokenOverview.
 * Collapses trades sharing a whole second to the last one.
 */
export function tradesToPriceSeries(trades: TradePriced[]): PricePoint[] {
  const byTime = new Map<number, number>();
  for (const t of trades) {
    const price = t.spot_price_quote ?? t.exec_price_quote;
    if (price == null) continue;
    const sec = Math.floor(new Date(t.block_time).getTime() / 1000);
    byTime.set(sec, price);
  }
  return [...byTime.entries()]
    .sort((a, b) => a[0] - b[0])
    .map(([time, value]) => ({ time: time as UTCTimestamp, value }));
}

export function PriceChart({ trades, height = 260 }: { trades: TradePriced[]; height?: number }) {
  const ref = useRef<HTMLDivElement>(null);
  const chartRef = useRef<IChartApi | null>(null);
  const seriesRef = useRef<ISeriesApi<'Line'> | null>(null);

  // Create the chart once.
  useEffect(() => {
    if (!ref.current) return;
    const chart = createChart(ref.current, {
      height,
      layout: {
        background: { type: ColorType.Solid, color: 'transparent' },
        textColor: '#9aa3b2',
        fontSize: 11,
      },
      grid: {
        vertLines: { color: 'rgba(255,255,255,0.04)' },
        horzLines: { color: 'rgba(255,255,255,0.04)' },
      },
      rightPriceScale: { borderColor: '#2a2f3a' },
      timeScale: { borderColor: '#2a2f3a', timeVisible: true, secondsVisible: false },
      crosshair: { mode: 0 },
      autoSize: true,
    });
    const series = chart.addSeries(LineSeries, {
      color: '#7eb8ff',
      lineWidth: 2,
      priceLineVisible: false,
      lastValueVisible: true,
    });
    chartRef.current = chart;
    seriesRef.current = series;
    return () => {
      chart.remove();
      chartRef.current = null;
      seriesRef.current = null;
    };
  }, [height]);

  // Push data whenever trades change.
  useEffect(() => {
    if (!seriesRef.current) return;
    seriesRef.current.setData(tradesToPriceSeries(trades));
    chartRef.current?.timeScale().fitContent();
  }, [trades]);

  return <div ref={ref} style={{ width: '100%', height }} />;
}
