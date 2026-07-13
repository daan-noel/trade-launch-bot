import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  createChart,
  createSeriesMarkers,
  LineStyle,
  type Coordinate,
  type IChartApi,
  type IPriceLine,
  type ISeriesApi,
  type LogicalRangeChangeEventHandler,
  type SeriesMarker,
  type Time,
  type UTCTimestamp,
} from 'lightweight-charts';
import {
  barsSignature,
  captureChartViewport,
  reapplyChartViewport,
} from './chartViewport';
import {
  aggregateTradesToBars,
  aggregateTradesToBarsBySlot,
  athChartValue,
  migrationChartValue,
  barsToCandleData,
  barsToLineData,
  barSelectionMarker,
  compareTradesChronologically,
  computeRangeStats,
  dropEmptyBars,
  tradeBarSlot,
  tradeBarTime,
} from './chartBars';
import { ChartRangeSlider } from './ChartRangeSlider';
import { ChartToolbar } from './ChartToolbar';
import { createChartTimeFormatters, getDefaultChartTimezone } from './chartTimezone';
import { cn } from '@shared/lib/cn';
import {
  CANDLE_SERIES_OPTIONS,
  CHART_COLORS,
  CHART_INTERVALS,
  createChartOptions,
  createChartPriceFormat,
  createChartPriceFormatter,
  DEFAULT_CHART_PREFS,
  LINE_SERIES_OPTIONS,
  LS_CHART_PREFS_KEY,
  SERIES_BY_STYLE,
  TOKEN_TOTAL_SUPPLY,
} from './constants';
import { getString, setString } from '@shared/lib/storage';
import { BarCrosshairTooltip } from './BarCrosshairTooltip';
import { WalletMarkersTooltip } from './WalletMarkersTooltip';
import { RangeSelectTooltip, formatRangeDuration } from './RangeSelectTooltip';
import { WalletMarkersPlugin, asSeriesPrimitive, type WalletMarkerDef, type MarkerShape } from './walletMarkersPlugin';
import { RangeSelectPlugin, asRangePrimitive } from './rangeSelectPlugin';
import type {
  ChartBarSelection,
  ChartCrosshairInfo,
  ChartGroupMode,
  ChartInterval,
  ChartStyle,
  ChartBarTooltipState,
  ChartEventMarker,
  ChartRangeSelection,
  ChartRangeTooltipState,
  ChartTrade,
  ChartWalletMarkersTooltipState,
  OhlcBar,
  ProfileWalletInfo,
  TokenPriceChartProps,
  WalletBarActivity,
} from './types';

const EMPTY_PROFILE_WALLETS: ProfileWalletInfo[] = [];

type ChartPrefs = {
  groupMode: ChartGroupMode;
  interval: ChartInterval;
  style: ChartStyle;
  showTradeMarkers: boolean;
  showMigrationLine: boolean;
  trimEmptyBars: boolean;
};

const DEFAULT_PREFS: ChartPrefs = {
  groupMode: DEFAULT_CHART_PREFS.groupMode,
  interval: DEFAULT_CHART_PREFS.interval,
  style: DEFAULT_CHART_PREFS.style,
  showTradeMarkers: DEFAULT_CHART_PREFS.showTradeMarkers,
  showMigrationLine: DEFAULT_CHART_PREFS.showMigrationLine,
  trimEmptyBars: DEFAULT_CHART_PREFS.trimEmptyBars,
};

function loadPrefs(): ChartPrefs {
  try {
    const raw = getString(LS_CHART_PREFS_KEY);
    if (raw) {
      const parsed = JSON.parse(raw) as Partial<ChartPrefs>;
      return { ...DEFAULT_PREFS, ...parsed };
    }
  } catch {
    /* ignore */
  }
  return DEFAULT_PREFS;
}

function savePrefs(prefs: ChartPrefs) {
  setString(LS_CHART_PREFS_KEY, JSON.stringify(prefs));
}

function buildTradeMarkers(
  trades: ChartTrade[],
  groupMode: ChartGroupMode,
  intervalSec: number,
): SeriesMarker<UTCTimestamp>[] {
  const countsByBar = new Map<UTCTimestamp, { buy: number; sell: number }>();
  for (const trade of trades) {
    const time =
      groupMode === 'slot'
        ? tradeBarSlot(trade)
        : tradeBarTime(trade.block_time, intervalSec);
    if (time == null) continue;
    const bucket = countsByBar.get(time) ?? { buy: 0, sell: 0 };
    if (trade.trade_type === 'buy') bucket.buy += 1;
    else bucket.sell += 1;
    countsByBar.set(time, bucket);
  }

  const markers: SeriesMarker<UTCTimestamp>[] = [];
  for (const [time, { buy, sell }] of countsByBar) {
    if (buy === 0 && sell === 0) continue;

    const textParts: string[] = [];
    if (buy > 0) textParts.push(`↑${buy}`);
    if (sell > 0) textParts.push(`↓${sell}`);

    const onlyBuy = buy > 0 && sell === 0;
    const onlySell = sell > 0 && buy === 0;

    markers.push({
      time,
      position: onlyBuy ? 'belowBar' : onlySell ? 'aboveBar' : 'inBar',
      color: onlyBuy ? CHART_COLORS.up : onlySell ? CHART_COLORS.down : CHART_COLORS.text,
      shape: onlyBuy ? 'arrowUp' : onlySell ? 'arrowDown' : 'circle',
      text: textParts.join(' '),
    });
  }
  return markers;
}

/** Resolve a strategy entry/exit point to the bar it belongs on. Prefers the
 *  exact bar of the matching tx; otherwise buckets the event time (time mode) or
 *  snaps to the nearest trade's slot (slot mode), then snaps to the closest real
 *  bar so the marker always lands on rendered data. */
