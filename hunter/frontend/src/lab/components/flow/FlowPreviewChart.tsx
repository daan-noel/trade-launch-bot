import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import {
  CandlestickSeries,
  createChart,
  createSeriesMarkers,
  LineSeries,
  LineStyle,
  type Coordinate,
  type IChartApi,
  type IPriceLine,
  type ISeriesApi,
  type MouseEventParams,
  type SeriesMarker,
  type Time,
  type UTCTimestamp,
} from 'lightweight-charts';
import {
  aggregateTradesToBars,
  aggregateTradesToBarsBySlot,
  athChartValue,
  barSelectionMarker,
  barsToCandleData,
  barsToLineData,
  compareTradesChronologically,
  computeRangeStats,
  dropEmptyBars,
  migrationChartValue,
  tradeBarTime,
} from 'components/token-price-chart/chartBars';
import { BarCrosshairFields } from 'components/token-price-chart/BarCrosshairFields';
import {
  BuySellCountsIcon,
  CandlesIcon,
  IconToggleButton,
  LineIcon,
  RangeSelectIcon,
  TrimGapsIcon,
  WalletMarkersIcon,
} from 'components/token-price-chart/ChartToolbar';
import {
  CANDLE_SERIES_OPTIONS,
  CHART_COLORS,
  CHART_GROUP_MODES,
  CHART_GROUP_MODE_LABELS,
  CHART_INTERVAL_LABELS,
  CHART_INTERVALS,
  CHART_STYLES,
  createChartOptions,
  createChartPriceFormatter,
  LINE_SERIES_OPTIONS,
} from 'components/token-price-chart/constants';
import type {
  ChartBarSelection,
  ChartCrosshairInfo,
  ChartGroupMode,
  ChartInterval,
  ChartRangeSelection,
  ChartRangeStats,
  ChartRangeTooltipState,
  ChartStyle,
  ChartWalletMarkersTooltipState,
  OhlcBar,
  WalletBarActivity,
} from 'components/token-price-chart/types';
import {
  buildTradeMarkers,
  buildWalletBarActivityMap,
  buildWalletMarkerDefs,
  sortSeriesMarkers,
  type MarkersPlugin,
} from 'components/token-price-chart/TokenPriceChart';
import { RangeSelectPlugin, asRangePrimitive } from 'components/token-price-chart/rangeSelectPlugin';
import {
  WalletMarkersPlugin,
  asSeriesPrimitive as asWalletSeriesPrimitive,
} from 'components/token-price-chart/walletMarkersPlugin';
import { RangeSelectTooltip, formatRangeDuration } from 'components/token-price-chart/RangeSelectTooltip';
import { WalletMarkersTooltip } from 'components/token-price-chart/WalletMarkersTooltip';
import { DataTable } from 'components/table/DataTable';
import { tokenTradeColumns } from 'components/tokens/tokenTradeColumns';
import { Badge } from 'components/ui/Badge';
import { Checkbox } from 'components/ui/Checkbox';
import { cn } from 'lib/cn';
import { useTimezone } from 'context/TimezoneContext';
import { useProfileWallets } from 'hooks/useProfileWallets';
import { formatTimestampMs } from 'utils/date';
import { buildFlowLines, type FlowBasis, type FlowLinePoint } from '@lab/lib/flow/flowChartData';
import type { TradeRecord } from 'types';

/** Trades within the clicked bar — same bucket-key matching `TokenTradeChart`
 *  uses for its own bar-click selection. */
function tradesInBar(trades: TradeRecord[], bar: ChartBarSelection): TradeRecord[] {
  if (bar.groupMode === 'slot') {
    return trades.filter((t) => t.slot === bar.slot);
  }
  const intervalSec = bar.intervalSec ?? 60;
  return trades.filter((t) => tradeBarTime(t.block_time, intervalSec) === bar.barTime);
}

/** Non-vol overlay color — brown/amber, distinct from the buy/sell candle
 *  green/red pair (`CHART_COLORS.up`/`.down`) and from the vol-side green so
 *  it reads as a third signal. */
const NON_VOL_COLOR = '#c17a3a';

const BASIS_OPTIONS: { value: FlowBasis; label: string }[] = [
  { value: 'sol', label: 'SOL' },
  { value: 'token', label: 'Token' },
];

/** Segmented pill group — same visual language as the shared `ChartToolbar`
 *  (`CHART_COLORS.grid` track, `activePill` fill). */
