import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  createChart,
  createSeriesMarkers,
  LineSeries,
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
  type ChartViewport,
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
import { createChartTimeFormatters } from './chartTimezone';
import { useTimezone } from 'context/TimezoneContext';
import { cn } from './cn';
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
  SWING_HIGH_OVERLAY_SERIES_OPTIONS,
  TOKEN_TOTAL_SUPPLY,
} from './constants';
import { BarCrosshairTooltip } from './BarCrosshairTooltip';
import { SwingCrosshairTooltip } from './SwingCrosshairTooltip';
import { ChainHighlightTooltip, type ChainTradeCounts } from './ChainHighlightTooltip';
import { WalletMarkersTooltip } from './WalletMarkersTooltip';
import { RangeSelectTooltip, formatRangeDuration } from './RangeSelectTooltip';
import { WalletMarkersPlugin, asSeriesPrimitive, type WalletMarkerDef } from './walletMarkersPlugin';
import { ChainHighlightPlugin, asChainPrimitive } from './chainHighlightPlugin';
import { RangeSelectPlugin, asRangePrimitive } from './rangeSelectPlugin';
import {
  buildLegSegment,
  chartTimeRangeForSpan,
  groupSequentialLegChains,
  resolveSwingLegAtChartInteraction,
  swingLegKey,
  swingLegBarTimes,
  swingLegSelectionMarkers,
  swingsToColoredLineData,
} from './swingOverlay';
import type {
  ChartBarSelection,
  ChartCrosshairInfo,
  ChartGroupMode,
  ChartInterval,
  ChartStyle,
  ChartSwingLeg,
  ChartBarTooltipState,
  ChartSwingTooltipState,
  ChartChainTooltipState,
  ChartChainHighlight,
  ChartEventMarker,
  ChartRangeSelection,
  ChartRangeTooltipState,
  ChartWalletMarkersTooltipState,
  ChartTrade,
  OhlcBar,
  ProfileWalletInfo,
  TokenPriceChartProps,
  WalletBarActivity,
} from './types';

function loadPrefs(): {
  groupMode: ChartGroupMode;
  interval: ChartInterval;
  style: ChartStyle;
  showTradeMarkers: boolean;
  showAthLine: boolean;
  showMigrationLine: boolean;
  trimEmptyBars: boolean;
} {
  try {
    const raw = localStorage.getItem(LS_CHART_PREFS_KEY);
    if (raw) {
      const parsed = JSON.parse(raw) as {
        groupMode?: ChartGroupMode;
        interval?: ChartInterval;
        style?: ChartStyle;
        showTradeMarkers?: boolean;
        showAthLine?: boolean;
        showMigrationLine?: boolean;
        trimEmptyBars?: boolean;
      };
      return {
        groupMode: parsed.groupMode ?? DEFAULT_CHART_PREFS.groupMode,
        interval: parsed.interval ?? DEFAULT_CHART_PREFS.interval,
        style: parsed.style ?? DEFAULT_CHART_PREFS.style,
        showTradeMarkers: parsed.showTradeMarkers ?? DEFAULT_CHART_PREFS.showTradeMarkers,
        showAthLine: parsed.showAthLine ?? DEFAULT_CHART_PREFS.showAthLine,
        showMigrationLine:
          parsed.showMigrationLine ?? DEFAULT_CHART_PREFS.showMigrationLine,
        trimEmptyBars: parsed.trimEmptyBars ?? DEFAULT_CHART_PREFS.trimEmptyBars,
      };
    }
  } catch {
    /* ignore */
  }
  return DEFAULT_CHART_PREFS;
}