function resolveEventBarTime(
  marker: ChartEventMarker,
  sortedTrades: ChartTrade[],
  bars: OhlcBar[],
  groupMode: ChartGroupMode,
  intervalSec: number,
): UTCTimestamp | null {
  if (bars.length === 0) return null;
  const eventMs = Date.parse(marker.time);

  let candidate: number | null = null;

  if (marker.txSignature) {
    const hit = sortedTrades.find((t) => t.tx_signature === marker.txSignature);
    if (hit) {
      const key =
        groupMode === 'slot' ? tradeBarSlot(hit) : tradeBarTime(hit.block_time, intervalSec);
      if (key != null) candidate = key as number;
    }
  }

  if (candidate == null && !Number.isNaN(eventMs)) {
    if (groupMode === 'slot') {
      let best: { slot: number; dist: number } | null = null;
      for (const t of sortedTrades) {
        if (t.slot == null) continue;
        const ms = Date.parse(t.block_time);
        if (Number.isNaN(ms)) continue;
        const dist = Math.abs(ms - eventMs);
        if (!best || dist < best.dist) best = { slot: t.slot, dist };
      }
      if (best) candidate = best.slot;
    } else {
      const key = tradeBarTime(marker.time, intervalSec);
      if (key != null) candidate = key as number;
    }
  }

  if (candidate == null) return null;

  let nearest = bars[0].time as number;
  let nearestDist = Math.abs(nearest - candidate);
  for (const b of bars) {
    const d = Math.abs((b.time as number) - candidate);
    if (d < nearestDist) {
      nearest = b.time as number;
      nearestDist = d;
    }
  }
  return nearest as UTCTimestamp;
}

function buildEventSeriesMarkers(
  eventMarkers: ChartEventMarker[],
  sortedTrades: ChartTrade[],
  bars: OhlcBar[],
  groupMode: ChartGroupMode,
  intervalSec: number,
): SeriesMarker<UTCTimestamp>[] {
  const out: SeriesMarker<UTCTimestamp>[] = [];
  for (const m of eventMarkers) {
    const time = resolveEventBarTime(m, sortedTrades, bars, groupMode, intervalSec);
    if (time == null) continue;
    const isEntry = m.kind === 'entry';
    out.push({
      time,
      position: isEntry ? 'belowBar' : 'aboveBar',
      color: isEntry ? CHART_COLORS.entry : CHART_COLORS.exit,
      shape: isEntry ? 'arrowUp' : 'arrowDown',
      text: m.label ?? (isEntry ? 'Entry' : 'Exit'),
      size: 2,
    });
  }
  return out;
}

type MarkersPlugin = {
  setMarkers: (markers: SeriesMarker<UTCTimestamp>[]) => void;
  detach: () => void;
};

function sortSeriesMarkers(
  markers: SeriesMarker<UTCTimestamp>[],
): SeriesMarker<UTCTimestamp>[] {
  return [...markers].sort((a, b) => (a.time as number) - (b.time as number));
}

/** Silhouette per wallet CLASS: `mine` → diamond (identity wins), else
 *  focused → hexagon, else circle. */
function walletShape(w: ProfileWalletInfo): MarkerShape {
  if (w.isMine) return 'diamond';
  if (w.isHighlighted) return 'hexagon';
  return 'circle';
}

function walletGlyph(w: ProfileWalletInfo): string {
  return w.isMine ? '★' : (w.profileName ?? w.label ?? w.address).charAt(0).toUpperCase();
}

/** Below this fraction of the episode's peak balance, a sell is treated as a full
 *  exit (fee/rounding dust rarely leaves the balance at exactly zero). */
const SELL_ALL_DUST_FRACTION = 0.02;

function buildWalletMarkerDefs(
  sortedTrades: ChartTrade[],
  profileWallets: ProfileWalletInfo[],
  bars: OhlcBar[],
  groupMode: ChartGroupMode,
  intervalSec: number,
): WalletMarkerDef[] {
  const walletMap = new Map(profileWallets.map((w) => [w.address, w]));
  const barMap = new Map(bars.map((b) => [b.time as number, b]));

  const groups = new Map<number, { buy: ProfileWalletInfo[]; sell: ProfileWalletInfo[] }>();
  const seen = new Set<string>();

  const roles = new Map<string, 'first_buy' | 'sell_all'>();
  const pos = new Map<string, number>();
  const peak = new Map<string, number>();
  const firstBuyDone = new Set<string>();

  for (const trade of sortedTrades) {
    if (!trade.wallet_address) continue;
    const wallet = walletMap.get(trade.wallet_address);
    if (!wallet) continue;
    const time =
      groupMode === 'slot'
        ? tradeBarSlot(trade)
        : tradeBarTime(trade.block_time, intervalSec);
    if (time == null) continue;
    const t = time as number;
    const addr = trade.wallet_address;
    const type = trade.trade_type === 'buy' ? 'buy' : 'sell';

    const amt = trade.token_amount ?? 0;
    if (type === 'buy') {
      if (!firstBuyDone.has(addr)) {
        firstBuyDone.add(addr);
        roles.set(`${addr}:${t}:buy`, 'first_buy');
      }
      const next = (pos.get(addr) ?? 0) + amt;
      pos.set(addr, next);
      peak.set(addr, Math.max(peak.get(addr) ?? 0, next));
    } else {
      const next = Math.max(0, (pos.get(addr) ?? 0) - amt);
      pos.set(addr, next);
      const pk = peak.get(addr) ?? 0;
      if (pk > 0 && next <= pk * SELL_ALL_DUST_FRACTION) {
        roles.set(`${addr}:${t}:sell`, 'sell_all');
        peak.set(addr, 0);
        pos.set(addr, 0);
      }
    }

    const key = `${addr}:${t}:${type}`;
    if (seen.has(key)) continue;
    seen.add(key);
    let group = groups.get(t);
    if (!group) { group = { buy: [], sell: [] }; groups.set(t, group); }
    group[type].push(wallet);
  }

  const defs: WalletMarkerDef[] = [];
  for (const [t, { buy, sell }] of groups) {
    const bar = barMap.get(t);
    if (!bar) continue;
    const barTime = t as UTCTimestamp;

    let buyStack = 0;
    for (const w of buy) {
      defs.push({
        barTime,
        barEdgePrice: bar.low,
        letter: walletGlyph(w),
        color: w.color,
        borderColor: CHART_COLORS.up,
        type: 'buy',
        stackIndex: buyStack++,
        shape: walletShape(w),
        role: roles.get(`${w.address}:${t}:buy`),
        highlighted: w.isHighlighted,
        ringColor: CHART_COLORS.highlightRing,
      });
    }
    let sellStack = 0;
    for (const w of sell) {
      defs.push({
        barTime,
        barEdgePrice: bar.high,
        letter: walletGlyph(w),
        color: w.color,
        borderColor: CHART_COLORS.down,
        type: 'sell',
        stackIndex: sellStack++,
        shape: walletShape(w),
        role: roles.get(`${w.address}:${t}:sell`),
        highlighted: w.isHighlighted,
        ringColor: CHART_COLORS.highlightRing,
      });
    }
  }
  return defs;
}

