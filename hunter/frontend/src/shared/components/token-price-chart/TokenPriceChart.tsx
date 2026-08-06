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
  alignFlowToBars,
  buildFlowLines,
  flowAt,
  flowSeriesScale,
  formatFlowTokenCount,
  FLOW_NON_VOL_LINE_COLOR,
  FLOW_VOL_LINE_COLOR,
  type FlowBasis,
  type FlowLines,
} from 'lib/flow/flowChartData';
import { attachDualPriceScaleSync, type DualPriceScaleSync } from './dualPriceScaleSync';
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
  barAgeSec,
  barsToCandleData,
  barsToLineData,
  barSelectionMarker,
  buildBarEarliestTradeSec,
  compareTradesChronologically,
  computeRangeStats,
  dropEmptyBars,
  tokenCreatedAtSec,
  tradeBarSlot,
  tradeBarTime,
} from './chartBars';
import { ChartRangeSlider } from './ChartRangeSlider';
import { ChartToolbar } from './ChartToolbar';
import { createChartTimeFormatters } from './chartTimezone';
import { useTimezone } from 'context/TimezoneContext';
import { useProfileWallets } from 'hooks/useProfileWallets';
import { cn } from 'lib/cn';
import {
  CANDLE_SERIES_OPTIONS,
  CHART_COLORS,
  CHART_INTERVALS,
  createChartOptions,
  createChartPriceFormat,
  createChartPriceFormatter,
  DEFAULT_CHART_PREFS,
  DUAL_CHART_HANDLE_SCALE,
  LINE_SERIES_OPTIONS,
  LS_CHART_PREFS_KEY,
  responsiveChartHeight,
  SERIES_BY_STYLE,
  TOKEN_TOTAL_SUPPLY,
} from './constants';
import { getString, setString } from 'lib/storage';
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
  ChartWalletMarkersTooltipState,
  ChartTrade,
  OhlcBar,
  ProfileWalletInfo,
  TokenPriceChartProps,
  WalletBarActivity,
} from './types';

type ChartPrefs = typeof DEFAULT_CHART_PREFS;

function loadPrefs(): ChartPrefs {
  try {
    const raw = getString(LS_CHART_PREFS_KEY);
    if (raw) {
      const parsed = JSON.parse(raw) as Partial<ChartPrefs>;
      // Merge over defaults so a key added after this blob was written falls back
      // instead of coming through undefined.
      return { ...DEFAULT_CHART_PREFS, ...parsed };
    }
  } catch {
    /* ignore */
  }
  return DEFAULT_CHART_PREFS;
}

/** Read-merge-write a subset of prefs. Each toggle handler persists only the
 *  field it changed (`savePrefs({ showWalletMarkers: next })`); the rest are
 *  read fresh from storage, so handlers never need to close over sibling state. */
function savePrefs(patch: Partial<ChartPrefs>) {
  setString(LS_CHART_PREFS_KEY, JSON.stringify({ ...loadPrefs(), ...patch }));
}

export function buildTradeMarkers(
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
      color: onlyBuy ? CHART_COLORS.buy : onlySell ? CHART_COLORS.sell : CHART_COLORS.text,
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
    const isSignal = m.role === 'signal';
    out.push({
      time,
      position: isEntry ? 'belowBar' : 'aboveBar',
      color: isSignal
        ? isEntry
          ? CHART_COLORS.signalEntry
          : CHART_COLORS.signalExit
        : isEntry
          ? CHART_COLORS.entry
          : CHART_COLORS.exit,
      // Fills keep directional arrows; metric signals are circles so the two
      // layers stay glanceably distinct when both are on the chart.
      shape: isSignal ? 'circle' : isEntry ? 'arrowUp' : 'arrowDown',
      text: m.label ?? (isEntry ? 'Entry' : 'Exit'),
      size: isSignal ? 1 : 2,
    });
  }
  return out;
}

export type MarkersPlugin = {
  setMarkers: (markers: SeriesMarker<UTCTimestamp>[]) => void;
  detach: () => void;
};

export function sortSeriesMarkers(
  markers: SeriesMarker<UTCTimestamp>[],
): SeriesMarker<UTCTimestamp>[] {
  return [...markers].sort((a, b) => (a.time as number) - (b.time as number));
}

/** Silhouette per wallet CLASS: `mine` → diamond (identity wins, permanent),
 *  dev/creator → triangle, else focused/input → hexagon, else circle. Focus still
 *  layers its gold ring on top of whatever shape, so a focused `mine` wallet stays
 *  a diamond and a focused dev stays a triangle. */
function walletShape(w: ProfileWalletInfo): MarkerShape {
  if (w.isMine) return 'diamond';
  if (w.isDev) return 'triangle';
  if (w.isHighlighted) return 'hexagon';
  return 'circle';
}