function savePrefs(
  groupMode: ChartGroupMode,
  interval: ChartInterval,
  style: ChartStyle,
  showTradeMarkers: boolean,
  showAthLine: boolean,
  showMigrationLine: boolean,
  trimEmptyBars: boolean,
) {
  try {
    localStorage.setItem(
      LS_CHART_PREFS_KEY,
      JSON.stringify({
        groupMode,
        interval,
        style,
        showTradeMarkers,
        showAthLine,
        showMigrationLine,
        trimEmptyBars,
      }),
    );
  } catch {
    /* ignore */
  }
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

  // Snap to the nearest existing bar so the marker always lands on data.
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

function buildWalletMarkerDefs(
  trades: ChartTrade[],
  profileWallets: ProfileWalletInfo[],
  bars: OhlcBar[],
  groupMode: ChartGroupMode,
  intervalSec: number,
): WalletMarkerDef[] {
  const walletMap = new Map(profileWallets.map((w) => [w.address, w]));
  const barMap = new Map(bars.map((b) => [b.time as number, b]));

  // barTime -> { buy: wallets[], sell: wallets[] } — one entry per wallet per type per bar
  const groups = new Map<number, { buy: ProfileWalletInfo[]; sell: ProfileWalletInfo[] }>();
  const seen = new Set<string>(); // `${address}:${barTime}:${type}`

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
    const type = trade.trade_type === 'buy' ? 'buy' : 'sell';
    const key = `${trade.wallet_address}:${t}:${type}`;
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

    // Always render tracked markers above the candle/line, stacking buys then
    // sells with a continuous index so they never overlap or fall below the bar.
    let stackIndex = 0;
    for (const w of buy) {
      defs.push({
        barTime,
        barEdgePrice: bar.high,
        letter: (w.profileName ?? w.label ?? w.address).charAt(0).toUpperCase(),
        color: w.color,
        borderColor: CHART_COLORS.up,
        type: 'sell',
        stackIndex: stackIndex++,
      });
    }
    for (const w of sell) {
      defs.push({
        barTime,
        barEdgePrice: bar.high,
        letter: (w.profileName ?? w.label ?? w.address).charAt(0).toUpperCase(),
        color: w.color,
        borderColor: CHART_COLORS.down,
        type: 'sell',
        stackIndex: stackIndex++,
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
  // barTime -> walletAddress -> activity
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
      activity.buySol += trade.sol_amount ?? 0;
    } else {
      activity.sellCount += 1;
      activity.sellSol += trade.sol_amount ?? 0;
    }
  }

  const result = new Map<number, WalletBarActivity[]>();
  for (const [barTime, walletActivities] of byBar) {
    result.set(barTime, [...walletActivities.values()]);
  }
  return result;
}

/** Total/buy/sell trade counts inside the chain window [startAt, endAt] (ms epoch). */
function computeChainTradeCounts(
  trades: ChartTrade[],
  highlight: ChartChainHighlight | null,
): ChainTradeCounts {
  if (!highlight) return { total: 0, buy: 0, sell: 0 };
  let buy = 0;
  let sell = 0;
  for (const t of trades) {
    const ms = new Date(t.block_time).getTime();
    if (Number.isNaN(ms) || ms < highlight.startAt || ms > highlight.endAt) continue;
    if (t.trade_type === 'buy') buy += 1;
    else sell += 1;
  }
  return { total: buy + sell, buy, sell };
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
  swingOverlay = null,
  highlightChain = null,
  selectedSwingLegKey = null,
  onSwingLegClick,
  connectSwings: connectSwingsProp,
  onConnectSwingsChange,
  athPriceInSol = null,
  isMigrated,
  isMayhemMode,
  isCashbackEnabled,
  profileWallets,
  tokenCreatedAt,
  eventMarkers = null,
}: TokenPriceChartProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const chartRef = useRef<IChartApi | null>(null);
  const onBarClickRef = useRef(onBarClick);
  onBarClickRef.current = onBarClick;
  const onRangeChangeRef = useRef(onRangeChange);
  onRangeChangeRef.current = onRangeChange;
  const onSwingLegClickRef = useRef(onSwingLegClick);
  onSwingLegClickRef.current = onSwingLegClick;
  const selectedSwingLegKeyRef = useRef(selectedSwingLegKey);
  selectedSwingLegKeyRef.current = selectedSwingLegKey;
  const seriesRef = useRef<ISeriesApi<'Line' | 'Candlestick'> | null>(null);
  const swingSeriesRefs = useRef<ISeriesApi<'Line'>[]>([]);
  const swingSeriesLegRef = useRef(new Map<ISeriesApi<'Line'>, ChartSwingLeg>());
  const swingOverlayMetaRef = useRef<{
    segmentMode: 'connected' | 'perLeg' | 'connectedSequential';
    groupMode: ChartGroupMode;
    legs: ChartSwingLeg[];
    allLegs?: ChartSwingLeg[];
  } | null>(null);
  const showSwingOverlayRef = useRef(true);
  const sortedTradesRef = useRef<ChartTrade[]>([]);
  const markersPluginRef = useRef<MarkersPlugin | null>(null);
  const walletMarkersPrimRef = useRef<WalletMarkersPlugin | null>(null);
  const chainHighlightPrimRef = useRef<ChainHighlightPlugin | null>(null);
  const rangeSelectPrimRef = useRef<RangeSelectPlugin | null>(null);
  const highlightChainRef = useRef(highlightChain);
  highlightChainRef.current = highlightChain;
  const barsRef = useRef<OhlcBar[]>([]);

  const initialPrefs = loadPrefs();
  const [groupMode, setGroupMode] = useState<ChartGroupMode>(initialPrefs.groupMode);
  const [interval, setInterval] = useState<ChartInterval>(initialPrefs.interval);
  const [style, setStyle] = useState<ChartStyle>(initialPrefs.style);
  const [showTradeMarkers, setShowTradeMarkers] = useState(initialPrefs.showTradeMarkers);
  const [showAthLine, setShowAthLine] = useState(initialPrefs.showAthLine);
  const [showMigrationLine, setShowMigrationLine] = useState(initialPrefs.showMigrationLine);
  const [trimEmptyBars, setTrimEmptyBars] = useState(initialPrefs.trimEmptyBars);
  const { timezone: chartTimezone } = useTimezone();
  const swingOverlayAvailable = (swingOverlay?.legs.length ?? 0) > 0;
  const chainHighlightAvailable = highlightChain != null;
  const [showSwingOverlay, setShowSwingOverlay] = useState(true);
  const [showChainHighlight, setShowChainHighlight] = useState(true);
  const [connectSwingsInternal, setConnectSwingsInternal] = useState(true);
  const connectSwings = connectSwingsProp ?? connectSwingsInternal;
  const setConnectSwings = onConnectSwingsChange ?? setConnectSwingsInternal;
  const [rangeSelectMode, setRangeSelectMode] = useState(false);
  const [selectedRange, setSelectedRange] = useState<ChartRangeSelection | null>(null);
  const [crosshair, setCrosshair] = useState<ChartCrosshairInfo | null>(null);
  const [barTooltip, setBarTooltip] = useState<ChartBarTooltipState | null>(null);
  const [swingTooltip, setSwingTooltip] = useState<ChartSwingTooltipState | null>(null);
  const [chainTooltip, setChainTooltip] = useState<ChartChainTooltipState | null>(null);
  const [rangeTooltip, setRangeTooltip] = useState<ChartRangeTooltipState | null>(null);
  const [walletMarkersTooltip, setWalletMarkersTooltip] = useState<ChartWalletMarkersTooltipState | null>(null);
  /** Visible window mirrored from the chart's time scale, drives the range slider. */
  const [sliderWindow, setSliderWindow] = useState<{ from: number; to: number } | null>(null);
  const walletActivityMapRef = useRef<Map<number, WalletBarActivity[]>>(new Map());
  const styleRef = useRef(style);
  styleRef.current = style;
  const athLineRef = useRef<IPriceLine | null>(null);
  const migrationLineRef = useRef<IPriceLine | null>(null);
  const eventLineRefs = useRef<IPriceLine[]>([]);

  const toValue = useCallback(
    (sol: number) => (toValueProp ? toValueProp(sol) : sol),
    [toValueProp],
  );

  const intervalSec = CHART_INTERVALS[interval];
  const groupingKey = groupMode === 'slot' ? 'slot' : intervalSec;
  const selectedBarTime = selectedBar?.barTime ?? null;

  const shouldFitContentRef = useRef(true);
  const prevIdRef = useRef(id);
  const prevGroupingKeyRef = useRef(groupingKey);
  const visibleViewportRef = useRef<ChartViewport | null>(null);
  const isRestoringViewportRef = useRef(false);
  const mountedSeriesStyleRef = useRef<ChartStyle | null>(null);
  const prevBarsSignatureRef = useRef<string | null>(null);
  const horzStepRef = useRef(intervalSec);
  horzStepRef.current = groupMode === 'slot' ? 1 : intervalSec;

  const snapshotVisibleViewport = useCallback(
    (chart: IChartApi, series: ISeriesApi<'Line' | 'Candlestick'>): ChartViewport | null => {
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
  showSwingOverlayRef.current = showSwingOverlay;

  // Token creation time (epoch seconds) for per-bar tx age in the crosshair tooltip.
  const tokenCreatedAtSec = useMemo(() => {
    if (!tokenCreatedAt) return null;
    const ms = Date.parse(tokenCreatedAt);
    return Number.isNaN(ms) ? null : Math.floor(ms / 1000);
  }, [tokenCreatedAt]);

  // Earliest real trade time (epoch seconds) per bar key, keyed exactly as bars are
  // bucketed: slot number in slot mode, bucket-start seconds in time mode.
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
      // Empty/flat bar (no trades): bar time is a real timestamp only in time mode.
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
    if (selectedSwingLegKey && swingOverlay?.legs.length) {
      const leg = swingOverlay.legs.find(
        (l) => swingLegKey(l) === selectedSwingLegKey,
      );
      if (leg) {
        for (const t of swingLegBarTimes(
          leg,
          bars,
          groupMode,
          sortedTrades,
          intervalSec,
        )) {
          times.add(t as number);
        }
      }
    }
    return times;
  }, [
    selectedBarTime,
    selectedSwingLegKey,
    swingOverlay,
    bars,
    groupMode,
    sortedTrades,
    intervalSec,
  ]);

  const highlightBarKey = useMemo(
    () => [...highlightBarTimes].sort((a, b) => a - b).join(','),
    [highlightBarTimes],
  );

  const formatChartPrice = useMemo(
    () => createChartPriceFormatter(priceUnit),
    [priceUnit],
  );
  const formatSwingPrice = useCallback(
    (priceInSol: number) => {
      const chartY = metric === 'price' ? priceInSol : TOKEN_TOTAL_SUPPLY * priceInSol;
      return formatChartPrice(toValue(chartY));
    },
    [metric, toValue, formatChartPrice],
  );
  const formatSwingAmount = useCallback(
    (sol: number) => formatChartPrice(toValue(sol)),
    [toValue, formatChartPrice],
  );
  const formatBarTime = useCallback(
    (barTime: UTCTimestamp) => {
      if (groupMode === 'slot') return `Slot ${barTime}`;
      return createChartTimeFormatters(chartTimezone).timeFormatter(barTime);
    },
    [groupMode, chartTimezone],
  );
  const formatVol = useMemo(() => createChartPriceFormatter('SOL'), []);

  const chainTradeCounts = useMemo(
    () => computeChainTradeCounts(sortedTrades, highlightChain),
    [sortedTrades, highlightChain],
  );

  const rangeStats = useMemo(
    () =>
      selectedRange
        ? computeRangeStats(sortedTrades, selectedRange, groupMode, intervalSec)
        : null,
    [selectedRange, sortedTrades, groupMode, intervalSec],
  );
  // Mirrored into refs so the (mount-time) crosshair handler can read the live
  // selection/stats and gate bar-selection clicks while in range-select mode.
  const rangeStatsRef = useRef(rangeStats);
  rangeStatsRef.current = rangeStats;
  const selectedRangeRef = useRef(selectedRange);
  selectedRangeRef.current = selectedRange;
  const rangeSelectModeRef = useRef(rangeSelectMode);
  rangeSelectModeRef.current = rangeSelectMode;

  const athLineAvailable = athChartValue(athPriceInSol, metric, toValue) != null;

  useEffect(() => {
    if (swingOverlayAvailable) setShowSwingOverlay(true);
  }, [swingOverlayAvailable, swingOverlay?.legs]);

  useEffect(() => {
    if (chainHighlightAvailable) setShowChainHighlight(true);
  }, [chainHighlightAvailable, highlightChain]);

  const handleGroupModeChange = useCallback(
    (next: ChartGroupMode) => {
      setGroupMode(next);
      savePrefs(next, interval, style, showTradeMarkers, showAthLine, showMigrationLine, trimEmptyBars);
      onBarClickRef.current?.(null);
    },
    [interval, style, showTradeMarkers, showAthLine, showMigrationLine, trimEmptyBars],
  );

  const handleIntervalChange = useCallback(
    (next: ChartInterval) => {
      setInterval(next);
      savePrefs(groupMode, next, style, showTradeMarkers, showAthLine, showMigrationLine, trimEmptyBars);
      onBarClickRef.current?.(null);
    },
    [groupMode, style, showTradeMarkers, showAthLine, showMigrationLine, trimEmptyBars],
  );

  const handleStyleChange = useCallback(
    (next: ChartStyle) => {
      setStyle(next);
      savePrefs(groupMode, interval, next, showTradeMarkers, showAthLine, showMigrationLine, trimEmptyBars);
    },
    [groupMode, interval, showTradeMarkers, showAthLine, showMigrationLine, trimEmptyBars],
  );

  const handleShowTradeMarkersChange = useCallback(
    (next: boolean) => {
      setShowTradeMarkers(next);
      savePrefs(groupMode, interval, style, next, showAthLine, showMigrationLine, trimEmptyBars);
    },
    [groupMode, interval, style, showAthLine, showMigrationLine, trimEmptyBars],
  );

  const handleShowAthLineChange = useCallback(
    (next: boolean) => {
      setShowAthLine(next);
      savePrefs(groupMode, interval, style, showTradeMarkers, next, showMigrationLine, trimEmptyBars);
    },
    [groupMode, interval, style, showTradeMarkers, showMigrationLine, trimEmptyBars],
  );

  const handleShowMigrationLineChange = useCallback(
    (next: boolean) => {
      setShowMigrationLine(next);
      savePrefs(groupMode, interval, style, showTradeMarkers, showAthLine, next, trimEmptyBars);
    },
    [groupMode, interval, style, showTradeMarkers, showAthLine, trimEmptyBars],
  );

  const handleTrimEmptyBarsChange = useCallback(
    (next: boolean) => {
      setTrimEmptyBars(next);
      savePrefs(groupMode, interval, style, showTradeMarkers, showAthLine, showMigrationLine, next);
    },
    [groupMode, interval, style, showTradeMarkers, showAthLine, showMigrationLine],
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
      // Range bounds are in the old grouping's units (slot vs bucket-sec) — drop them.
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

    chart.subscribeCrosshairMove((param) => {
      // Hovering the chain label chip (not the band body) shows the chain totals
      // tooltip and suppresses every other tooltip.
      const onChainLabel =
        param.point != null &&
        (chainHighlightPrimRef.current?.containsLabelPoint(param.point.x, param.point.y) ??
          false);
      if (onChainLabel && highlightChainRef.current && param.point) {
        setChainTooltip({ highlight: highlightChainRef.current, point: param.point });
        setSwingTooltip(null);
        setBarTooltip(null);
        setWalletMarkersTooltip(null);
        setRangeTooltip(null);
        return;
      }
      setChainTooltip(null);

      // Hovering the range-selection label chip shows the range totals tooltip
      // and suppresses every other tooltip (same pattern as the chain label).
      const onRangeLabel =
        param.point != null &&
        (rangeSelectPrimRef.current?.containsLabelPoint(param.point.x, param.point.y) ??
          false);
      if (onRangeLabel && selectedRangeRef.current && rangeStatsRef.current && param.point) {
        setRangeTooltip({ stats: rangeStatsRef.current, point: param.point });
        setSwingTooltip(null);
        setBarTooltip(null);
        setWalletMarkersTooltip(null);
        return;
      }
      setRangeTooltip(null);

      const hovered =
        param.hoveredSeries ?? param.hoveredInfo?.series;
      const onSwingSeries =
        hovered != null &&
        swingSeriesRefs.current.includes(hovered as ISeriesApi<'Line'>);
      const chartTime = onSwingSeries
        ? typeof param.time === 'number'
          ? param.time
          : hovered != null
            ? (() => {
                const seriesData = param.seriesData.get(hovered);
                return seriesData && 'time' in seriesData
                  ? (seriesData.time as number)
                  : undefined;
              })()
            : undefined
        : undefined;

      let activeSwingLeg: ChartSwingLeg | undefined;
      if (!param.point || !showSwingOverlayRef.current || !onSwingSeries) {
        setSwingTooltip(null);
      } else {
        const leg = resolveSwingLegAtChartInteraction(
          hovered as ISeriesApi<'Line'> | undefined,
          param.seriesData,
          chartTime,
          swingSeriesRefs.current,
          swingSeriesLegRef.current,
          swingOverlayMetaRef.current,
          sortedTradesRef.current,
        );
        if (leg) {
          activeSwingLeg = leg;
          setSwingTooltip({ leg, point: param.point });
        } else {
          setSwingTooltip(null);
        }
      }

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
        const onMainSeries =
          hovered != null &&
          seriesRef.current != null &&
          hovered === seriesRef.current;
        if (param.point && (!activeSwingLeg || onMainSeries)) {
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
      // In range-select mode the pointer-drag handler owns clicks; don't also
      // toggle a bar selection.
      if (rangeSelectModeRef.current) return;
      const hovered =
        param.hoveredSeries ?? param.hoveredInfo?.series;
      const onSwingSeries =
        hovered != null &&
        swingSeriesRefs.current.includes(hovered as ISeriesApi<'Line'>);
      const chartTime = onSwingSeries
        ? typeof param.time === 'number'
          ? param.time
          : hovered != null
            ? (() => {
                const seriesData = param.seriesData.get(hovered);
                return seriesData && 'time' in seriesData
                  ? (seriesData.time as number)
                  : undefined;
              })()
            : undefined
        : undefined;
      const swingLeg =
        showSwingOverlayRef.current && onSwingSeries
          ? resolveSwingLegAtChartInteraction(
              hovered as ISeriesApi<'Line'> | undefined,
              param.seriesData,
              chartTime,
              swingSeriesRefs.current,
              swingSeriesLegRef.current,
              swingOverlayMetaRef.current,
              sortedTradesRef.current,
              { requireSwingSeries: true },
            )
          : undefined;
      if (swingLeg && onSwingLegClickRef.current) {
        const key = swingLegKey(swingLeg);
        const next =
          selectedSwingLegKeyRef.current === key ? null : swingLeg;
        onSwingLegClickRef.current(next);
        return;
      }

      if (!param.time) {
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
      ro.disconnect();
      markersPluginRef.current?.detach();
      markersPluginRef.current = null;
      walletMarkersPrimRef.current = null;
      chainHighlightPrimRef.current = null;
      rangeSelectPrimRef.current = null;
      seriesRef.current = null;
      swingSeriesRefs.current = [];
      chart.remove();
      chartRef.current = null;
      setCrosshair(null);
      setBarTooltip(null);
      setSwingTooltip(null);
      setChainTooltip(null);
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
      if (athLineRef.current) {
        existing.removePriceLine(athLineRef.current);
        athLineRef.current = null;
      }
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
      if (chainHighlightPrimRef.current) {
        existing.detachPrimitive(asChainPrimitive(chainHighlightPrimRef.current));
        chainHighlightPrimRef.current = null;
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

    const chainPrim = new ChainHighlightPlugin();
    series.attachPrimitive(asChainPrimitive(chainPrim));
    chainHighlightPrimRef.current = chainPrim;

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

    if (profileWallets && profileWallets.length > 0) {
      const walletDefs = buildWalletMarkerDefs(trades, profileWallets, bars, groupMode, intervalSec);
      walletMarkersPrimRef.current?.setMarkers(walletDefs);
      walletActivityMapRef.current = buildWalletBarActivityMap(trades, profileWallets, groupMode, intervalSec);
    } else {
      walletMarkersPrimRef.current?.setMarkers([]);
      walletActivityMapRef.current = new Map();
    }

    if (selectedSwingLegKey && swingOverlay?.legs.length) {
      const leg = swingOverlay.legs.find(
        (l) => swingLegKey(l) === selectedSwingLegKey,
      );
      if (leg) {
        markers.push(
          ...swingLegSelectionMarkers(
            leg,
            bars,
            groupMode,
            sortedTrades,
            intervalSec,
          ),
        );
      }
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
    selectedSwingLegKey,
    selectedBarTime,
    swingOverlay,
    sortedTrades,
    bars,
    profileWallets,
    eventMarkers,
  ]);

  // Highlight the longest swing chain as a full-height band. Resolving the
  // span to bar-aligned chart times here keeps the plugin in sync with the
  // current grouping/interval; the plugin recomputes pixel positions per frame.
  useEffect(() => {
    const prim = chainHighlightPrimRef.current;
    if (!prim || !showChart) return;

    if (!highlightChain || !showChainHighlight) {
      prim.setHighlight(null);
      setChainTooltip(null);
      return;
    }

    const range = chartTimeRangeForSpan(
      highlightChain.startAt,
      highlightChain.endAt,
      groupMode,
      sortedTrades,
      intervalSec,
    );
    if (!range) {
      prim.setHighlight(null);
      return;
    }

    prim.setHighlight({
      loTime: range.lo as UTCTimestamp,
      hiTime: range.hi as UTCTimestamp,
      pairCount: highlightChain.pairCount,
    });
  }, [highlightChain, showChainHighlight, groupMode, intervalSec, sortedTrades, showChart, style, bars]);

  // Render the committed range selection as a band with a duration chip. Keyed
  // on style/grouping/bars so it re-applies after the series (and its plugin)
  // is recreated; the live drag preview is driven directly from the pointer
  // handlers below. `rangeSelectMode` is a dep so flipping the mode wipes any
  // stale draft band left over from an interrupted drag.
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
  }, [selectedRange, rangeStats, rangeSelectMode, showChart, style, groupingKey, bars]);

  // Surface the committed range (with grouping context) to the parent so it can
  // list the range's trades below the chart. `selectedRange` is reset to null on
  // id/grouping changes, so this also clears the parent's selection on those.
  useEffect(() => {
    onRangeChangeRef.current?.(
      selectedRange
        ? { lo: selectedRange.lo, hi: selectedRange.hi, groupMode, intervalSec }
        : null,
    );
  }, [selectedRange, groupMode, intervalSec]);

  // Drag-to-select a time range. Active only in range-select mode: disable the
  // chart's pan/zoom so a horizontal drag draws a band instead of scrolling,
  // and snap both edges to the nearest bar via the logical coordinate.
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
      // A drag too short to clear the threshold reads as a click → clear. Drop
      // the draft band directly too: if no selection existed, setSelectedRange
      // is a no-op and the band effect won't fire to clear the pointerdown dot.
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
      // Restore pan/zoom unless the chart was already torn down/replaced.
      if (chartRef.current === chart) {
        chart.applyOptions({ handleScroll: true, handleScale: true });
      }
    };
  }, [showChart, height, groupingKey, groupMode, priceUnit, chartTimezone, rangeSelectMode]);

  useEffect(() => {
    const series = seriesRef.current;
    if (!series || !showChart) return;

    if (athLineRef.current) {
      series.removePriceLine(athLineRef.current);
      athLineRef.current = null;
    }

    if (!showAthLine || !athLineAvailable) return;

    const price = athChartValue(athPriceInSol, metric, toValue);
    if (price == null) return;

    athLineRef.current = series.createPriceLine({
      price,
      color: CHART_COLORS.athLine,
      lineWidth: 1,
      lineStyle: LineStyle.Dashed,
      axisLabelVisible: true,
      title: 'ATH',
    });
  }, [showAthLine, athLineAvailable, athPriceInSol, metric, toValue, showChart, style]);

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

  // Dashed horizontal lines at the strategy's entry/exit fill prices. `bars` is a
  // dep so the lines are re-created on the fresh series after a grouping/interval
  // change recreates it (mirrors how the markers effect re-runs).
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

  useEffect(() => {
    const chart = chartRef.current;
    if (!chart || !showChart) return;

    const mainSeries = seriesRef.current;
    const savedViewport =
      mainSeries != null ? snapshotVisibleViewport(chart, mainSeries) : null;
    if (savedViewport) visibleViewportRef.current = savedViewport;

    for (const series of swingSeriesRefs.current) {
      chart.removeSeries(series);
    }
    swingSeriesRefs.current = [];
    swingSeriesLegRef.current.clear();
    swingOverlayMetaRef.current = null;
    setSwingTooltip(null);

    if (!showSwingOverlay || !swingOverlay?.legs.length) {
      if (savedViewport) {
        isRestoringViewportRef.current = true;
        reapplyChartViewport(chart.timeScale(), savedViewport, 'logical');
        requestAnimationFrame(() => {
          isRestoringViewportRef.current = false;
        });
      }
      return;
    }

    const segmentMode = swingOverlay.segmentMode ?? 'connected';
    swingOverlayMetaRef.current = {
      segmentMode,
      groupMode,
      legs: swingOverlay.legs,
      allLegs: swingOverlay.allLegs,
    };

    const addConnectedPath = (legs: ChartSwingLeg[]) => {
      const data = swingsToColoredLineData(
        legs,
        metric,
        toValue,
        groupMode,
        sortedTrades,
      );
      const pointCount = data.filter((p) => 'value' in p).length;
      if (pointCount < 2) return;
      const series = chart.addSeries(LineSeries, {
        ...SWING_HIGH_OVERLAY_SERIES_OPTIONS,
        priceFormat: createChartPriceFormat(priceUnit),
      });
      series.setData(data);
      swingSeriesRefs.current.push(series);
    };

    if (segmentMode === 'perLeg') {
      for (const leg of swingOverlay.legs) {
        const segment = buildLegSegment(leg, metric, toValue, groupMode, sortedTrades);
        if (!segment) continue;
        const series = chart.addSeries(LineSeries, {
          ...SWING_HIGH_OVERLAY_SERIES_OPTIONS,
          color: segment.color,
          priceFormat: createChartPriceFormat(priceUnit),
        });
        series.setData(segment.data);
        swingSeriesRefs.current.push(series);
        swingSeriesLegRef.current.set(series, leg);
      }
    } else if (segmentMode === 'connectedSequential') {
      const chains = groupSequentialLegChains(
        swingOverlay.legs,
        swingOverlay.allLegs ?? swingOverlay.legs,
      );
      for (const chain of chains) {
        addConnectedPath(chain);
      }
    } else {
      addConnectedPath(swingOverlay.legs);
    }

    if (savedViewport) {
      isRestoringViewportRef.current = true;
      reapplyChartViewport(chart.timeScale(), savedViewport, 'logical');
      requestAnimationFrame(() => {
        isRestoringViewportRef.current = false;
      });
    }

    return () => {
      if (!chartRef.current) return;
      for (const series of swingSeriesRefs.current) {
        try {
          chartRef.current.removeSeries(series);
        } catch {
          /* chart may already be removed */
        }
      }
      swingSeriesRefs.current = [];
    };
  }, [
    showSwingOverlay,
    swingOverlay,
    metric,
    toValue,
    groupMode,
    sortedTrades,
    showChart,
    style,
    priceUnit,
    snapshotVisibleViewport,
  ]);

  if (!id) {
    return (
      <Placeholder
        message="Select a token row to view price history."
        className={className}
        height={height}
      />
    );
  }

  if (loading) {
    return (
      <Placeholder
        message={`Loading trades for ${symbol}…`}
        className={className}
        height={height}
      />
    );
  }

  if (error) {
    return (
      <div
        className={panelClass(cn('flex items-center justify-center p-4 text-xs text-red', className))}
        style={{ ...panelStyle, height, borderColor: '#f2364544' }}
      >
        {error}
      </div>
    );
  }

  if (trades.length === 0) {
    return (
      <Placeholder
        message={`No trades recorded for ${symbol}.`}
        className={className}
        height={height}
      />
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
        showAthLine={showAthLine}
        athLineAvailable={athLineAvailable}
        showMigrationLine={showMigrationLine}
        trimEmptyBars={trimEmptyBars}
        swingOverlayAvailable={swingOverlayAvailable}
        showSwingOverlay={showSwingOverlay}
        connectSwings={connectSwings}
        chainHighlightAvailable={chainHighlightAvailable}
        showChainHighlight={showChainHighlight}
        rangeSelectMode={rangeSelectMode}
        crosshair={crosshair}
        isMigrated={isMigrated}
        isMayhemMode={isMayhemMode}
        isCashbackEnabled={isCashbackEnabled}
        onGroupModeChange={handleGroupModeChange}
        onIntervalChange={handleIntervalChange}
        onStyleChange={handleStyleChange}
        onMetricChange={onMetricChange}
        onShowTradeMarkersChange={handleShowTradeMarkersChange}
        onShowAthLineChange={handleShowAthLineChange}
        onShowMigrationLineChange={handleShowMigrationLineChange}
        onTrimEmptyBarsChange={handleTrimEmptyBarsChange}
        onShowSwingOverlayChange={setShowSwingOverlay}
        onConnectSwingsChange={setConnectSwings}
        onShowChainHighlightChange={setShowChainHighlight}
        onRangeSelectModeChange={setRangeSelectMode}
      />
      <div className="relative" style={{ height, width: '100%' }}>
        <div ref={containerRef} style={{ height: '100%', width: '100%' }} />
        {barTooltip && !swingTooltip && (
          <BarCrosshairTooltip
            tooltip={barTooltip}
            formatVol={formatVol}
            formatTime={formatBarTime}
          />
        )}
        {swingTooltip && showSwingOverlay && (
          <SwingCrosshairTooltip
            tooltip={swingTooltip}
            formatPrice={formatSwingPrice}
            formatAmount={formatSwingAmount}
          />
        )}
        {chainTooltip && (
          <ChainHighlightTooltip
            tooltip={chainTooltip}
            tradeCounts={chainTradeCounts}
            formatAmount={formatSwingAmount}
            formatPrice={formatSwingPrice}
          />
        )}
        {walletMarkersTooltip && (
          <WalletMarkersTooltip tooltip={walletMarkersTooltip} />
        )}
        {rangeTooltip && (
          <RangeSelectTooltip
            tooltip={rangeTooltip}
            formatAmount={formatSwingAmount}
            formatPrice={formatSwingPrice}
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