function buildWalletBarActivityMap(
  trades: ChartTrade[],
  profileWallets: ProfileWalletInfo[],
  groupMode: ChartGroupMode,
  intervalSec: number,
): Map<number, WalletBarActivity[]> {
  const walletMap = new Map(profileWallets.map((w) => [w.address, w]));
  const byBar = new Map<number, Map<string, WalletBarActivity>>();

  for (const trade of trades) {
    if (!trade.wallet_address) continue;
    const wallet = walletMap.get(trade.wallet_address);
    if (!wallet) continue;
    const time =
      groupMode === 'slot'
        ? tradeBarSlot(trade)
        : tradeBarTime(trade.block_time, intervalSec);
    if (time == null) continue;
    const t = time as number;
    let barMap = byBar.get(t);
    if (!barMap) { barMap = new Map(); byBar.set(t, barMap); }
    let activity = barMap.get(trade.wallet_address);
    if (!activity) {
      activity = { wallet, buyCount: 0, sellCount: 0, buySol: 0, sellSol: 0 };
      barMap.set(trade.wallet_address, activity);
    }
    if (trade.trade_type === 'buy') {
      activity.buyCount += 1;
      activity.buySol += trade.amount_sol ?? 0;
    } else {
      activity.sellCount += 1;
      activity.sellSol += trade.amount_sol ?? 0;
    }
  }

  const result = new Map<number, WalletBarActivity[]>();
  for (const [barTime, walletActivities] of byBar) {
    result.set(barTime, [...walletActivities.values()]);
  }
  return result;
}

function panelClass(className?: string) {
  return cn('rounded-lg border', className);
}

const panelStyle = {
  borderColor: CHART_COLORS.border,
  backgroundColor: CHART_COLORS.background,
};

function Placeholder({
  message,
  className,
  height = 320,
}: {
  message: string;
  className?: string;
  height?: number;
}) {
  return (
    <div
      className={panelClass(cn('flex items-center justify-center text-xs', className))}
      style={{ ...panelStyle, height, color: CHART_COLORS.panelTextDim }}
    >
      {message}
    </div>
  );
}