function walletGlyph(w: ProfileWalletInfo): string {
  if (w.isMine) return '★';
  if (w.isDev) return 'D';
  return (w.profileName ?? w.label ?? w.address).charAt(0).toUpperCase();
}

/** Below this fraction of the episode's peak balance, a sell is treated as a full
 *  exit (fee/rounding dust rarely leaves the balance at exactly zero). */
const SELL_ALL_DUST_FRACTION = 0.02;

export function buildWalletMarkerDefs(
  // Must be in canonical order (`slot → tx_index → leg_index`) — position tracking
  // for first_buy/sell_all replays each wallet's trades in execution order.
  sortedTrades: ChartTrade[],
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

  // Lifecycle roles keyed `${address}:${barTime}:${type}`. Computed by replaying
  // each wallet's running token balance in canonical order, so first_buy/sell_all
  // land on the bucket that actually opened/closed the position.
  const roles = new Map<string, 'first_buy' | 'sell_all'>();
  const pos = new Map<string, number>();   // running balance (raw token units)
  const peak = new Map<string, number>();  // peak balance in the current holding episode
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

    // Lifecycle role — replay position before the per-bucket dedup below (every
    // trade must advance the balance, not just the first of its bucket).
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
        peak.set(addr, 0); // episode closed; a later re-accumulation can flag again
        pos.set(addr, 0);
      }
    }

    // One marker per wallet per type per bar.
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

    // Buys stack downward below the bar low, sells stack upward above the bar high
    // — the two sides never collide, so each restarts its own stack index at 0.
    let buyStack = 0;
    for (const w of buy) {
      defs.push({
        barTime,
        barEdgePrice: bar.low,
        letter: walletGlyph(w),
        color: w.color,
        borderColor: CHART_COLORS.buy,
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
        borderColor: CHART_COLORS.sell,
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

export function buildWalletBarActivityMap(
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
  height: fixedHeight,
  onBarClick,
  selectedBar = null,
  onRangeChange,
  onCrosshairTimeChange,
  externalCrosshairTimeSec = null,
  onVisibleTimeRangeChange,
  athPriceInSol = null,
  isMigrated,
  isMayhemMode,
  isCashbackEnabled,
  profileWallets,
  creatorWallet = null,
  tokenCreatedAt,
  eventMarkers = null,
  flowPatternKeys = null,
  flowBasis = 'cost_sol',
}: TokenPriceChartProps) {
  // Tracked-wallet markers are a project-wide invariant: EVERY token trade chart
  // renders them. Callers may supply `profileWallets` (e.g. `TokenTradeChart`,
  // which augments the tracked set with the highlighted/synthetic input wallet);
  // when a caller omits the prop we fall back to the tracked profile wallets
  // here, so a token trade chart can never render without them — by construction,
  // not by convention.
  const trackedProfileWallets = useProfileWallets();
  const effectiveProfileWallets = profileWallets ?? trackedProfileWallets;

  // The token's dev/creator as a synthetic tracked wallet — folded into the same
  // marker pipeline so its first_buy/sell_all lifecycle + triangle silhouette
  // come for free (mirrors `FlowPreviewChart`). No parallel dev-marker path.
  const devWallet = useMemo<ProfileWalletInfo | null>(
    () =>
      creatorWallet
        ? {
            address: creatorWallet,
            label: 'Dev',
            profileName: 'Dev',
            color: CHART_COLORS.dev,
            tags: [],
            isDev: true,
          }
        : null,
    [creatorWallet],
  );

  const containerRef = useRef<HTMLDivElement>(null);
  const chartRef = useRef<IChartApi | null>(null);
  const onBarClickRef = useRef(onBarClick);
  onBarClickRef.current = onBarClick;
  const onRangeChangeRef = useRef(onRangeChange);
  onRangeChangeRef.current = onRangeChange;
  const onCrosshairTimeChangeRef = useRef(onCrosshairTimeChange);
  onCrosshairTimeChangeRef.current = onCrosshairTimeChange;
  const onVisibleTimeRangeChangeRef = useRef(onVisibleTimeRangeChange);
  onVisibleTimeRangeChangeRef.current = onVisibleTimeRangeChange;
  /** True while applying {@link externalCrosshairTimeSec} so the resulting
   *  subscribeCrosshairMove echo doesn't bounce back to the sibling. */
  const applyingExternalCrosshairRef = useRef(false);
  const prevExternalCrosshairRef = useRef<number | null>(null);
  const seriesRef = useRef<ISeriesApi<'Line' | 'Candlestick'> | null>(null);
  const volSeriesRef = useRef<ISeriesApi<'Line'> | null>(null);
  const nonVolSeriesRef = useRef<ISeriesApi<'Line'> | null>(null);
  const sortedTradesRef = useRef<ChartTrade[]>([]);
  const markersPluginRef = useRef<MarkersPlugin | null>(null);
  const walletMarkersPrimRef = useRef<WalletMarkersPlugin | null>(null);
  const rangeSelectPrimRef = useRef<RangeSelectPlugin | null>(null);
  const barsRef = useRef<OhlcBar[]>([]);
  const alignedFlowLinesRef = useRef<FlowLines>({ vol: [], nonVol: [] });

  // Height tracks width unless the caller pins it (`fixedHeight`). A fluid width
  // with a fixed height renders wide-and-flat on a big monitor; deriving height
  // from width keeps a readable aspect ratio. Set from the measured width at
  // chart creation (below); resize stays width-only, so this never feeds a
  // height->width->height loop (see the ResizeObserver note in the create effect).
  const [chartHeight, setChartHeight] = useState(() => fixedHeight ?? responsiveChartHeight(0));
  /** Measured panel width — only so a crosshair tooltip can flip at the right edge
   *  instead of spilling out of a narrow panel. Written from the same width-only
   *  ResizeObserver below, so it changes at most once per real resize. */
  const [chartWidth, setChartWidth] = useState(0);

  const initialPrefs = loadPrefs();
  const [groupMode, setGroupMode] = useState<ChartGroupMode>(initialPrefs.groupMode);
  const [interval, setInterval] = useState<ChartInterval>(initialPrefs.interval);
  const [style, setStyle] = useState<ChartStyle>(initialPrefs.style);
  const [showTradeMarkers, setShowTradeMarkers] = useState(initialPrefs.showTradeMarkers);
  const [showAthLine, setShowAthLine] = useState(initialPrefs.showAthLine);
  const [showMigrationLine, setShowMigrationLine] = useState(initialPrefs.showMigrationLine);
  const [trimEmptyBars, setTrimEmptyBars] = useState(initialPrefs.trimEmptyBars);
  const [showWalletMarkers, setShowWalletMarkers] = useState(initialPrefs.showWalletMarkers);
  const [showDevMarkers, setShowDevMarkers] = useState(initialPrefs.showDevMarkers);
  const [devMarkersBoundariesOnly, setDevMarkersBoundariesOnly] = useState(
    initialPrefs.devMarkersBoundariesOnly,
  );
  const [showEventMarkers, setShowEventMarkers] = useState(initialPrefs.showEventMarkers);
  const [showFlowLines, setShowFlowLines] = useState(initialPrefs.showFlowLines);
  /** Same gate as the trades-table Vol badge — empty/omit ⇒ no overlay. */
  const flowLinesAvailable = flowPatternKeys != null && flowPatternKeys.size > 0;
  const flowLinesVisible = showFlowLines && flowLinesAvailable;
  const flowLinesAvailableRef = useRef(flowLinesAvailable);
  flowLinesAvailableRef.current = flowLinesAvailable;
  const { timezone: chartTimezone } = useTimezone();
  const [rangeSelectMode, setRangeSelectMode] = useState(false);
  const [selectedRange, setSelectedRange] = useState<ChartRangeSelection | null>(null);
  const [crosshair, setCrosshair] = useState<ChartCrosshairInfo | null>(null);
  const [barTooltip, setBarTooltip] = useState<ChartBarTooltipState | null>(null);
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
  const selectedBarTimeRef = useRef(selectedBarTime);
  selectedBarTimeRef.current = selectedBarTime;

  const shouldFitContentRef = useRef(true);
  const prevIdRef = useRef(id);
  const prevGroupingKeyRef = useRef(groupingKey);
  const visibleViewportRef = useRef<ChartViewport | null>(null);
  const isRestoringViewportRef = useRef(false);
  const mountedSeriesStyleRef = useRef<ChartStyle | null>(null);
  const prevBarsSignatureRef = useRef<string | null>(null);
  const scaleSyncRef = useRef<DualPriceScaleSync | null>(null);
  /** Non-data inputs that change what the price axes MEAN. Only these justify
   *  dropping a hand-set Y zoom; a new trade never does. */
  const flowScaleResetKeyRef = useRef<string | null>(null);
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

  // Token creation time (epoch seconds) for per-bar tx age in the crosshair tooltip.
  const createdAtSec = useMemo(() => tokenCreatedAtSec(tokenCreatedAt), [tokenCreatedAt]);

  const barEarliestTradeSec = useMemo(
    () => buildBarEarliestTradeSec(sortedTrades, groupMode, intervalSec),
    [sortedTrades, groupMode, intervalSec],
  );

  const computeBarAgeSec = useCallback(
    (barTime: number): number | null =>
      barAgeSec(barTime, createdAtSec, barEarliestTradeSec, groupMode),
    [createdAtSec, barEarliestTradeSec, groupMode],
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

  const formatChartPrice = useMemo(
    () => createChartPriceFormatter(priceUnit),
    [priceUnit],
  );
  /** Format a chart-Y price magnitude (priceInSol) in the display unit, honoring
   *  the price/MC metric — used by the range-selection tooltip. */
  const formatChartValuePrice = useCallback(
    (priceInSol: number) => {
      const chartY = metric === 'price' ? priceInSol : TOKEN_TOTAL_SUPPLY * priceInSol;
      return formatChartPrice(toValue(chartY));
    },
    [metric, toValue, formatChartPrice],
  );
  const formatBarTime = useCallback(
    (barTime: UTCTimestamp) => {
      if (groupMode === 'slot') return `Slot ${barTime}`;
      return createChartTimeFormatters(chartTimezone).timeFormatter(barTime);
    },
    [groupMode, chartTimezone],
  );
  const formatVol = useMemo(() => createChartPriceFormatter('SOL'), []);
  const formatFlow = useCallback(
    (v: number) =>
      flowBasis === 'token' ? formatFlowTokenCount(v) : formatChartPrice(toValue(v)),
    [flowBasis, formatChartPrice, toValue],
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

  // crosshair-move fires on every pixel; without coalescing each move triggers a
  // full TokenPriceChart + ChartToolbar re-render. Collect the latest tooltip
  // snapshot per move and flush all setters once per animation frame so hovering
  // costs at most one render per frame instead of one per pixel.
  const crosshairRafRef = useRef<number | null>(null);
  const pendingCrosshairRef = useRef<{
    crosshair: ChartCrosshairInfo | null;
    barTooltip: ChartBarTooltipState | null;
    rangeTooltip: ChartRangeTooltipState | null;
    walletMarkersTooltip: ChartWalletMarkersTooltipState | null;
    crosshairTimeSec: number | null;
  } | null>(null);
  const groupModeRef = useRef(groupMode);
  groupModeRef.current = groupMode;

  const athLineAvailable = athChartValue(athPriceInSol, metric, toValue) != null;

  const handleGroupModeChange = useCallback((next: ChartGroupMode) => {
    setGroupMode(next);
    savePrefs({ groupMode: next });
    onBarClickRef.current?.(null);
  }, []);

  const handleIntervalChange = useCallback((next: ChartInterval) => {
    setInterval(next);
    savePrefs({ interval: next });
    onBarClickRef.current?.(null);
  }, []);

  const handleStyleChange = useCallback((next: ChartStyle) => {
    setStyle(next);
    savePrefs({ style: next });
  }, []);

  const handleShowTradeMarkersChange = useCallback((next: boolean) => {
    setShowTradeMarkers(next);
    savePrefs({ showTradeMarkers: next });
  }, []);

  const handleShowAthLineChange = useCallback((next: boolean) => {
    setShowAthLine(next);
    savePrefs({ showAthLine: next });
  }, []);

  const handleShowMigrationLineChange = useCallback((next: boolean) => {
    setShowMigrationLine(next);
    savePrefs({ showMigrationLine: next });
  }, []);

  const handleTrimEmptyBarsChange = useCallback((next: boolean) => {
    setTrimEmptyBars(next);
    savePrefs({ trimEmptyBars: next });
  }, []);

  const handleShowWalletMarkersChange = useCallback((next: boolean) => {
    setShowWalletMarkers(next);
    savePrefs({ showWalletMarkers: next });
  }, []);

  const handleShowDevMarkersChange = useCallback((next: boolean) => {
    setShowDevMarkers(next);
    savePrefs({ showDevMarkers: next });
  }, []);

  const handleDevMarkersBoundariesOnlyChange = useCallback((next: boolean) => {
    setDevMarkersBoundariesOnly(next);
    savePrefs({ devMarkersBoundariesOnly: next });
  }, []);

  const handleShowEventMarkersChange = useCallback((next: boolean) => {
    setShowEventMarkers(next);
    savePrefs({ showEventMarkers: next });
  }, []);

  const handleShowFlowLinesChange = useCallback((next: boolean) => {
    setShowFlowLines(next);
    savePrefs({ showFlowLines: next });
  }, []);

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
    const width = rect.width || el.clientWidth;
    // Derive height from the measured WIDTH (one-way) so the chart isn't a
    // wide-flat band on a big monitor. Resize stays width-only (below), so this
    // never becomes a height->width->height feedback loop.
    const initialHeight = fixedHeight ?? responsiveChartHeight(width);
    setChartHeight(initialHeight);
    const chart = createChart(
      el,
      createChartOptions(width, initialHeight, groupMode, priceUnit, chartTimezone, {
        dualPriceScale: true,
      }),
    );
    chartRef.current = chart;

    const volSeries = chart.addSeries(LineSeries, {
      color: FLOW_VOL_LINE_COLOR,
      lineWidth: 2,
      priceScaleId: 'left',
      title: 'Vol makers',
      lastValueVisible: true,
      priceLineVisible: false,
      visible: false,
    });
    const nonVolSeries = chart.addSeries(LineSeries, {
      color: FLOW_NON_VOL_LINE_COLOR,
      lineWidth: 2,
      priceScaleId: 'left',
      title: 'Non-vol',
      lastValueVisible: true,
      priceLineVisible: false,
      visible: false,
    });
    chart.priceScale('left').applyOptions({ visible: false });
    volSeriesRef.current = volSeries;
    nonVolSeriesRef.current = nonVolSeries;

    const scaleSync = attachDualPriceScaleSync(chart, el, {
      isPaused: () => rangeSelectModeRef.current,
    });
    scaleSyncRef.current = scaleSync;

    // Width-only resize. Feeding contentRect.height back into applyOptions fights
    // the fixed parent height and the inspect-modal scrollbar (content grows →
    // gutter appears → width/height thrash → visible vibration / style break).
    let lastWidth = Math.round(rect.width || el.clientWidth || 0);
    setChartWidth(lastWidth);
    const ro = new ResizeObserver((entries) => {
      const width = Math.round(entries[0]?.contentRect.width ?? 0);
      if (width > 0 && width !== lastWidth) {
        lastWidth = width;
        chartRef.current?.applyOptions({ width });
        setChartWidth(width);
      }
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
      if (!applyingExternalCrosshairRef.current) {
        onCrosshairTimeChangeRef.current?.(next.crosshairTimeSec);
      }
    };
    const scheduleCrosshair = () => {
      if (crosshairRafRef.current == null) {
        crosshairRafRef.current = requestAnimationFrame(flushCrosshair);
      }
    };

    chart.subscribeCrosshairMove((param) => {
      // Each move accumulates into a single snapshot flushed once per frame (see
      // pendingCrosshairRef). Local setters below write the snapshot instead of
      // calling React setters per pixel; the rAF coalesces them into one render.
      const next = {
        crosshair: null as ChartCrosshairInfo | null,
        barTooltip: null as ChartBarTooltipState | null,
        rangeTooltip: null as ChartRangeTooltipState | null,
        walletMarkersTooltip: null as ChartWalletMarkersTooltipState | null,
        crosshairTimeSec: null as number | null,
      };
      pendingCrosshairRef.current = next;
      const setCrosshair = (v: ChartCrosshairInfo | null) => {
        next.crosshair = v;
      };
      const setBarTooltip = (v: ChartBarTooltipState | null) => {
        next.barTooltip = v;
      };
      const setRangeTooltip = (v: ChartRangeTooltipState | null) => {
        next.rangeTooltip = v;
      };
      const setWalletMarkersTooltip = (v: ChartWalletMarkersTooltipState | null) => {
        next.walletMarkersTooltip = v;
      };
      const setCrosshairTimeSec = (v: number | null) => {
        next.crosshairTimeSec = v;
      };
      // Schedule the single per-frame flush; every early return below has already
      // recorded its intent into `next` via the shadowed setters above.
      scheduleCrosshair();

      // Hovering the range-selection label chip shows the range totals tooltip
      // and suppresses every other tooltip.
      const onRangeLabel =
        param.point != null &&
        (rangeSelectPrimRef.current?.containsLabelPoint(param.point.x, param.point.y) ??
          false);
      if (onRangeLabel && selectedRangeRef.current && rangeStatsRef.current && param.point) {
        setRangeTooltip({ stats: rangeStatsRef.current, point: param.point });
        setBarTooltip(null);
        setWalletMarkersTooltip(null);
        setCrosshairTimeSec(null);
        return;
      }
      setRangeTooltip(null);

      if (!param.time) {
        setCrosshair(null);
        setBarTooltip(null);
        setWalletMarkersTooltip(null);
        setCrosshairTimeSec(null);
        return;
      }
      const bar = barsRef.current.find((b) => b.time === param.time);
      if (!bar) {
        setCrosshair(null);
        setBarTooltip(null);
        setWalletMarkersTooltip(null);
        setCrosshairTimeSec(null);
        return;
      }
      const flow = flowLinesAvailableRef.current
        ? flowAt(alignedFlowLinesRef.current, param.time)
        : { vol: null, nonVol: null };
      const info: ChartCrosshairInfo = {
        open: bar.open,
        high: bar.high,
        low: bar.low,
        close: bar.close,
        volume: bar.volume,
        inflow: bar.inflow,
        outflow: bar.outflow,
        liquiditySol: bar.liquiditySol,
        flowVol: flow.vol,
        flowNonVol: flow.nonVol,
      };
      setCrosshair(info);
      // Resolve wall-clock seconds for sibling panes (metric series). Time mode
      // uses the bar key directly; slot mode maps to the last trade in that slot.
      if (groupModeRef.current === 'time') {
        setCrosshairTimeSec(Number(param.time));
      } else {
        const slot = Number(param.time);
        let wall: number | null = null;
        for (let i = sortedTradesRef.current.length - 1; i >= 0; i--) {
          const t = sortedTradesRef.current[i];
          if (t.slot === slot) {
            const ms = Date.parse(t.block_time);
            wall = Number.isFinite(ms) ? ms / 1000 : null;
            break;
          }
        }
        setCrosshairTimeSec(wall);
      }
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
      // In range-select mode the pointer-drag handler owns clicks; don't also
      // toggle a bar selection.
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
      if (!range) {
        onVisibleTimeRangeChangeRef.current?.(null);
        return;
      }
      const from = Number(range.from);
      const to = Number(range.to);
      setSliderWindow((prev) =>
        prev && prev.from === from && prev.to === to ? prev : { from, to },
      );
      // Slot mode's time scale is slot indices — only forward wall-clock windows.
      if (groupModeRef.current === 'time') {
        onVisibleTimeRangeChangeRef.current?.({ from, to });
      } else {
        onVisibleTimeRangeChangeRef.current?.(null);
      }
    };
    chart.timeScale().subscribeVisibleTimeRangeChange(onVisibleTimeRangeChange);

    return () => {
      chart.timeScale().unsubscribeVisibleLogicalRangeChange(onVisibleLogicalRangeChange);
      chart.timeScale().unsubscribeVisibleTimeRangeChange(onVisibleTimeRangeChange);
      scaleSync.detach();
      scaleSyncRef.current = null;
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
      volSeriesRef.current = null;
      nonVolSeriesRef.current = null;
      chart.remove();
      chartRef.current = null;
      setCrosshair(null);
      setBarTooltip(null);
      onCrosshairTimeChangeRef.current?.(null);
      onVisibleTimeRangeChangeRef.current?.(null);
      setRangeTooltip(null);
      setWalletMarkersTooltip(null);
    };
  }, [showChart, fixedHeight, groupingKey, groupMode, priceUnit, chartTimezone]);

  useEffect(() => {
    const series = seriesRef.current;
    if (!series || !showChart) return;
    // Dual-axis: do not set chart-level localization.priceFormatter — it would
    // override the left (flow) series formatters. Main series owns its labels.
    series.applyOptions({ priceFormat: createChartPriceFormat(priceUnit) });
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

  // Vol/non-vol cumulative overlay (left price scale) — only when fingerprint
  // has non-empty volume_ix_patterns (matches trades-table Vol badge gate).
  const flowLines = useMemo(() => {
    if (!flowLinesAvailable || !flowPatternKeys) {
      return { vol: [], nonVol: [] } satisfies FlowLines;
    }
    return buildFlowLines(sortedTrades, groupMode, intervalSec, flowBasis as FlowBasis, {
      patternKeys: flowPatternKeys,
      creatorWallet,
    });
  }, [
    sortedTrades,
    groupMode,
    intervalSec,
    flowBasis,
    flowLinesAvailable,
    flowPatternKeys,
    creatorWallet,
  ]);
  const alignedFlowLines = useMemo(() => alignFlowToBars(flowLines, bars), [flowLines, bars]);
  alignedFlowLinesRef.current = alignedFlowLines;

  useEffect(() => {
    if (!showChart) return;
    const tokenScale = flowSeriesScale(flowBasis as FlowBasis);
    const priceFormat =
      flowBasis === 'token'
        ? {
            type: 'custom' as const,
            formatter: (v: number) => {
              const n = v * tokenScale;
              if (Math.abs(n) >= 1e12) return `${(n / 1e12).toFixed(2)}T`;
              if (Math.abs(n) >= 1e9) return `${(n / 1e9).toFixed(2)}B`;
              if (Math.abs(n) >= 1e6) return `${(n / 1e6).toFixed(2)}M`;
              return n.toFixed(0);
            },
            minMove: 0.01,
          }
        : {
            type: 'custom' as const,
            formatter: createChartPriceFormatter(priceUnit),
            minMove: 0.01,
          };
    const toData = (pts: { time: UTCTimestamp; value: number }[]) =>
      pts.map((p) => ({
        time: p.time,
        value:
          flowBasis === 'token' ? p.value / tokenScale : toValue(p.value),
      }));
    volSeriesRef.current?.applyOptions({ priceFormat, visible: flowLinesVisible });
    nonVolSeriesRef.current?.applyOptions({ priceFormat, visible: flowLinesVisible });
    const chart = chartRef.current;
    if (chart) {
      // Re-fit only when what an axis MEANS changed (overlay toggled, unit/basis
      // switched) — never on a data update. `alignedFlowLines` and `toValue` are
      // deps of this effect and both churn on every live trade / SOL-price tick,
      // so an unconditional re-fit here re-armed autoScale continuously and threw
      // away the user's hand-set price zoom.
      const resetKey = `${flowLinesVisible}|${flowBasis}|${priceUnit}|${style}|${groupingKey}`;
      if (flowScaleResetKeyRef.current !== resetKey) {
        flowScaleResetKeyRef.current = resetKey;
        scaleSyncRef.current?.reset();
      }
      chart.priceScale('left').applyOptions({ visible: flowLinesVisible });
    }
    volSeriesRef.current?.setData(toData(alignedFlowLines.vol));
    nonVolSeriesRef.current?.setData(toData(alignedFlowLines.nonVol));
  }, [
    alignedFlowLines,
    flowBasis,
    priceUnit,
    toValue,
    flowLinesVisible,
    showChart,
    style,
    groupingKey,
  ]);

  // Sibling panes (metric series) drive the price-chart crosshair via wall-clock time.
  useEffect(() => {
    const chart = chartRef.current;
    const series = seriesRef.current;
    if (!chart || !series || !showChart) return;

    if (externalCrosshairTimeSec == null) {
      if (prevExternalCrosshairRef.current != null) {
        applyingExternalCrosshairRef.current = true;
        chart.clearCrosshairPosition();
        applyingExternalCrosshairRef.current = false;
      }
      prevExternalCrosshairRef.current = null;
      return;
    }
    prevExternalCrosshairRef.current = externalCrosshairTimeSec;

    let barTime: number | null = null;
    let price: number | null = null;

    if (groupMode === 'time') {
      // Nearest bar by bucket-start seconds.
      let best: (typeof bars)[number] | null = null;
      let bestDist = Infinity;
      for (const b of bars) {
        const d = Math.abs(Number(b.time) - externalCrosshairTimeSec);
        if (d < bestDist) {
          bestDist = d;
          best = b;
        }
      }
      if (best) {
        barTime = Number(best.time);
        price = best.close;
      }
    } else {
      // Slot mode: map wall-clock → trade → slot bar.
      let bestTrade: (typeof sortedTrades)[number] | null = null;
      let bestDist = Infinity;
      for (const t of sortedTrades) {
        const ms = Date.parse(t.block_time);
        if (!Number.isFinite(ms)) continue;
        const d = Math.abs(ms / 1000 - externalCrosshairTimeSec);
        if (d < bestDist) {
          bestDist = d;
          bestTrade = t;
        }
      }
      if (bestTrade?.slot != null) {
        const bar = bars.find((b) => Number(b.time) === bestTrade!.slot);
        if (bar) {
          barTime = Number(bar.time);
          price = bar.close;
        }
      }
    }

    if (barTime == null || price == null) return;
    applyingExternalCrosshairRef.current = true;
    chart.setCrosshairPosition(price, barTime as UTCTimestamp, series);
    // setCrosshairPosition fires subscribeCrosshairMove synchronously — clear
    // the guard after the current stack so the echo is swallowed.
    queueMicrotask(() => {
      applyingExternalCrosshairRef.current = false;
    });
  }, [externalCrosshairTimeSec, showChart, bars, sortedTrades, groupMode]);

  useEffect(() => {
    const series = seriesRef.current;
    if (!series || !showChart) return;

    const markers: SeriesMarker<UTCTimestamp>[] = [];

    if (showTradeMarkers) {
      markers.push(...buildTradeMarkers(trades, groupMode, intervalSec));
    }

    // Tracked-wallet + dev markers share ONE plugin: compose the wallet list from
    // the two independent toggles (dev appended LAST so a dev that's also tracked
    // dedups to the dev entry — `buildWalletMarkerDefs` keys by address, last write
    // wins), then run the pipeline once.
    const markerWallets: ProfileWalletInfo[] = [];
    if (showWalletMarkers) markerWallets.push(...effectiveProfileWallets);
    if (showDevMarkers && devWallet) markerWallets.push(devWallet);
    if (markerWallets.length > 0) {
      let walletDefs = buildWalletMarkerDefs(sortedTrades, markerWallets, bars, groupMode, intervalSec);
      if (devMarkersBoundariesOnly) {
        // Keep only the dev's lifecycle boundaries (first_buy/sell_all) — drop its
        // mid-position adds/trims. Dev defs are the only triangles; tracked-wallet
        // markers (other shapes) pass through untouched.
        walletDefs = walletDefs.filter((d) => d.shape !== 'triangle' || d.role != null);
      }
      walletMarkersPrimRef.current?.setMarkers(walletDefs);
      walletActivityMapRef.current = buildWalletBarActivityMap(trades, markerWallets, groupMode, intervalSec);
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
    showWalletMarkers,
    showDevMarkers,
    devMarkersBoundariesOnly,
    devWallet,
    eventMarkers,
  ]);

  // Render the committed range selection as a band with a duration chip. Keyed
  // on style/grouping so it re-applies after the series (and its plugin) is
  // recreated; the live drag preview is driven directly from the pointer
  // handlers below. `rangeSelectMode` is a dep so flipping the mode wipes any
  // stale draft band left over from an interrupted drag. `bars` is intentionally
  // NOT a dep: a per-tick bars change reuses the existing series/plugin, so
  // re-running here would only repaint the unchanged band.
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
        chart.applyOptions({
          handleScroll: true,
          handleScale: { ...DUAL_CHART_HANDLE_SCALE },
        });
        scaleSyncRef.current?.rearm();
      }
    };
  }, [showChart, fixedHeight, groupingKey, groupMode, priceUnit, chartTimezone, rangeSelectMode]);

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

    if (!showEventMarkers || !eventMarkers) return;

    // Price lines are fill-only — metric signal markers already label the bar;
    // a second dashed line at nearly the same price reads as a duplicate fill.
    for (const m of eventMarkers) {
      if (m.role === 'signal') continue;
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
  }, [eventMarkers, showEventMarkers, metric, toValue, showChart, style, bars]);

  if (!id) {
    return (
      <Placeholder
        message="Select a token row to view price history."
        className={className}
        height={chartHeight}
      />
    );
  }

  if (loading) {
    return (
      <Placeholder
        message={`Loading trades for ${symbol}…`}
        className={className}
        height={chartHeight}
      />
    );
  }

  if (error) {
    return (
      <div
        className={panelClass(cn('flex items-center justify-center p-4 text-xs text-red', className))}
        style={{ ...panelStyle, height: chartHeight, borderColor: '#f2364544' }}
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
        height={chartHeight}
      />
    );
  }

  if (bars.length === 0) {
    return (
      <Placeholder
        message={`No chart data for ${symbol} at ${groupMode === 'slot' ? 'slot' : interval} grouping.`}
        className={className}
        height={chartHeight}
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
        showWalletMarkers={showWalletMarkers}
        showDevMarkers={showDevMarkers}
        devMarkersAvailable={devWallet != null}
        devMarkersBoundariesOnly={devMarkersBoundariesOnly}
        showEventMarkers={showEventMarkers}
        eventMarkersAvailable={!!eventMarkers && eventMarkers.length > 0}
        showAthLine={showAthLine}
        athLineAvailable={athLineAvailable}
        showMigrationLine={showMigrationLine}
        trimEmptyBars={trimEmptyBars}
        showFlowLines={showFlowLines}
        flowLinesAvailable={flowLinesAvailable}
        rangeSelectMode={rangeSelectMode}
        crosshair={crosshair}
        formatFlow={formatFlow}
        isMigrated={isMigrated}
        isMayhemMode={isMayhemMode}
        isCashbackEnabled={isCashbackEnabled}
        onGroupModeChange={handleGroupModeChange}
        onIntervalChange={handleIntervalChange}
        onStyleChange={handleStyleChange}
        onMetricChange={onMetricChange}
        onShowTradeMarkersChange={handleShowTradeMarkersChange}
        onShowWalletMarkersChange={handleShowWalletMarkersChange}
        onShowDevMarkersChange={handleShowDevMarkersChange}
        onDevMarkersBoundariesOnlyChange={handleDevMarkersBoundariesOnlyChange}
        onShowEventMarkersChange={handleShowEventMarkersChange}
        onShowAthLineChange={handleShowAthLineChange}
        onShowMigrationLineChange={handleShowMigrationLineChange}
        onTrimEmptyBarsChange={handleTrimEmptyBarsChange}
        onShowFlowLinesChange={handleShowFlowLinesChange}
        onRangeSelectModeChange={setRangeSelectMode}
      />
      <div className="relative" style={{ height: chartHeight, width: '100%' }}>
        <div ref={containerRef} style={{ height: '100%', width: '100%' }} />
        {barTooltip && (
          <BarCrosshairTooltip
            tooltip={barTooltip}
            formatVol={formatVol}
            formatFlow={formatFlow}
            formatTime={formatBarTime}
            containerWidth={chartWidth}
          />
        )}
        {walletMarkersTooltip && (
          <WalletMarkersTooltip
            tooltip={walletMarkersTooltip}
            containerWidth={chartWidth}
          />
        )}
        {rangeTooltip && (
          <RangeSelectTooltip
            tooltip={rangeTooltip}
            formatAmount={formatVol}
            formatPrice={formatChartValuePrice}
            containerWidth={chartWidth}
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