function PillGroup<T extends string>({
  options,
  labels,
  value,
  onChange,
  disabled = false,
}: {
  options: readonly T[];
  labels: Record<T, string>;
  value: T;
  onChange: (v: T) => void;
  disabled?: boolean;
}) {
  return (
    <div
      className={cn('flex rounded-md p-0.5', disabled && 'opacity-40')}
      style={{ backgroundColor: CHART_COLORS.grid }}
    >
      {options.map((key) => (
        <button
          key={key}
          type="button"
          disabled={disabled}
          onClick={() => onChange(key)}
          className={cn(
            'rounded px-2 py-0.5 text-[11px] font-semibold transition-colors',
            value === key ? 'text-[#0a0a0a]' : 'hover:text-white',
            disabled && 'cursor-not-allowed hover:text-inherit',
          )}
          style={
            value === key
              ? { backgroundColor: CHART_COLORS.activePill }
              : { color: CHART_COLORS.panelTextDim }
          }
        >
          {labels[key]}
        </button>
      ))}
    </div>
  );
}

const STYLE_ICONS: Record<ChartStyle, ReactNode> = {
  candles: <CandlesIcon />,
  line: <LineIcon />,
};

/** Icon-pill group for the candle/line style toggle — same track/fill as
 *  {@link PillGroup} but icon children instead of text labels, matching the
 *  real `ChartToolbar`'s style buttons exactly. */
function StylePillGroup({
  value,
  onChange,
}: {
  value: ChartStyle;
  onChange: (v: ChartStyle) => void;
}) {
  return (
    <div className="flex rounded-md p-0.5" style={{ backgroundColor: CHART_COLORS.grid }}>
      {CHART_STYLES.map((key) => (
        <button
          key={key}
          type="button"
          onClick={() => onChange(key)}
          aria-label={`${key} chart`}
          aria-pressed={value === key}
          className={cn(
            'flex items-center justify-center rounded px-2.5 py-0.5 transition-colors',
            value === key ? 'text-[#0a0a0a]' : 'hover:text-white',
          )}
          style={
            value === key
              ? { backgroundColor: CHART_COLORS.activePill }
              : { color: CHART_COLORS.panelTextDim }
          }
        >
          {STYLE_ICONS[key]}
        </button>
      ))}
    </div>
  );
}

function formatAmount(v: number, unit: string): string {
  if (!Number.isFinite(v)) return '—';
  const a = Math.abs(v);
  const s = a >= 1000 ? v.toFixed(0) : a >= 1 ? v.toFixed(2) : v.toPrecision(2);
  return `${s}${unit}`;
}

/** Find the vol/non-vol point matching a bar's time key (both arrays share
 *  the exact same time sequence — see {@link buildFlowLines}). */
function flowAt(
  lines: { vol: FlowLinePoint[]; nonVol: FlowLinePoint[] },
  time: Time,
): { vol: number | null; nonVol: number | null } {
  const idx = lines.vol.findIndex((p) => p.time === time);
  if (idx === -1) return { vol: null, nonVol: null };
  return { vol: lines.vol[idx].value, nonVol: lines.nonVol[idx].value };
}

export interface FlowPreviewChartProps {
  trades: TradeRecord[];
  /** `JSON.stringify(labels)` keys of the checked volume_ix_patterns rows —
   *  redraws the two overlay lines whenever this set changes. */
  patternKeys: ReadonlySet<string>;
  /** Token creator wallet address, when known — offered as a toggle since the
   *  real classifier always treats the creator as volume-side. */
  creatorWallet?: string | null;
  /** ATH price (SOL) for the ATH reference line — same field the shared
   *  token detail chart uses. */
  athPriceInSol?: number | null;
  /** Drives the "Migration" reference line the same way the shared chart's
   *  toggle does (the line itself is a fixed pump.fun constant either way). */
  isMigrated?: boolean;
  height?: number;
}

/** Per-token chart with cumulative volume-maker (green) vs non-volume (brown)
 *  overlay lines that redraw instantly as `patternKeys` changes — a
 *  client-side preview of the live wallet-flow classifier (see
 *  `classifyFlow.ts`). Styled + featured like the shared token trade-history
 *  charts (full toolbar incl. trade/wallet markers, ATH/migration lines,
 *  range-select, O/H/L/C/Vol/Liq header, crosshair tooltip) but kept as its
 *  own bespoke component — not the shared `TokenPriceChart` — since this view
 *  is expected to diverge further for flow-discovery-specific needs. Reuses
 *  `TokenPriceChart`'s exported marker/plugin helpers rather than
 *  re-deriving them (SSOT). */
