import { useEffect, useMemo, useRef, useState } from 'react';
import {
  CandlestickSeries,
  createChart,
  LineSeries,
  type IChartApi,
  type ISeriesApi,
} from 'lightweight-charts';
import {
  aggregateTradesToBars,
  aggregateTradesToBarsBySlot,
  barsToCandleData,
  dropEmptyBars,
} from 'components/token-price-chart/chartBars';
import {
  CANDLE_SERIES_OPTIONS,
  CHART_COLORS,
  CHART_INTERVAL_LABELS,
  CHART_INTERVALS,
  createChartOptions,
} from 'components/token-price-chart/constants';
import type { ChartGroupMode, ChartInterval } from 'components/token-price-chart/types';
import { Checkbox } from 'components/ui/Checkbox';
import { Select } from 'components/ui/Select';
import { useTimezone } from 'context/TimezoneContext';
import { buildFlowLines, type FlowBasis } from '@lab/lib/flow/flowChartData';
import type { TradeRecord } from 'types';

const BASIS_OPTIONS: { value: FlowBasis; label: string }[] = [
  { value: 'sol', label: 'SOL amount' },
  { value: 'token', label: 'Token amount' },
];

export interface FlowPreviewChartProps {
  trades: TradeRecord[];
  /** `JSON.stringify(labels)` keys of the checked volume_ix_patterns rows —
   *  redraws the two overlay lines whenever this set changes. */
  patternKeys: ReadonlySet<string>;
  /** Token creator wallet address, when known — offered as a toggle since the
   *  real classifier always treats the creator as volume-side. */
  creatorWallet?: string | null;
  height?: number;
}

/** Per-token candlestick chart with cumulative volume-maker (green) vs
 *  non-volume (brown/gray) overlay lines that redraw instantly as
 *  `patternKeys` changes — a client-side preview of the live wallet-flow
 *  classifier (see `classifyFlow.ts`). Not the shared `TokenPriceChart`: this
 *  is a small lab-only preview, kept out of the 1600-line shared component
 *  that `live` also consumes. */
export function FlowPreviewChart({
  trades,
  patternKeys,
  creatorWallet,
  height = 320,
}: FlowPreviewChartProps) {
  const { timezone } = useTimezone();
  const containerRef = useRef<HTMLDivElement | null>(null);
  const chartRef = useRef<IChartApi | null>(null);
  const candleSeriesRef = useRef<ISeriesApi<'Candlestick'> | null>(null);
  const volSeriesRef = useRef<ISeriesApi<'Line'> | null>(null);
  const nonVolSeriesRef = useRef<ISeriesApi<'Line'> | null>(null);

  const [groupMode, setGroupMode] = useState<ChartGroupMode>('time');
  const [interval, setInterval] = useState<ChartInterval>('1m');
  const [basis, setBasis] = useState<FlowBasis>('sol');
  const [seedCreatorAsVol, setSeedCreatorAsVol] = useState(false);

  // Create the chart + series once; resize in place afterward.
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const chart = createChart(
      el,
      createChartOptions(el.clientWidth, height, groupMode, 'SOL', timezone),
    );
    chartRef.current = chart;

    const candleSeries = chart.addSeries(CandlestickSeries, {
      ...CANDLE_SERIES_OPTIONS,
      priceScaleId: 'right',
    });
    candleSeriesRef.current = candleSeries;

    const volSeries = chart.addSeries(LineSeries, {
      color: CHART_COLORS.up,
      lineWidth: 2,
      priceScaleId: 'left',
      title: 'Vol makers',
      lastValueVisible: true,
      priceLineVisible: false,
    });
    volSeriesRef.current = volSeries;

    const nonVolSeries = chart.addSeries(LineSeries, {
      color: CHART_OHLC_ORANGE,
      lineWidth: 2,
      priceScaleId: 'left',
      title: 'Non-vol',
      lastValueVisible: true,
      priceLineVisible: false,
    });
    nonVolSeriesRef.current = nonVolSeries;

    chart.priceScale('left').applyOptions({ visible: true, borderColor: CHART_COLORS.border });

    const observer = new ResizeObserver(() => {
      if (containerRef.current) {
        chart.applyOptions({ width: containerRef.current.clientWidth });
      }
    });
    observer.observe(el);

    return () => {
      observer.disconnect();
      chart.remove();
      chartRef.current = null;
      candleSeriesRef.current = null;
      volSeriesRef.current = null;
      nonVolSeriesRef.current = null;
    };
    // groupMode/timezone changes need a fresh time-scale formatter — recreate.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [groupMode, height, timezone]);

  const intervalSec = CHART_INTERVALS[interval];

  const bars = useMemo(() => {
    const raw =
      groupMode === 'slot'
        ? aggregateTradesToBarsBySlot(trades, (v) => v, 'price')
        : aggregateTradesToBars(trades, intervalSec, (v) => v, 'price');
    return dropEmptyBars(raw);
  }, [trades, groupMode, intervalSec]);

  const classifyOpts = useMemo(
    () => ({ patternKeys, creatorWallet: seedCreatorAsVol ? creatorWallet : null }),
    [patternKeys, creatorWallet, seedCreatorAsVol],
  );

  const lines = useMemo(
    () => buildFlowLines(trades, groupMode, intervalSec, basis, classifyOpts),
    [trades, groupMode, intervalSec, basis, classifyOpts],
  );

  useEffect(() => {
    candleSeriesRef.current?.setData(barsToCandleData(bars));
  }, [bars]);

  useEffect(() => {
    volSeriesRef.current?.setData(lines.vol);
    nonVolSeriesRef.current?.setData(lines.nonVol);
  }, [lines]);

  return (
    <div className="flex flex-col gap-2">
      <div className="flex flex-wrap items-center gap-3 text-[11px] text-text-dim">
        <Select
          fieldSize="sm"
          value={groupMode}
          onChange={(e) => setGroupMode(e.target.value as ChartGroupMode)}
        >
          <option value="time">Time</option>
          <option value="slot">Slot</option>
        </Select>
        {groupMode === 'time' && (
          <Select
            fieldSize="sm"
            value={interval}
            onChange={(e) => setInterval(e.target.value as ChartInterval)}
          >
            {CHART_INTERVAL_LABELS.map((i) => (
              <option key={i} value={i}>
                {i}
              </option>
            ))}
          </Select>
        )}
        <Select
          fieldSize="sm"
          value={basis}
          onChange={(e) => setBasis(e.target.value as FlowBasis)}
        >
          {BASIS_OPTIONS.map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </Select>
        <label className="flex items-center gap-1.5">
          <Checkbox
            checked={seedCreatorAsVol}
            disabled={!creatorWallet}
            onChange={(e) => setSeedCreatorAsVol(e.target.checked)}
          />
          Seed creator as vol
        </label>
      </div>
      <div ref={containerRef} style={{ height }} />
    </div>
  );
}

/** Non-vol overlay color — brown/amber, distinct from the buy/sell candle
 *  green/red pair (`CHART_COLORS.up`/`.down`) so it reads as a third signal. */
const CHART_OHLC_ORANGE = '#c17a3a';