export function TokenPriceChart({
  symbol,
  id = '',
  trades,
  loading = false,
  error = null,
  toValue: toValueProp,
  priceLabel = 'SOL',
  priceUnit = 'SOL',
  metric = 'price',
  onMetricChange,
  className,
  height = 320,
  onBarClick,
  selectedBar = null,
  onRangeChange,
  isMigrated,
  profileWallets,
  tokenCreatedAt,
  eventMarkers = null,
}: TokenPriceChartProps) {
  const effectiveProfileWallets = profileWallets ?? EMPTY_PROFILE_WALLETS;

  const containerRef = useRef<HTMLDivElement>(null);
  const chartRef = useRef<IChartApi | null>(null);
  const onBarClickRef = useRef(onBarClick);
  onBarClickRef.current = onBarClick;
  const onRangeChangeRef = useRef(onRangeChange);
  onRangeChangeRef.current = onRangeChange;
  const seriesRef = useRef<ISeriesApi<'Line' | 'Candlestick'> | null>(null);
  const sortedTradesRef = useRef<ChartTrade[]>([]);
  const markersPluginRef = useRef<MarkersPlugin | null>(null);
  const walletMarkersPrimRef = useRef<WalletMarkersPlugin | null>(null);
  const rangeSelectPrimRef = useRef<RangeSelectPlugin | null>(null);
  const barsRef = useRef<OhlcBar[]>([]);

  const initialPrefs = loadPrefs();
  const [groupMode, setGroupMode] = useState<ChartGroupMode>(initialPrefs.groupMode);
  const [interval, setInterval] = useState<ChartInterval>(initialPrefs.interval);
  const [style, setStyle] = useState<ChartStyle>(initialPrefs.style);
  const [showTradeMarkers, setShowTradeMarkers] = useState(initialPrefs.showTradeMarkers);
  const [showMigrationLine, setShowMigrationLine] = useState(initialPrefs.showMigrationLine);
  const [trimEmptyBars, setTrimEmptyBars] = useState(initialPrefs.trimEmptyBars);
  // Forge has no timezone preference — render in the browser's local zone.
  const chartTimezone = useMemo(() => getDefaultChartTimezone(), []);
  const [rangeSelectMode, setRangeSelectMode] = useState(false);
  const [selectedRange, setSelectedRange] = useState<ChartRangeSelection | null>(null);
  const [crosshair, setCrosshair] = useState<ChartCrosshairInfo | null>(null);
  const [barTooltip, setBarTooltip] = useState<ChartBarTooltipState | null>(null);
  const [rangeTooltip, setRangeTooltip] = useState<ChartRangeTooltipState | null>(null);
  const [walletMarkersTooltip, setWalletMarkersTooltip] =
    useState<ChartWalletMarkersTooltipState | null>(null);
  /** Visible window mirrored from the chart's time scale, drives the range slider. */
  const [sliderWindow, setSliderWindow] = useState<{ from: number; to: number } | null>(null);
  const walletActivityMapRef = useRef<Map<number, WalletBarActivity[]>>(new Map());
  const styleRef = useRef(style);
  styleRef.current = style;
  const migrationLineRef = useRef<IPriceLine | null>(null);
  const eventLineRefs = useRef<IPriceLine[]>([]);

  const toValue = useCallback(
    (sol: number) => (toValueProp ? toValueProp(sol) : sol),
    [toValueProp],
  );

  const intervalSec = CHART_INTERVALS[interval];
  const groupingKey = groupMode === 'slot' ? 'slot' : intervalSec;
  const selectedBarTime = selectedBar?.barTime ?? null;
  const selectedBarTimeRef = useRef(selectedBarTime);
  selectedBarTimeRef.current = selectedBarTime;

  const shouldFitContentRef = useRef(true);
  const prevIdRef = useRef(id);
  const prevGroupingKeyRef = useRef(groupingKey);
  const visibleViewportRef = useRef<ReturnType<typeof captureChartViewport> | null>(null);
  const isRestoringViewportRef = useRef(false);
  const mountedSeriesStyleRef = useRef<ChartStyle | null>(null);
  const prevBarsSignatureRef = useRef<string | null>(null);
  const horzStepRef = useRef(intervalSec);
  horzStepRef.current = groupMode === 'slot' ? 1 : intervalSec;

  const snapshotVisibleViewport = useCallback(
    (chart: IChartApi, series: ISeriesApi<'Line' | 'Candlestick'>) => {
      if (shouldFitContentRef.current) return null;
      const logical = chart.timeScale().getVisibleLogicalRange();
      if (!logical) return null;
      return captureChartViewport(series, logical, horzStepRef.current);
    },
    [],
  );

  const sortedTrades = useMemo(
    () => [...trades].sort(compareTradesChronologically),
    [trades],
  );

  const bars = useMemo(() => {
    const built =
      groupMode === 'slot'
        ? aggregateTradesToBarsBySlot(sortedTrades, toValue, metric)
        : aggregateTradesToBars(sortedTrades, intervalSec, toValue, metric);
    return trimEmptyBars ? dropEmptyBars(built) : built;
  }, [sortedTrades, groupMode, intervalSec, toValue, metric, trimEmptyBars]);
  barsRef.current = bars;
  sortedTradesRef.current = sortedTrades;

  const tokenCreatedAtSec = useMemo(() => {
    if (!tokenCreatedAt) return null;
    const ms = Date.parse(tokenCreatedAt);
    return Number.isNaN(ms) ? null : Math.floor(ms / 1000);
  }, [tokenCreatedAt]);

  const barEarliestTradeSec = useMemo(() => {
    const map = new Map<number, number>();
    for (const trade of sortedTrades) {
      const key = groupMode === 'slot' ? tradeBarSlot(trade) : tradeBarTime(trade.block_time, intervalSec);
      if (key == null) continue;
      const ms = Date.parse(trade.block_time);
      if (Number.isNaN(ms)) continue;
      const sec = Math.floor(ms / 1000);
      const prev = map.get(key as number);
      if (prev == null || sec < prev) map.set(key as number, sec);
    }
    return map;
  }, [sortedTrades, groupMode, intervalSec]);

  const computeBarAgeSec = useCallback(
    (barTime: number): number | null => {
      if (tokenCreatedAtSec == null) return null;
      const earliest = barEarliestTradeSec.get(barTime);
      if (earliest != null) return Math.max(0, earliest - tokenCreatedAtSec);
      if (groupMode === 'slot') return null;
      return Math.max(0, barTime - tokenCreatedAtSec);
    },
    [tokenCreatedAtSec, barEarliestTradeSec, groupMode],
  );
  const computeBarAgeSecRef = useRef(computeBarAgeSec);
  computeBarAgeSecRef.current = computeBarAgeSec;

  const highlightBarTimes = useMemo(() => {
    const times = new Set<number>();
    if (selectedBarTime != null) times.add(selectedBarTime as number);
    return times;
  }, [selectedBarTime]);

  const highlightBarKey = useMemo(
    () => [...highlightBarTimes].sort((a, b) => a - b).join(','),
    [highlightBarTimes],
  );

  const formatBarTime = useCallback(
    (barTime: UTCTimestamp) => {
      if (groupMode === 'slot') return `Slot ${barTime}`;
      return createChartTimeFormatters(chartTimezone).timeFormatter(barTime);
    },
    [groupMode, chartTimezone],
  );
  const formatVol = useMemo(() => createChartPriceFormatter('SOL'), []);
  const formatRangePrice = useCallback(
    (priceInSol: number) => {
      const chartY = metric === 'price' ? priceInSol : TOKEN_TOTAL_SUPPLY * priceInSol;
      return createChartPriceFormatter(priceUnit)(toValue(chartY));
    },
    [metric, toValue, priceUnit],
  );

  const rangeStats = useMemo(
    () =>
      selectedRange
        ? computeRangeStats(sortedTrades, selectedRange, groupMode, intervalSec)
        : null,
    [selectedRange, sortedTrades, groupMode, intervalSec],
  );
  const rangeStatsRef = useRef(rangeStats);
  rangeStatsRef.current = rangeStats;
  const selectedRangeRef = useRef(selectedRange);
  selectedRangeRef.current = selectedRange;
  const rangeSelectModeRef = useRef(rangeSelectMode);
  rangeSelectModeRef.current = rangeSelectMode;

  const crosshairRafRef = useRef<number | null>(null);
  const pendingCrosshairRef = useRef<{
    crosshair: ChartCrosshairInfo | null;
    barTooltip: ChartBarTooltipState | null;
    rangeTooltip: ChartRangeTooltipState | null;
    walletMarkersTooltip: ChartWalletMarkersTooltipState | null;
  } | null>(null);

  const handleGroupModeChange = useCallback(
    (next: ChartGroupMode) => {
      setGroupMode(next);
      savePrefs({ groupMode: next, interval, style, showTradeMarkers, showMigrationLine, trimEmptyBars });
      onBarClickRef.current?.(null);
    },
    [interval, style, showTradeMarkers, showMigrationLine, trimEmptyBars],
  );

  const handleIntervalChange = useCallback(
    (next: ChartInterval) => {
      setInterval(next);
      savePrefs({ groupMode, interval: next, style, showTradeMarkers, showMigrationLine, trimEmptyBars });
      onBarClickRef.current?.(null);
    },
    [groupMode, style, showTradeMarkers, showMigrationLine, trimEmptyBars],
  );

  const handleStyleChange = useCallback(
    (next: ChartStyle) => {
      setStyle(next);
      savePrefs({ groupMode, interval, style: next, showTradeMarkers, showMigrationLine, trimEmptyBars });
    },
    [groupMode, interval, showTradeMarkers, showMigrationLine, trimEmptyBars],
  );

  const handleShowTradeMarkersChange = useCallback(
    (next: boolean) => {
      setShowTradeMarkers(next);
      savePrefs({ groupMode, interval, style, showTradeMarkers: next, showMigrationLine, trimEmptyBars });
    },
    [groupMode, interval, style, showMigrationLine, trimEmptyBars],
  );

  const handleShowMigrationLineChange = useCallback(
    (next: boolean) => {
      setShowMigrationLine(next);
      savePrefs({ groupMode, interval, style, showTradeMarkers, showMigrationLine: next, trimEmptyBars });
    },
    [groupMode, interval, style, showTradeMarkers, trimEmptyBars],
  );

  const handleTrimEmptyBarsChange = useCallback(
    (next: boolean) => {
      setTrimEmptyBars(next);
      savePrefs({ groupMode, interval, style, showTradeMarkers, showMigrationLine, trimEmptyBars: next });
    },
    [groupMode, interval, style, showTradeMarkers, showMigrationLine],
  );

  const handleSliderChange = useCallback((from: number, to: number) => {
    const chart = chartRef.current;
    if (!chart) return;
    chart.timeScale().setVisibleRange({
      from: from as UTCTimestamp,
      to: to as UTCTimestamp,
    });
  }, []);

  const showChart = Boolean(id) && !loading && !error && trades.length > 0 && bars.length > 0;

  useEffect(() => {
    if (prevIdRef.current !== id || prevGroupingKeyRef.current !== groupingKey) {
      shouldFitContentRef.current = true;
      visibleViewportRef.current = null;
      prevBarsSignatureRef.current = null;
      mountedSeriesStyleRef.current = null;
      prevIdRef.current = id;
      prevGroupingKeyRef.current = groupingKey;
      setSliderWindow(null);
      setSelectedRange(null);
    }
  }, [id, groupingKey]);

  useEffect(() => {
    if (!showChart) return;

    const el = containerRef.current;
    if (!el) return;

    const rect = el.getBoundingClientRect();
    const chart = createChart(
      el,
      createChartOptions(
        rect.width || el.clientWidth,
        height,
        groupMode,
        priceUnit,
        chartTimezone,
      ),
    );
    chartRef.current = chart;

    const ro = new ResizeObserver((entries) => {
      const { width, height: h } = entries[0]?.contentRect ?? { width: 0, height: 0 };
      if (width > 0 && h > 0) chartRef.current?.applyOptions({ width, height: h });
    });
    ro.observe(el);

    const flushCrosshair = () => {
      crosshairRafRef.current = null;
      const next = pendingCrosshairRef.current;
      if (!next) return;
      setCrosshair(next.crosshair);
      setBarTooltip(next.barTooltip);
      setRangeTooltip(next.rangeTooltip);
      setWalletMarkersTooltip(next.walletMarkersTooltip);
    };
    const scheduleCrosshair = () => {
      if (crosshairRafRef.current == null) {
        crosshairRafRef.current = requestAnimationFrame(flushCrosshair);
      }
    };

    chart.subscribeCrosshairMove((param) => {
      const next = {
        crosshair: null as ChartCrosshairInfo | null,
        barTooltip: null as ChartBarTooltipState | null,
        rangeTooltip: null as ChartRangeTooltipState | null,
        walletMarkersTooltip: null as ChartWalletMarkersTooltipState | null,
      };
      pendingCrosshairRef.current = next;
      const setCrosshair = (v: ChartCrosshairInfo | null) => { next.crosshair = v; };
      const setBarTooltip = (v: ChartBarTooltipState | null) => { next.barTooltip = v; };
      const setRangeTooltip = (v: ChartRangeTooltipState | null) => { next.rangeTooltip = v; };
      const setWalletMarkersTooltip = (v: ChartWalletMarkersTooltipState | null) => {
        next.walletMarkersTooltip = v;
      };
      scheduleCrosshair();

      // Hovering the range-selection label chip shows the range totals tooltip.
      const onRangeLabel =
        param.point != null &&
        (rangeSelectPrimRef.current?.containsLabelPoint(param.point.x, param.point.y) ?? false);
      if (onRangeLabel && selectedRangeRef.current && rangeStatsRef.current && param.point) {
        setRangeTooltip({ stats: rangeStatsRef.current, point: param.point });
        setBarTooltip(null);
        setWalletMarkersTooltip(null);
        return;
      }
      setRangeTooltip(null);

      if (!param.time) {
        setCrosshair(null);
        setBarTooltip(null);
        setWalletMarkersTooltip(null);
        return;
      }
      const bar = barsRef.current.find((b) => b.time === param.time);
      if (!bar) {
        setCrosshair(null);
        setBarTooltip(null);
        setWalletMarkersTooltip(null);
        return;
      }
      const info: ChartCrosshairInfo = {
        open: bar.open,
        high: bar.high,
        low: bar.low,
        close: bar.close,
        volume: bar.volume,
        inflow: bar.inflow,
        outflow: bar.outflow,
        liquiditySol: bar.liquiditySol,
      };
      setCrosshair(info);
      const onWalletMarker =
        param.point != null &&
        (walletMarkersPrimRef.current?.containsPoint(param.point.x, param.point.y) ?? false);

      if (onWalletMarker) {
        const walletActivity = walletActivityMapRef.current.get(bar.time as number);
        setWalletMarkersTooltip(
          walletActivity && walletActivity.length > 0
            ? { point: param.point!, wallets: walletActivity }
            : null,
        );
        setBarTooltip(null);
      } else {
        setWalletMarkersTooltip(null);
        if (param.point) {
          setBarTooltip({
            ...info,
            barTime: bar.time,
            ageSec: computeBarAgeSecRef.current(bar.time as number),
            style: styleRef.current,
            point: param.point,
          });
        } else {
          setBarTooltip(null);
        }
      }
    });

    const groupModeAtMount = groupMode;
    const intervalAtMount = intervalSec;
    chart.subscribeClick((param) => {
      if (rangeSelectModeRef.current) return;
      if (!param.time) {
        onBarClickRef.current?.(null);
        return;
      }
      if (selectedBarTimeRef.current === param.time) {
        onBarClickRef.current?.(null);
        return;
      }
      const selection: ChartBarSelection =
        groupModeAtMount === 'slot'
          ? {
              barTime: param.time as UTCTimestamp,
              groupMode: 'slot',
              slot: param.time as number,
            }
          : {
              barTime: param.time as UTCTimestamp,
              groupMode: 'time',
              intervalSec: intervalAtMount,
            };
      onBarClickRef.current?.(selection);
    });

    const onVisibleLogicalRangeChange: LogicalRangeChangeEventHandler = (logical) => {
      if (shouldFitContentRef.current || isRestoringViewportRef.current || logical == null) {
        return;
      }
      const series = seriesRef.current;
      if (!series) return;
      visibleViewportRef.current = captureChartViewport(
        series,
        logical,
        horzStepRef.current,
      );
    };
    chart.timeScale().subscribeVisibleLogicalRangeChange(onVisibleLogicalRangeChange);

    const onVisibleTimeRangeChange = (range: { from: Time; to: Time } | null) => {
      if (!range) return;
      const from = Number(range.from);
      const to = Number(range.to);
      setSliderWindow((prev) =>
        prev && prev.from === from && prev.to === to ? prev : { from, to },
      );
    };
    chart.timeScale().subscribeVisibleTimeRangeChange(onVisibleTimeRangeChange);

    return () => {
      chart.timeScale().unsubscribeVisibleLogicalRangeChange(onVisibleLogicalRangeChange);
      chart.timeScale().unsubscribeVisibleTimeRangeChange(onVisibleTimeRangeChange);
      if (crosshairRafRef.current != null) {
        cancelAnimationFrame(crosshairRafRef.current);
        crosshairRafRef.current = null;
      }
      pendingCrosshairRef.current = null;
      ro.disconnect();
      markersPluginRef.current?.detach();
      markersPluginRef.current = null;
      walletMarkersPrimRef.current = null;
      rangeSelectPrimRef.current = null;
      seriesRef.current = null;
      chart.remove();
      chartRef.current = null;
      setCrosshair(null);
      setBarTooltip(null);
      setRangeTooltip(null);
      setWalletMarkersTooltip(null);
    };
  }, [showChart, height, groupingKey, groupMode, priceUnit, chartTimezone]);

  useEffect(() => {
    const chart = chartRef.current;
    const series = seriesRef.current;
    if (!chart || !series || !showChart) return;

    const priceFormat = createChartPriceFormat(priceUnit);
    chart.applyOptions({
      localization: { priceFormatter: priceFormat.formatter },
    });
    series.applyOptions({ priceFormat });
  }, [priceUnit, showChart]);

  useEffect(() => {
    const chart = chartRef.current;
    if (!chart || !showChart || groupMode !== 'time') return;
    const { timeFormatter, tickMarkFormatter } = createChartTimeFormatters(chartTimezone);
    chart.applyOptions({
      localization: { timeFormatter },
      timeScale: { tickMarkFormatter },
    });
  }, [chartTimezone, groupMode, showChart]);

  useEffect(() => {
    const chart = chartRef.current;
    if (!chart || !showChart || bars.length === 0) return;

    const ts = chart.timeScale();
    const sig = barsSignature(bars);
    const barsShapeChanged =
      prevBarsSignatureRef.current != null && prevBarsSignatureRef.current !== sig;
    prevBarsSignatureRef.current = sig;

    const existing = seriesRef.current;
    const styleChanged = mountedSeriesStyleRef.current !== style;

    if (existing && !styleChanged) {
      const savedViewport = snapshotVisibleViewport(chart, existing);
      if (savedViewport) visibleViewportRef.current = savedViewport;

      if (style === 'line') {
        existing.setData(barsToLineData(bars));
      } else {
        existing.setData(barsToCandleData(bars, highlightBarTimes));
      }

      if (savedViewport) {
        isRestoringViewportRef.current = true;
        reapplyChartViewport(ts, savedViewport, barsShapeChanged ? 'time' : 'logical');
        requestAnimationFrame(() => {
          isRestoringViewportRef.current = false;
        });
      }
      return;
    }

    const savedViewport =
      existing != null
        ? snapshotVisibleViewport(chart, existing)
        : shouldFitContentRef.current
          ? null
          : visibleViewportRef.current;
    if (savedViewport) visibleViewportRef.current = savedViewport;

    if (existing) {
      if (migrationLineRef.current) {
        existing.removePriceLine(migrationLineRef.current);
        migrationLineRef.current = null;
      }
      markersPluginRef.current?.detach();
      markersPluginRef.current = null;
      if (walletMarkersPrimRef.current) {
        existing.detachPrimitive(asSeriesPrimitive(walletMarkersPrimRef.current));
        walletMarkersPrimRef.current = null;
      }
      if (rangeSelectPrimRef.current) {
        existing.detachPrimitive(asRangePrimitive(rangeSelectPrimRef.current));
        rangeSelectPrimRef.current = null;
      }
      chart.removeSeries(existing);
      seriesRef.current = null;
    }

    const SeriesCtor = SERIES_BY_STYLE[style];
    const baseOptions = style === 'line' ? LINE_SERIES_OPTIONS : CANDLE_SERIES_OPTIONS;
    const series = chart.addSeries(SeriesCtor, {
      ...baseOptions,
      priceFormat: createChartPriceFormat(priceUnit),
    });
    seriesRef.current = series as ISeriesApi<'Line' | 'Candlestick'>;
    mountedSeriesStyleRef.current = style;

    const walletPrim = new WalletMarkersPlugin();
    series.attachPrimitive(asSeriesPrimitive(walletPrim));
    walletMarkersPrimRef.current = walletPrim;

    const rangePrim = new RangeSelectPlugin();
    series.attachPrimitive(asRangePrimitive(rangePrim));
    rangeSelectPrimRef.current = rangePrim;

    if (style === 'line') {
      series.setData(barsToLineData(bars));
    } else {
      series.setData(barsToCandleData(bars, highlightBarTimes));
    }

    if (shouldFitContentRef.current) {
      ts.fitContent();
      shouldFitContentRef.current = false;
      visibleViewportRef.current = null;
    } else if (savedViewport) {
      isRestoringViewportRef.current = true;
      reapplyChartViewport(ts, savedViewport, barsShapeChanged ? 'time' : 'logical');
      requestAnimationFrame(() => {
        isRestoringViewportRef.current = false;
      });
    }
  }, [bars, style, showChart, groupingKey, priceUnit, highlightBarKey, snapshotVisibleViewport]);

  useEffect(() => {
    const series = seriesRef.current;
    if (!series || !showChart) return;

    const markers: SeriesMarker<UTCTimestamp>[] = [];

    if (showTradeMarkers) {
      markers.push(...buildTradeMarkers(trades, groupMode, intervalSec));
    }

    if (effectiveProfileWallets.length > 0) {
      const walletDefs = buildWalletMarkerDefs(sortedTrades, effectiveProfileWallets, bars, groupMode, intervalSec);
      walletMarkersPrimRef.current?.setMarkers(walletDefs);
      walletActivityMapRef.current = buildWalletBarActivityMap(trades, effectiveProfileWallets, groupMode, intervalSec);
    } else {
      walletMarkersPrimRef.current?.setMarkers([]);
      walletActivityMapRef.current = new Map();
    }

    if (selectedBarTime != null) {
      const bar = barsRef.current.find((b) => b.time === selectedBarTime);
      if (bar) {
        markers.push(barSelectionMarker(bar));
      }
    }

    if (eventMarkers && eventMarkers.length > 0) {
      markers.push(
        ...buildEventSeriesMarkers(eventMarkers, sortedTrades, bars, groupMode, intervalSec),
      );
    }

    const sorted = sortSeriesMarkers(markers);

    markersPluginRef.current?.detach();
    markersPluginRef.current = null;

    if (sorted.length > 0) {
      markersPluginRef.current = createSeriesMarkers(series, sorted) as MarkersPlugin;
    }
  }, [
    showTradeMarkers,
    trades,
    groupMode,
    intervalSec,
    showChart,
    style,
    selectedBarTime,
    sortedTrades,
    bars,
    effectiveProfileWallets,
    eventMarkers,
  ]);

  // Render the committed range selection as a band with a duration chip.
  useEffect(() => {
    const prim = rangeSelectPrimRef.current;
    if (!prim || !showChart) return;

    if (!selectedRange) {
      prim.setBand(null);
      setRangeTooltip(null);
      return;
    }

    const label = rangeStats ? formatRangeDuration(rangeStats.durationMs) : 'Range';
    prim.setBand({
      loTime: Math.min(selectedRange.lo, selectedRange.hi) as UTCTimestamp,
      hiTime: Math.max(selectedRange.lo, selectedRange.hi) as UTCTimestamp,
      label,
      dashed: false,
    });
  }, [selectedRange, rangeStats, rangeSelectMode, showChart, style, groupingKey]);

  // Surface the committed range (with grouping context) to the parent.
  useEffect(() => {
    onRangeChangeRef.current?.(
      selectedRange
        ? { lo: selectedRange.lo, hi: selectedRange.hi, groupMode, intervalSec }
        : null,
    );
  }, [selectedRange, groupMode, intervalSec]);

  // Drag-to-select a time range.
  useEffect(() => {
    if (!showChart || !rangeSelectMode) return;
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
      try { el.setPointerCapture(e.pointerId); } catch { /* ignore */ }
      rangeSelectPrimRef.current?.setBand({
        loTime: t as UTCTimestamp,
        hiTime: t as UTCTimestamp,
        dashed: true,
      });
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
      try { el.releasePointerCapture(e.pointerId); } catch { /* ignore */ }
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
  }, [showChart, height, groupingKey, groupMode, priceUnit, chartTimezone, rangeSelectMode]);

  // Dashed line at the pump.fun graduation price.
  useEffect(() => {
    const series = seriesRef.current;
    if (!series || !showChart) return;

    if (migrationLineRef.current) {
      series.removePriceLine(migrationLineRef.current);
      migrationLineRef.current = null;
    }

    if (!showMigrationLine) return;

    const price = migrationChartValue(metric, toValue);

    migrationLineRef.current = series.createPriceLine({
      price,
      color: CHART_COLORS.migrationLine,
      lineWidth: 1,
      lineStyle: LineStyle.Dashed,
      axisLabelVisible: true,
      title: 'Migration',
    });
  }, [showMigrationLine, metric, toValue, showChart, style]);

  // Dashed horizontal lines at the strategy's entry/exit fill prices.
  useEffect(() => {
    const series = seriesRef.current;
    if (!series || !showChart) return;

    for (const line of eventLineRefs.current) {
      series.removePriceLine(line);
    }
    eventLineRefs.current = [];

    if (!eventMarkers) return;

    for (const m of eventMarkers) {
      const value = athChartValue(m.priceInSol, metric, toValue);
      if (value == null) continue;
      const isEntry = m.kind === 'entry';
      eventLineRefs.current.push(
        series.createPriceLine({
          price: value,
          color: isEntry ? CHART_COLORS.entry : CHART_COLORS.exit,
          lineWidth: 1,
          lineStyle: LineStyle.Dashed,
          axisLabelVisible: true,
          title: isEntry ? 'Entry' : 'Exit',
        }),
      );
    }
  }, [eventMarkers, metric, toValue, showChart, style, bars]);

  if (!id) {
    return (
      <Placeholder
        message="Select a token to view price history."
        className={className}
        height={height}
      />
    );
  }

  if (loading) {
    return (
      <Placeholder message={`Loading trades for ${symbol}…`} className={className} height={height} />
    );
  }

  if (error) {
    return (
      <div
        className={panelClass(cn('flex items-center justify-center p-4 text-xs', className))}
        style={{ ...panelStyle, height, borderColor: '#f2364544', color: CHART_COLORS.down }}
      >
        {error}
      </div>
    );
  }

  if (trades.length === 0) {
    return (
      <Placeholder message={`No trades recorded for ${symbol}.`} className={className} height={height} />
    );
  }

  if (bars.length === 0) {
    return (
      <Placeholder
        message={`No chart data for ${symbol} at ${groupMode === 'slot' ? 'slot' : interval} grouping.`}
        className={className}
        height={height}
      />
    );
  }

  return (
    <div className={panelClass(className)} style={panelStyle}>
      <ChartToolbar
        symbol={symbol}
        groupMode={groupMode}
        interval={interval}
        style={style}
        priceLabel={priceLabel}
        priceUnit={priceUnit}
        metric={metric}
        tradeCount={trades.length}
        showTradeMarkers={showTradeMarkers}
        showMigrationLine={showMigrationLine}
        trimEmptyBars={trimEmptyBars}
        rangeSelectMode={rangeSelectMode}
        crosshair={crosshair}
        isMigrated={isMigrated}
        onGroupModeChange={handleGroupModeChange}
        onIntervalChange={handleIntervalChange}
        onStyleChange={handleStyleChange}
        onMetricChange={onMetricChange}
        onShowTradeMarkersChange={handleShowTradeMarkersChange}
        onShowMigrationLineChange={handleShowMigrationLineChange}
        onTrimEmptyBarsChange={handleTrimEmptyBarsChange}
        onRangeSelectModeChange={setRangeSelectMode}
      />
      <div className="relative" style={{ height, width: '100%' }}>
        <div ref={containerRef} style={{ height: '100%', width: '100%' }} />
        {barTooltip && (
          <BarCrosshairTooltip
            tooltip={barTooltip}
            formatVol={formatVol}
            formatTime={formatBarTime}
          />
        )}
        {walletMarkersTooltip && <WalletMarkersTooltip tooltip={walletMarkersTooltip} />}
        {rangeTooltip && (
          <RangeSelectTooltip
            tooltip={rangeTooltip}
            formatAmount={formatVol}
            formatPrice={formatRangePrice}
          />
        )}
      </div>
      {bars.length > 1 && (
        <ChartRangeSlider
          min={bars[0].time as number}
          max={bars[bars.length - 1].time as number}
          from={sliderWindow?.from ?? (bars[0].time as number)}
          to={sliderWindow?.to ?? (bars[bars.length - 1].time as number)}
          onChange={handleSliderChange}
        />
      )}
    </div>
  );
}