export function FlowPreviewChart({
  trades,
  patternKeys,
  creatorWallet,
  athPriceInSol = null,
  isMigrated = false,
  height = 320,
}: FlowPreviewChartProps) {
  const { timezone } = useTimezone();
  const profileWallets = useProfileWallets();
  const containerRef = useRef<HTMLDivElement | null>(null);
  const chartRef = useRef<IChartApi | null>(null);
  const mainSeriesRef = useRef<ISeriesApi<'Candlestick'> | ISeriesApi<'Line'> | null>(null);
  const volSeriesRef = useRef<ISeriesApi<'Line'> | null>(null);
  const nonVolSeriesRef = useRef<ISeriesApi<'Line'> | null>(null);
  const barsRef = useRef<OhlcBar[]>([]);
  const linesRef = useRef<{ vol: FlowLinePoint[]; nonVol: FlowLinePoint[] }>({ vol: [], nonVol: [] });
  const athLineRef = useRef<IPriceLine | null>(null);
  const migrationLineRef = useRef<IPriceLine | null>(null);
  const markersPluginRef = useRef<MarkersPlugin | null>(null);
  const walletMarkersPrimRef = useRef<WalletMarkersPlugin | null>(null);
  const walletActivityMapRef = useRef<Map<number, WalletBarActivity[]>>(new Map());
  const rangeSelectPrimRef = useRef<RangeSelectPlugin | null>(null);
  const rangeSelectModeRef = useRef(false);
  const selectedBarRef = useRef<ChartBarSelection | null>(null);

  const [style, setStyle] = useState<ChartStyle>('candles');
  const [groupMode, setGroupMode] = useState<ChartGroupMode>('time');
  const [interval, setInterval] = useState<ChartInterval>('1m');
  const [basis, setBasis] = useState<FlowBasis>('sol');
  const [trimEmptyBars, setTrimEmptyBars] = useState(false);
  const [seedCreatorAsVol, setSeedCreatorAsVol] = useState(false);
  const [showTradeMarkers, setShowTradeMarkers] = useState(true);
  const [showWalletMarkers, setShowWalletMarkers] = useState(true);
  const [showAthLine, setShowAthLine] = useState(true);
  const [showMigrationLine, setShowMigrationLine] = useState(true);
  const [rangeSelectMode, setRangeSelectMode] = useState(false);
  const [showMore, setShowMore] = useState(false);
  const [selectedRange, setSelectedRange] = useState<ChartRangeSelection | null>(null);
  const [selectedBar, setSelectedBar] = useState<ChartBarSelection | null>(null);

  const [crosshair, setCrosshair] = useState<ChartCrosshairInfo | null>(null);
  const [flowCrosshair, setFlowCrosshair] = useState<{ vol: number | null; nonVol: number | null } | null>(
    null,
  );
  const [tooltipPoint, setTooltipPoint] = useState<{ x: number; y: number } | null>(null);
  const [rangeTooltip, setRangeTooltip] = useState<ChartRangeTooltipState | null>(null);
  const [walletTooltip, setWalletTooltip] = useState<ChartWalletMarkersTooltipState | null>(null);

  const formatPrice = useMemo(() => createChartPriceFormatter('SOL'), []);
  const formatVol = useMemo(() => createChartPriceFormatter('SOL'), []);
  const toValue = (sol: number) => sol;

  const intervalSec = CHART_INTERVALS[interval];
  const intervalsDisabled = groupMode === 'slot';

  const sortedTrades = useMemo(() => [...trades].sort(compareTradesChronologically), [trades]);

  const bars = useMemo(() => {
    const raw =
      groupMode === 'slot'
        ? aggregateTradesToBarsBySlot(sortedTrades, toValue, 'price')
        : aggregateTradesToBars(sortedTrades, intervalSec, toValue, 'price');
    return trimEmptyBars ? dropEmptyBars(raw) : raw;
  }, [sortedTrades, groupMode, intervalSec, trimEmptyBars]);

  const classifyOpts = useMemo(
    () => ({ patternKeys, creatorWallet: seedCreatorAsVol ? creatorWallet : null }),
    [patternKeys, creatorWallet, seedCreatorAsVol],
  );

  const lines = useMemo(
    () => buildFlowLines(trades, groupMode, intervalSec, basis, classifyOpts),
    [trades, groupMode, intervalSec, basis, classifyOpts],
  );

  const rangeStats: ChartRangeStats | null = useMemo(() => {
    if (!selectedRange) return null;
    return computeRangeStats(sortedTrades, selectedRange, groupMode, intervalSec);
  }, [selectedRange, sortedTrades, groupMode, intervalSec]);

  useEffect(() => {
    barsRef.current = bars;
  }, [bars]);
  useEffect(() => {
    linesRef.current = lines;
  }, [lines]);
  useEffect(() => {
    rangeSelectModeRef.current = rangeSelectMode;
  }, [rangeSelectMode]);
  useEffect(() => {
    selectedBarRef.current = selectedBar;
  }, [selectedBar]);

  const selectionTrades = useMemo(
    () => (selectedBar ? tradesInBar(sortedTrades, selectedBar) : []),
    [sortedTrades, selectedBar],
  );

  // Create the chart + series + plugins on any structural change; everything
  // else (data, toggles) updates in place via the effects below.
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const chart = createChart(
      el,
      createChartOptions(el.clientWidth, height, groupMode, 'SOL', timezone),
    );
    chartRef.current = chart;

    const mainSeries =
      style === 'candles'
        ? chart.addSeries(CandlestickSeries, { ...CANDLE_SERIES_OPTIONS, priceScaleId: 'right' })
        : chart.addSeries(LineSeries, { ...LINE_SERIES_OPTIONS, priceScaleId: 'right' });
    mainSeriesRef.current = mainSeries;

    const walletPrim = new WalletMarkersPlugin();
    mainSeries.attachPrimitive(asWalletSeriesPrimitive(walletPrim));
    walletMarkersPrimRef.current = walletPrim;

    const rangePrim = new RangeSelectPlugin();
    mainSeries.attachPrimitive(asRangePrimitive(rangePrim));
    rangeSelectPrimRef.current = rangePrim;

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
      color: NON_VOL_COLOR,
      lineWidth: 2,
      priceScaleId: 'left',
      title: 'Non-vol',
      lastValueVisible: true,
      priceLineVisible: false,
    });
    nonVolSeriesRef.current = nonVolSeries;

    chart.priceScale('left').applyOptions({ visible: true, borderColor: CHART_COLORS.border });

    const handleCrosshairMove = (param: MouseEventParams) => {
      const onRangeLabel =
        param.point != null &&
        (rangeSelectPrimRef.current?.containsLabelPoint(param.point.x, param.point.y) ?? false);
      if (onRangeLabel && rangeStats && param.point) {
        setRangeTooltip({ stats: rangeStats, point: param.point });
        setWalletTooltip(null);
        setCrosshair(null);
        setFlowCrosshair(null);
        setTooltipPoint(null);
        return;
      }
      setRangeTooltip(null);

      if (!param.time) {
        setCrosshair(null);
        setFlowCrosshair(null);
        setTooltipPoint(null);
        setWalletTooltip(null);
        return;
      }
      const bar = barsRef.current.find((b) => b.time === param.time);
      if (!bar) {
        setCrosshair(null);
        setFlowCrosshair(null);
        setTooltipPoint(null);
        setWalletTooltip(null);
        return;
      }
      setCrosshair({
        open: bar.open,
        high: bar.high,
        low: bar.low,
        close: bar.close,
        volume: bar.volume,
        inflow: bar.inflow,
        outflow: bar.outflow,
        liquiditySol: bar.liquiditySol,
      });
      setFlowCrosshair(flowAt(linesRef.current, param.time));

      const onWalletMarker =
        param.point != null &&
        (walletMarkersPrimRef.current?.containsPoint(param.point.x, param.point.y) ?? false);
      if (onWalletMarker && param.point) {
        const activity = walletActivityMapRef.current.get(bar.time as number);
        setWalletTooltip(activity && activity.length > 0 ? { point: param.point, wallets: activity } : null);
      } else {
        setWalletTooltip(null);
      }
      setTooltipPoint(param.point ? { x: param.point.x, y: param.point.y } : null);
    };
    chart.subscribeCrosshairMove(handleCrosshairMove);

    const groupModeAtMount = groupMode;
    const intervalAtMount = intervalSec;
    chart.subscribeClick((param) => {
      // In range-select mode the pointer-drag handler owns clicks.
      if (rangeSelectModeRef.current) return;
      if (!param.time) {
        setSelectedBar(null);
        return;
      }
      if (selectedBarRef.current?.barTime === param.time) {
        setSelectedBar(null);
        return;
      }
      const selection: ChartBarSelection =
        groupModeAtMount === 'slot'
          ? { barTime: param.time as UTCTimestamp, groupMode: 'slot', slot: param.time as number }
          : { barTime: param.time as UTCTimestamp, groupMode: 'time', intervalSec: intervalAtMount };
      setSelectedBar(selection);
    });

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
      mainSeriesRef.current = null;
      volSeriesRef.current = null;
      nonVolSeriesRef.current = null;
      athLineRef.current = null;
      migrationLineRef.current = null;
      markersPluginRef.current = null;
      walletMarkersPrimRef.current = null;
      rangeSelectPrimRef.current = null;
      setCrosshair(null);
      setFlowCrosshair(null);
      setTooltipPoint(null);
      setRangeTooltip(null);
      setWalletTooltip(null);
      setSelectedBar(null);
    };
    // rangeStats intentionally omitted: the range-label tooltip reads the
    // latest value via closure-fresh state is unnecessary here since it's
    // only consulted on hover, and re-running this effect would tear down
    // the whole chart on every drag-stats recompute. `intervalSec` IS a dep
    // (mirrors `TokenPriceChart`'s `groupingKey`) — the click handler below
    // closes over `intervalAtMount`, which must be rebuilt whenever the
    // bucket width changes or `tradesInBar` filters against the wrong width.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [style, groupMode, intervalSec, height, timezone]);

  useEffect(() => {
    if (style === 'candles') {
      const highlight = selectedBar ? new Set([selectedBar.barTime as number]) : undefined;
      (mainSeriesRef.current as ISeriesApi<'Candlestick'> | null)?.setData(
        barsToCandleData(bars, highlight),
      );
    } else {
      (mainSeriesRef.current as ISeriesApi<'Line'> | null)?.setData(barsToLineData(bars));
    }
  }, [bars, style, selectedBar]);

  useEffect(() => {
    volSeriesRef.current?.setData(lines.vol);
    nonVolSeriesRef.current?.setData(lines.nonVol);
  }, [lines]);

  // Trade (buy/sell count) markers + the selected-bar marker.
  useEffect(() => {
    const series = mainSeriesRef.current;
    if (!series) return;
    const markers: SeriesMarker<UTCTimestamp>[] = showTradeMarkers
      ? buildTradeMarkers(sortedTrades, groupMode, intervalSec)
      : [];
    if (selectedBar) {
      const bar = bars.find((b) => b.time === selectedBar.barTime);
      if (bar) markers.push(barSelectionMarker(bar));
    }
    const sorted = sortSeriesMarkers(markers);
    markersPluginRef.current?.detach();
    markersPluginRef.current = null;
    if (sorted.length > 0) {
      markersPluginRef.current = createSeriesMarkers(series, sorted) as MarkersPlugin;
    }
  }, [showTradeMarkers, sortedTrades, groupMode, intervalSec, bars, style, selectedBar]);

  // Tracked-wallet markers.
  useEffect(() => {
    if (showWalletMarkers && profileWallets.length > 0) {
      const defs = buildWalletMarkerDefs(sortedTrades, profileWallets, bars, groupMode, intervalSec);
      walletMarkersPrimRef.current?.setMarkers(defs);
      walletActivityMapRef.current = buildWalletBarActivityMap(
        sortedTrades,
        profileWallets,
        groupMode,
        intervalSec,
      );
    } else {
      walletMarkersPrimRef.current?.setMarkers([]);
      walletActivityMapRef.current = new Map();
    }
  }, [showWalletMarkers, sortedTrades, profileWallets, bars, groupMode, intervalSec, style]);

  // ATH reference line.
  useEffect(() => {
    const series = mainSeriesRef.current;
    if (!series) return;
    if (athLineRef.current) {
      series.removePriceLine(athLineRef.current);
      athLineRef.current = null;
    }
    if (!showAthLine || athPriceInSol == null) return;
    const price = athChartValue(athPriceInSol, 'price', toValue);
    if (price == null) return;
    athLineRef.current = series.createPriceLine({
      price,
      color: CHART_COLORS.athLine,
      lineWidth: 1,
      lineStyle: LineStyle.Dashed,
      axisLabelVisible: true,
      title: 'ATH',
    });
  }, [showAthLine, athPriceInSol, style]);

  // Pump.fun migration reference line.
  useEffect(() => {
    const series = mainSeriesRef.current;
    if (!series) return;
    if (migrationLineRef.current) {
      series.removePriceLine(migrationLineRef.current);
      migrationLineRef.current = null;
    }
    if (!showMigrationLine) return;
    const price = migrationChartValue('price', toValue);
    migrationLineRef.current = series.createPriceLine({
      price,
      color: CHART_COLORS.migrationLine,
      lineWidth: 1,
      lineStyle: LineStyle.Dashed,
      axisLabelVisible: true,
      title: 'Migration',
    });
  }, [showMigrationLine, style]);

  // Render the committed range band + duration chip.
  useEffect(() => {
    const prim = rangeSelectPrimRef.current;
    if (!prim) return;
    if (!selectedRange) {
      prim.setBand(null);
      return;
    }
    const label = rangeStats ? formatRangeDuration(rangeStats.durationMs) : 'Range';
    prim.setBand({
      loTime: Math.min(selectedRange.lo, selectedRange.hi) as UTCTimestamp,
      hiTime: Math.max(selectedRange.lo, selectedRange.hi) as UTCTimestamp,
      label,
      dashed: false,
    });
  }, [selectedRange, rangeStats, style, groupMode]);

  // Drag-to-select a time range (range-select mode only).
  useEffect(() => {
    if (!rangeSelectMode) return;
    const el = containerRef.current;
    const chart = chartRef.current;
    if (!el || !chart) return;

    chart.applyOptions({ handleScroll: false, handleScale: false });
    el.style.cursor = 'crosshair';

    const coordToBarTime = (clientX: number): number | null => {
      const chartBars = barsRef.current;
      if (chartBars.length === 0) return null;
      const rect = el.getBoundingClientRect();
      const logical = chart.timeScale().coordinateToLogical((clientX - rect.left) as Coordinate);
      if (logical == null) return null;
      const idx = Math.max(0, Math.min(chartBars.length - 1, Math.round(logical)));
      return chartBars[idx].time as number;
    };

    let dragging = false;
    let startX = 0;
    let startTime: number | null = null;

    const onPointerDown = (e: PointerEvent) => {
      if (e.button !== 0) return;
      const t = coordToBarTime(e.clientX);
      if (t == null) return;
      dragging = true;
      startX = e.clientX;
      startTime = t;
      try {
        el.setPointerCapture(e.pointerId);
      } catch {
        /* ignore */
      }
      rangeSelectPrimRef.current?.setBand({ loTime: t as UTCTimestamp, hiTime: t as UTCTimestamp, dashed: true });
    };

    const onPointerMove = (e: PointerEvent) => {
      if (!dragging || startTime == null) return;
      const t = coordToBarTime(e.clientX);
      if (t == null) return;
      rangeSelectPrimRef.current?.setBand({
        loTime: Math.min(startTime, t) as UTCTimestamp,
        hiTime: Math.max(startTime, t) as UTCTimestamp,
        dashed: true,
      });
    };

    const finishDrag = (e: PointerEvent) => {
      if (!dragging) return;
      dragging = false;
      try {
        el.releasePointerCapture(e.pointerId);
      } catch {
        /* ignore */
      }
      const t = coordToBarTime(e.clientX);
      if (startTime == null || t == null || Math.abs(e.clientX - startX) < 4) {
        startTime = null;
        rangeSelectPrimRef.current?.setBand(null);
        setSelectedRange(null);
        return;
      }
      setSelectedRange({ lo: Math.min(startTime, t), hi: Math.max(startTime, t) });
      startTime = null;
    };

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setSelectedRange(null);
    };

    el.addEventListener('pointerdown', onPointerDown);
    el.addEventListener('pointermove', onPointerMove);
    el.addEventListener('pointerup', finishDrag);
    el.addEventListener('pointercancel', finishDrag);
    window.addEventListener('keydown', onKeyDown);

    return () => {
      el.removeEventListener('pointerdown', onPointerDown);
      el.removeEventListener('pointermove', onPointerMove);
      el.removeEventListener('pointerup', finishDrag);
      el.removeEventListener('pointercancel', finishDrag);
      window.removeEventListener('keydown', onKeyDown);
      el.style.cursor = '';
      if (chartRef.current === chart) {
        chart.applyOptions({ handleScroll: true, handleScale: true });
      }
    };
  }, [rangeSelectMode, style, groupMode, intervalSec, height, timezone]);

  const unit = basis === 'token' ? '' : '◎';
  const readVol = flowCrosshair?.vol ?? lines.vol.at(-1)?.value ?? null;
  const readNonVol = flowCrosshair?.nonVol ?? lines.nonVol.at(-1)?.value ?? null;
  const moreActive = showMore || showAthLine || showMigrationLine || rangeSelectMode;

  const tradeColumns = useMemo(() => tokenTradeColumns('SOL'), []);
  const selectionLabel = selectedBar
    ? selectedBar.groupMode === 'slot'
      ? `Slot ${selectedBar.slot}`
      : formatTimestampMs(Number(selectedBar.barTime) * 1000, timezone)
    : '';

  return (
    <div className="flex flex-col gap-2">
      {/* Toolbar */}
      <div
        className="flex flex-wrap items-center gap-2 border-b px-1 pb-2"
        style={{ borderColor: CHART_COLORS.border }}
      >
        <PillGroup
          options={CHART_GROUP_MODES}
          labels={CHART_GROUP_MODE_LABELS}
          value={groupMode}
          onChange={setGroupMode}
        />
        <PillGroup
          options={CHART_INTERVAL_LABELS}
          labels={Object.fromEntries(CHART_INTERVAL_LABELS.map((i) => [i, i])) as Record<
            ChartInterval,
            string
          >}
          value={interval}
          onChange={setInterval}
          disabled={intervalsDisabled}
        />
        <StylePillGroup value={style} onChange={setStyle} />
        <PillGroup
          options={BASIS_OPTIONS.map((o) => o.value)}
          labels={Object.fromEntries(BASIS_OPTIONS.map((o) => [o.value, o.label])) as Record<
            FlowBasis,
            string
          >}
          value={basis}
          onChange={setBasis}
        />

        <IconToggleButton
          active={showTradeMarkers}
          onClick={() => setShowTradeMarkers((v) => !v)}
          label="Toggle buy/sell counts per bar"
          tooltip="Buy/sell counts per bar"
        >
          <BuySellCountsIcon />
        </IconToggleButton>

        <IconToggleButton
          active={showWalletMarkers}
          onClick={() => setShowWalletMarkers((v) => !v)}
          label="Toggle tracked-wallet markers"
          tooltip="Tracked-wallet buy/sell markers"
        >
          <WalletMarkersIcon />
        </IconToggleButton>

        <IconToggleButton
          active={trimEmptyBars}
          onClick={() => setTrimEmptyBars((v) => !v)}
          label="Toggle trimming of empty bars"
          tooltip="Hide flat bars for intervals with no trades"
        >
          <TrimGapsIcon />
        </IconToggleButton>

        <button
          type="button"
          onClick={() => setShowMore((v) => !v)}
          aria-expanded={showMore}
          className={cn(
            'rounded-md px-2 py-1 text-[11px] font-semibold transition-colors',
            moreActive ? 'text-[#0a0a0a]' : 'hover:text-white',
          )}
          style={
            moreActive
              ? { backgroundColor: CHART_COLORS.activePill }
              : { backgroundColor: CHART_COLORS.grid, color: CHART_COLORS.panelTextDim }
          }
        >
          More
        </button>
      </div>

      {showMore && (
        <div className="flex flex-wrap items-center gap-2 px-1">
          <label
            className="flex cursor-pointer items-center gap-1.5 rounded-md px-2 py-1 text-[11px] font-semibold"
            style={{ backgroundColor: CHART_COLORS.grid, color: CHART_COLORS.panelTextDim }}
            title={athPriceInSol != null ? 'Show all-time high price line' : 'No ATH price recorded for this token'}
          >
            <Checkbox
              boxSize="sm"
              checked={showAthLine}
              disabled={athPriceInSol == null}
              onChange={(e) => setShowAthLine(e.target.checked)}
            />
            <span style={showAthLine && athPriceInSol != null ? { color: CHART_COLORS.athLine } : undefined}>
              ATH
            </span>
          </label>

          <label
            className="flex cursor-pointer items-center gap-1.5 rounded-md px-2 py-1 text-[11px] font-semibold"
            style={{ backgroundColor: CHART_COLORS.grid, color: CHART_COLORS.panelTextDim }}
            title="Show pump.fun bonding-curve graduation price"
          >
            <Checkbox
              boxSize="sm"
              checked={showMigrationLine}
              onChange={(e) => setShowMigrationLine(e.target.checked)}
            />
            <span style={showMigrationLine ? { color: CHART_COLORS.migrationLine } : undefined}>
              Migration{isMigrated ? ' ✓' : ''}
            </span>
          </label>

          <IconToggleButton
            active={rangeSelectMode}
            onClick={() => setRangeSelectMode((v) => !v)}
            label="Toggle range-select mode"
            tooltip="Drag to select a time range; hover its label for totals"
          >
            <RangeSelectIcon />
          </IconToggleButton>

          <label
            className={cn(
              'flex cursor-pointer items-center gap-1.5 rounded-md px-2 py-1 text-[11px] font-semibold',
              !creatorWallet && 'cursor-not-allowed opacity-40',
            )}
            style={{ backgroundColor: CHART_COLORS.grid, color: CHART_COLORS.panelTextDim }}
            title={
              creatorWallet
                ? 'Treat the token creator wallet as always volume-side (mirrors the live classifier)'
                : 'No creator wallet known for this token'
            }
          >
            <Checkbox
              boxSize="sm"
              checked={seedCreatorAsVol}
              disabled={!creatorWallet}
              onChange={(e) => setSeedCreatorAsVol(e.target.checked)}
            />
            Seed creator as vol
          </label>
        </div>
      )}

      {/* Header: O/H/L/C/Vol/Liq + flow-split readout */}
      <div className="flex flex-wrap items-center gap-x-4 gap-y-1 px-1 font-mono text-[11px]">
        {crosshair ? (
          <BarCrosshairFields
            style={style}
            crosshair={crosshair}
            formatPrice={formatPrice}
            formatVol={formatVol}
            layout="inline"
          />
        ) : (
          <span className="text-text-dim/50">hover the chart for O/H/L/C/Vol/Liq</span>
        )}
        <span style={{ color: CHART_COLORS.up }}>
          <span className="font-semibold">VolMk</span>{' '}
          {readVol != null ? formatAmount(readVol, unit) : '—'}
        </span>
        <span style={{ color: NON_VOL_COLOR }}>
          <span className="font-semibold">NonVol</span>{' '}
          {readNonVol != null ? formatAmount(readNonVol, unit) : '—'}
        </span>
      </div>

      <div className="relative">
        <div ref={containerRef} style={{ height }} />

        {rangeTooltip && (
          <RangeSelectTooltip
            tooltip={rangeTooltip}
            formatAmount={(sol) => formatVol(sol)}
            formatPrice={(p) => formatPrice(p)}
          />
        )}
        {!rangeTooltip && walletTooltip && <WalletMarkersTooltip tooltip={walletTooltip} />}
        {!rangeTooltip && !walletTooltip && tooltipPoint && crosshair && (
          <div
            className="pointer-events-none absolute z-20 max-w-55 rounded-md border px-2.5 py-2 font-mono text-[10px] leading-snug shadow-lg"
            style={{
              left: tooltipPoint.x + 14,
              top: Math.max(0, tooltipPoint.y - 10),
              borderColor: CHART_COLORS.crosshair,
              backgroundColor: '#0d0d0df0',
              color: CHART_COLORS.panelText,
            }}
          >
            <BarCrosshairFields
              style={style}
              crosshair={crosshair}
              formatPrice={formatPrice}
              formatVol={formatVol}
              layout="grid"
            />
            <div className="mt-1 grid grid-cols-[auto_1fr] gap-x-2 gap-y-0.5">
              <span className="font-semibold" style={{ color: CHART_COLORS.up }}>
                VolMk
              </span>
              <span style={{ color: CHART_COLORS.up }}>
                {readVol != null ? formatAmount(readVol, unit) : '—'}
              </span>
              <span className="font-semibold" style={{ color: NON_VOL_COLOR }}>
                NonVol
              </span>
              <span style={{ color: NON_VOL_COLOR }}>
                {readNonVol != null ? formatAmount(readNonVol, unit) : '—'}
              </span>
            </div>
          </div>
        )}
      </div>

      {selectedBar && (
        <div className="border-t border-white/7 pt-2">
          <div className="mb-2 flex flex-wrap items-center gap-2">
            <span className="text-[9px] font-bold uppercase tracking-widest text-text-dim">
              Bar Trades
            </span>
            <span className="font-mono text-[11px] text-text-dim">{selectionLabel}</span>
            <Badge variant="primary" className="font-mono font-normal">
              {selectionTrades.length} trade{selectionTrades.length === 1 ? '' : 's'}
            </Badge>
            <button
              type="button"
              onClick={() => setSelectedBar(null)}
              className="text-[11px] text-text-dim hover:text-text"
            >
              Clear
            </button>
          </div>
          <DataTable
            columns={tradeColumns}
            rows={selectionTrades}
            rowKey={(t) => t.id}
            searchable
            colFilters
            hoverable
            emptyMessage="No trades in this bar."
          />
        </div>
      )}
    </div>
  );
}
