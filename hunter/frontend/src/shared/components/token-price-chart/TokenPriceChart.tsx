import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  createChart,
  createSeriesMarkers,
  LineSeries,
  LineStyle,
  type IChartApi,
  type IPriceLine,
  type ISeriesApi,
  type ITimeScaleApi,
  type LogicalRangeChangeEventHandler,
  type SeriesMarker,
  type Time,
  type UTCTimestamp,
} from 'lightweight-charts';
import {
  alignFlowToBars,
  buildFlowLines,
  EMPTY_FLOW_LINES,
  flowAt,
  formatFlowTokenCount,
  FLOW_NON_VOL_LINE_COLOR,
  FLOW_VOL_LINE_COLOR,
  type FlowBasis,
  type FlowLinePoint,
  type FlowLines,
} from 'lib/flow/flowChartData';
import { classifyOptsForTag } from 'lib/flow/tapeClassify';
import { tagSentence } from 'lib/strategy/tagsDoc';
import { useFlowLensContext } from 'context/FlowLensContext';
import {
  barsShape,
  captureChartViewport,
  focusLogicalRange,
  reapplyChartViewport,
  type BarsShape,
  type ChartViewport,
} from './chartViewport';
import {
  aggregateTradesToBars,
  aggregateTradesToBarsBySlot,
  athChartValue,
  migrationChartValue,
  priceSolChartValue,
  barAgeSec,
  barsToCandleData,
  barsToLineData,
  barSelectionMarker,
  buildBarEarliestTradeSec,
  buildBarWallEndSec,
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
import { useStoredField } from 'hooks/useLocalStorage';
import { useProfileWallets } from 'hooks/useProfileWallets';
import { cn } from 'lib/cn';
import { STORAGE_KEYS } from 'lib/storage';
import {
  CANDLE_SERIES_OPTIONS,
  CHART_COLORS,
  LENS_TRACK_COLORS,
  CHART_INTERVALS,
  createChartPriceFormat,
  createChartPriceFormatter,
  DEFAULT_CHART_PREFS,
  CHART_HANDLE_SCALE,
  LINE_SERIES_OPTIONS,
  LS_CHART_PREFS_KEY,
  responsiveChartHeight,
  TOKEN_TOTAL_SUPPLY,
} from './constants';
import { PRICE_SCALE_MARGINS, createChartOptions, SERIES_BY_STYLE } from './chartOptions';
import { getString, setString } from 'lib/storage';
import { BarCrosshairTooltip } from './BarCrosshairTooltip';
import { LensLaneTooltip } from './LensLaneTooltip';
import { WalletMarkersTooltip } from './WalletMarkersTooltip';
import { RangeSelectTooltip, formatRangeDuration } from './RangeSelectTooltip';
import { WalletMarkersPlugin, asSeriesPrimitive, type WalletMarkerDef, type MarkerShape } from './walletMarkersPlugin';
import { BarTintPlugin, EMPTY_BAR_TINTS, asBarTintPrimitive } from './barTintPlugin';
import {
  EMPTY_LENS_LANE,
  LensLanePlugin,
  asLensLanePrimitive,
  lensLaneHeight,
} from './lensLanePlugin';
import { buildLensMatch, lensItemId, lensItemMatches, type LensMatch } from './lensTint';
import { HOST_RANGE_COLORS, RangeSelectPlugin, asRangePrimitive } from './rangeSelectPlugin';
import { rangeForSpan } from './barTrades';
import { barTimeAtClientX } from './paneCoords';
import {
  TimeBandsPlugin,
  asTimeBandsPrimitive,
  snapSpanToBars,
  type TimeBandLane,
} from './timeBandsPlugin';
import {
  applyFlowLineVisibility,
  flowLineVisibilityFromPrefs,
  type FlowLineVisibility,
} from './flowLineVisibility';
import type {
  ChartBarSelection,
  ChartCrosshairInfo,
  ChartGroupMode,
  ChartInterval,
  ChartLensItem,
  ChartLensMatches,
  ChartStyle,
  ChartBarTooltipState,
  ChartEventMarker,
  ChartRangeSelection,
  ChartRangeTooltipState,
  ChartWalletMarkersTooltipState,
  ChartLensLaneTooltipState,
  ChartTrade,
  OhlcBar,
  ProfileWalletInfo,
  TokenPriceChartProps,
  WalletBarActivity,
} from './types';

type ChartPrefs = typeof DEFAULT_CHART_PREFS;

const EMPTY_LENS_ITEMS: readonly ChartLensItem[] = [];

function loadPrefs(): ChartPrefs {
  try {
    const raw = getString(LS_CHART_PREFS_KEY);
    if (raw) {
      const parsed = JSON.parse(raw) as Partial<ChartPrefs> & { showFlowLines?: boolean };
      // Merge over defaults so a key added after this blob was written falls back
      // instead of coming through undefined. The flow overlay is the one exception:
      // a pre-split blob holds a single `showFlowLines`, so seed both per-curve
      // flags from it rather than resetting the user's saved state.
      const flow = flowLineVisibilityFromPrefs(parsed);
      return {
        ...DEFAULT_CHART_PREFS,
        ...parsed,
        showFlowTagged: flow.tagged,
        showFlowUntagged: flow.untagged,
      };
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

/** Silhouette per wallet CLASS: `mine` → arrow in the trade's direction (identity
 *  wins, permanent), dev/creator → triangle, else focused/input → hexagon, else
 *  lens-armed → diamond, else comparison-set → square, else circle. Focus still
 *  layers its gold ring on top of whatever shape, so a focused `mine` wallet stays
 *  an arrow and a focused dev stays a triangle — and a compared `mine`/dev wallet keeps its class shape while the
 *  comparison ring carries the tier.
 *
 *  Shape is deliberately redundant with size here rather than economical: size
 *  is the cue that survives a color-blind read, shape is the one that survives
 *  zooming out, and the comparison tier has to hold at both. */
function walletShape(w: ProfileWalletInfo, type: 'buy' | 'sell'): MarkerShape {
  if (w.isMine) return type === 'buy' ? 'arrowUp' : 'arrowDown';
  if (w.isDev) return 'triangle';
  if (w.isHighlighted) return 'hexagon';
  if (w.isLensed) return 'diamond';
  if (w.isCompared) return 'square';
  return 'circle';
}

/** Rank inside a bar's stack: focus, lens wallets, the comparison set, then everyone else.
 *  The stack packs outward from the bar edge, so the oversized tiers take the
 *  rows nearest the bar where nothing can crowd them and the dimmed crowd gets
 *  pushed out behind them. Order within a tier is unchanged. */
const walletTier = (w: ProfileWalletInfo): number =>
  w.isHighlighted ? 3 : w.isLensed ? 2 : w.isCompared ? 1 : 0;

function focusFirst(wallets: ProfileWalletInfo[]): ProfileWalletInfo[] {
  if (wallets.length < 2 || !wallets.some((w) => walletTier(w) > 0)) return wallets;
  return [...wallets].sort((a, b) => walletTier(b) - walletTier(a));
}

function walletGlyph(w: ProfileWalletInfo): string {
  if (w.isMine) return '★';
  if (w.isDev) return 'D';
  return (w.profileName ?? w.label ?? w.address).charAt(0).toUpperCase();
}

/** Below this fraction of the episode's peak balance, a sell is treated as a full
 *  exit (fee/rounding dust rarely leaves the balance at exactly zero). */
const SELL_ALL_DUST_FRACTION = 0.02;

/** The share of the chart's width a host default range opens at, centered. */
const DEFAULT_RANGE_VIEW_SHARE = 0.25;

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
    // The focused wallet takes the row nearest the bar so its oversized marker is
    // never buried behind the rest of the crowd.
    let buyStack = 0;
    for (const w of focusFirst(buy)) {
      defs.push({
        barTime,
        barEdgePrice: bar.low,
        letter: walletGlyph(w),
        color: w.color,
        borderColor: CHART_COLORS.buy,
        type: 'buy',
        stackIndex: buyStack++,
        shape: walletShape(w, 'buy'),
        role: roles.get(`${w.address}:${t}:buy`),
        highlighted: w.isHighlighted,
        ringColor: CHART_COLORS.highlightRing,
        lensed: w.isLensed,
        compared: w.isCompared,
        dimmed: w.dimmed,
      });
    }
    let sellStack = 0;
    for (const w of focusFirst(sell)) {
      defs.push({
        barTime,
        barEdgePrice: bar.high,
        letter: walletGlyph(w),
        color: w.color,
        borderColor: CHART_COLORS.sell,
        type: 'sell',
        stackIndex: sellStack++,
        shape: walletShape(w, 'sell'),
        role: roles.get(`${w.address}:${t}:sell`),
        highlighted: w.isHighlighted,
        ringColor: CHART_COLORS.highlightRing,
        lensed: w.isLensed,
        compared: w.isCompared,
        dimmed: w.dimmed,
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
  chrome = 'full',
  height: fixedHeight,
  onBarClick,
  selectedBar = null,
  onRangeChange,
  onCrosshairTimeChange,
  externalCrosshairTimeSec = null,
  onVisibleTimeRangeChange,
  timeBands = null,
  valueLane = null,
  timeBandCoverage = null,
  athPriceInSol = null,
  isMigrated,
  isMayhemMode,
  isCashbackEnabled,
  profileWallets,
  creatorWallet = null,
  tokenCreatedAt,
  eventMarkers = null,
  flowTag = null,
  flowBasis = 'cost_sol',
  highlightLens = null,
  onHighlightLensMatch,
  toolbarRow,
  defaultRange = null,
  hostRangeLabel = 'Range',
  onHostRangeChange,
}: TokenPriceChartProps) {
  // Tracked-wallet markers are a project-wide invariant: EVERY token trade chart
  // renders them. Callers may supply `profileWallets` (e.g. `TokenTradeChart`,
  // which augments the tracked set with the highlighted/synthetic input wallet);
  // when a caller omits the prop we fall back to the tracked profile wallets
  // here, so a token trade chart can never render without them — by construction,
  // not by convention.
  const trackedProfileWallets = useProfileWallets();
  const effectiveProfileWallets = profileWallets ?? trackedProfileWallets;

  // Compact chrome: Tools open/closed persists per chart id so a narrow host
  // (Console manual-trade) stays plot-first across reloads.
  const [toolsOpen, setToolsOpen] = useStoredField(
    STORAGE_KEYS.chartToolbarOpen,
    id || '_default',
    false,
  );

  // The token's dev/creator as a synthetic tracked wallet — folded into the same
  // marker pipeline so its first_buy/sell_all lifecycle + triangle silhouette
  // come for free. No parallel dev-marker path.
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
  const onHostRangeChangeRef = useRef(onHostRangeChange);
  onHostRangeChangeRef.current = onHostRangeChange;
  const onCrosshairTimeChangeRef = useRef(onCrosshairTimeChange);
  onCrosshairTimeChangeRef.current = onCrosshairTimeChange;
  const onVisibleTimeRangeChangeRef = useRef(onVisibleTimeRangeChange);
  onVisibleTimeRangeChangeRef.current = onVisibleTimeRangeChange;
  /** True while applying {@link externalCrosshairTimeSec} so the resulting
   *  subscribeCrosshairMove echo doesn't bounce back to the sibling. */
  const applyingExternalCrosshairRef = useRef(false);
  const prevExternalCrosshairRef = useRef<number | null>(null);
  const seriesRef = useRef<ISeriesApi<'Line' | 'Candlestick'> | null>(null);
  const taggedSeriesRef = useRef<ISeriesApi<'Line'> | null>(null);
  const untaggedSeriesRef = useRef<ISeriesApi<'Line'> | null>(null);
  const sortedTradesRef = useRef<ChartTrade[]>([]);
  const markersPluginRef = useRef<MarkersPlugin | null>(null);
  const walletMarkersPrimRef = useRef<WalletMarkersPlugin | null>(null);
  const rangeSelectPrimRef = useRef<RangeSelectPlugin | null>(null);
  const hostRangePrimRef = useRef<RangeSelectPlugin | null>(null);
  const timeBandsPrimRef = useRef<TimeBandsPlugin | null>(null);
  const barTintPrimRef = useRef<BarTintPlugin | null>(null);
  const lensLanePrimRef = useRef<LensLanePlugin | null>(null);
  const barsRef = useRef<OhlcBar[]>([]);
  const alignedFlowLinesRef = useRef<FlowLines>(EMPTY_FLOW_LINES);
  const valueLaneSeriesRef = useRef<ISeriesApi<'Line'> | null>(null);
  /** One price line per authored threshold — a band draws both of its edges. */
  const valueLaneLinesRef = useRef<IPriceLine[]>([]);

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
  const [flowLineVis, setFlowLineVis] = useState<FlowLineVisibility>({
    tagged: initialPrefs.showFlowTagged,
    untagged: initialPrefs.showFlowUntagged,
  });
  // A page-wide flow lens (Trader Analysis) adds its excluded wallets (the studied
  // trader). The tag itself - fingerprint tag, lens set or staging draft - is the
  // host's to pick and arrives as `flowTag`.
  const lens = useFlowLensContext();
  const flowExcludeWallets = lens?.excludeWallets ?? null;
  const classifyOpts = useMemo(
    () => classifyOptsForTag(flowTag, creatorWallet, flowExcludeWallets),
    [flowTag, creatorWallet, flowExcludeWallets],
  );
  /** The overlay draws exactly when the tag can classify (it uses a matcher). */
  const flowLinesAvailable = classifyOpts != null;
  const flowTagName = classifyOpts ? classifyOpts.tag.name : null;
  const flowTagText = useMemo(() => (flowTag ? tagSentence({ ...flowTag, id: '' }) : null), [flowTag]);
  const flowTagNameRef = useRef(flowTagName);
  flowTagNameRef.current = flowTagName;
  // A tag becoming readable is only feedback if the lines are on screen. The overlay
  // toggle is a persisted pref and the button is dead until something classifies,
  // so that first toggle would otherwise change nothing visible. Fires on the
  // transition only - turning the lines back off stays the user's call.
  const wasFlowLinesAvailable = useRef(flowLinesAvailable);
  useEffect(() => {
    const was = wasFlowLinesAvailable.current;
    wasFlowLinesAvailable.current = flowLinesAvailable;
    if (!was && flowLinesAvailable) setFlowLineVis({ tagged: true, untagged: true });
  }, [flowLinesAvailable]);
  const flowLinesAvailableRef = useRef(flowLinesAvailable);
  flowLinesAvailableRef.current = flowLinesAvailable;
  const { timezone: chartTimezone } = useTimezone();
  const [rangeSelectMode, setRangeSelectModeRaw] = useState(false);
  const [selectedRange, setSelectedRange] = useState<ChartRangeSelection | null>(null);
  // The host range (`toolbarRow`): its own band and drag mode, never the reader's.
  const [hostPickMode, setHostPickModeRaw] = useState(false);
  const [hostRange, setHostRange] = useState<ChartRangeSelection | null>(null);
  // The two drag modes are exclusive: one drag draws one band.
  const setRangeSelectMode = useCallback((on: boolean) => {
    setRangeSelectModeRaw(on);
    if (on) setHostPickModeRaw(false);
  }, []);
  const setHostPickMode = useCallback((on: boolean) => {
    setHostPickModeRaw(on);
    if (on) setRangeSelectModeRaw(false);
  }, []);
  const dragTarget: 'range' | 'host' | null = rangeSelectMode ? 'range' : hostPickMode ? 'host' : null;
  const [crosshair, setCrosshair] = useState<ChartCrosshairInfo | null>(null);
  const [barTooltip, setBarTooltip] = useState<ChartBarTooltipState | null>(null);
  const [rangeTooltip, setRangeTooltip] = useState<ChartRangeTooltipState | null>(null);
  const [walletMarkersTooltip, setWalletMarkersTooltip] = useState<ChartWalletMarkersTooltipState | null>(null);
  const [laneTooltip, setLaneTooltip] = useState<ChartLensLaneTooltipState | null>(null);
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
  /** Whether THIS chart instance has had the opening view applied; reset with
   *  every new chart, so a teardown before the first paint re-applies it. */
  const openingViewAppliedRef = useRef(false);
  const prevIdRef = useRef(id);
  const prevGroupingKeyRef = useRef(groupingKey);
  const visibleViewportRef = useRef<ChartViewport | null>(null);
  const isRestoringViewportRef = useRef(false);
  const mountedSeriesStyleRef = useRef<ChartStyle | null>(null);
  /** Shape of the bar array currently ON the chart — the baseline a saved logical
   *  range is translated from when the next `setData` shifts the indices. */
  const renderedBarsShapeRef = useRef<BarsShape | null>(null);
  const snapshotVisibleViewport = useCallback((chart: IChartApi): ChartViewport | null => {
    if (shouldFitContentRef.current) return null;
    const logical = chart.timeScale().getVisibleLogicalRange();
    if (!logical) return null;
    return captureChartViewport(logical, renderedBarsShapeRef.current);
  }, []);

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

  /** Wall-clock end of every bar — what a hovered candle resolves to for the
   *  condition strip and the condition-value pane. Includes empty slot bars, which
   *  is the whole reason it exists (see `buildBarWallEndSec`). */
  const barWallEndSec = useMemo(
    () => buildBarWallEndSec(bars, sortedTrades, groupMode, intervalSec),
    [bars, sortedTrades, groupMode, intervalSec],
  );
  const barWallEndSecRef = useRef(barWallEndSec);
  barWallEndSecRef.current = barWallEndSec;

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

  // ── Highlight lenses ────────────────────────────────────────────────────────
  //
  // "Where did this wallet trade / where did this structure appear", washed behind
  // the candles. Computed HERE and nowhere else: a hit is keyed to the bars this
  // chart draws, so it can only be honest in the one place that owns the bars.
  // `onHighlightLensMatch` hands the same numbers back out so
  // a host's chips can never quote a different count from the tint.
  // Wallets first, then structures, each in arming order — the lane's row order.
  const lensItems = useMemo(() => {
    const items = highlightLens?.items ?? EMPTY_LENS_ITEMS;
    return [
      ...items.filter((i) => i.kind === 'wallet'),
      ...items.filter((i) => i.kind !== 'wallet'),
    ];
  }, [highlightLens?.items]);
  const lensSizeLabels = highlightLens?.sizeLabels ?? 'buys';

  // One pass per armed item, keyed by `lensItemId` — the map the chips, the washes
  // and the lane hover all read.
  const lensMatches = useMemo<ChartLensMatches>(() => {
    const out = new Map<string, LensMatch>();
    for (const item of lensItems) {
      out.set(
        lensItemId(item),
        buildLensMatch(sortedTrades, bars, groupMode, intervalSec, metric, (t) =>
          lensItemMatches(item, t),
        ),
      );
    }
    return out;
  }, [lensItems, sortedTrades, bars, groupMode, intervalSec, metric]);

  // The lane's rows mirrored onto the range slider, in the lane's order.
  const sliderMarks = useMemo(
    () =>
      lensItems.map((item) => ({
        color: item.color,
        times: (lensMatches.get(lensItemId(item))?.tint ?? []).map((t) => t.barTime),
      })),
    [lensItems, lensMatches],
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
  const dragArmedRef = useRef(dragTarget != null);
  dragArmedRef.current = dragTarget != null;
  const hostStats = useMemo(
    () => (hostRange ? computeRangeStats(sortedTrades, hostRange, groupMode, intervalSec) : null),
    [hostRange, sortedTrades, groupMode, intervalSec],
  );

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
    laneTooltip: ChartLensLaneTooltipState | null;
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

  const handleFlowLinesChange = useCallback((next: FlowLineVisibility) => {
    setFlowLineVis(next);
    savePrefs({ showFlowTagged: next.tagged, showFlowUntagged: next.untagged });
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

  // `style`/`bars` are deps because a series rebuild hands us a FRESH plugin with
  // no tints — without them an armed lens silently blanks on a line/candle flip.
  useEffect(() => {
    if (!showChart) return;
    const rows = lensItems.map((item) => ({
      label: item.label,
      color: item.color,
      track: LENS_TRACK_COLORS[item.kind],
      tints: lensMatches.get(lensItemId(item))?.tint ?? [],
    }));
    const layers = rows.filter((r) => r.tints.length > 0);
    barTintPrimRef.current?.setTints(
      layers.length === 0 ? EMPTY_BAR_TINTS : { layers, laneRows: rows.length },
    );
    // Every armed item keeps its row, matched or not: an empty row says "never
    // appeared on this token", which a missing row cannot.
    lensLanePrimRef.current?.setState(
      rows.length === 0 ? EMPTY_LENS_LANE : { rows, sizeLabels: lensSizeLabels },
    );
    // The chart-rebuild keys too: a rebuilt chart hands over fresh plugins with no state.
  }, [lensItems, lensMatches, lensSizeLabels, showChart, style, bars, groupingKey, priceUnit, chartTimezone]);

  // The lane's strip is reserved under the candles through the price scale's
  // bottom margin, so no candle or flow line is ever drawn beneath a row. Read off
  // the pane's real height a frame later — a condition-value pane added in the
  // same flush takes its share first. The time bands move up over the lane.
  const laneRows = lensItems.length;
  const hasValueLane = !!valueLane && valueLane.points.length > 0;
  useEffect(() => {
    if (!showChart) return;
    const raf = requestAnimationFrame(() => {
      const chart = chartRef.current;
      if (!chart) return;
      const paneH = chart.panes()[0]?.getHeight() ?? 0;
      const laneH = paneH > 0 ? lensLaneHeight(laneRows, paneH) : 0;
      chart.priceScale('right').applyOptions({
        scaleMargins: {
          top: PRICE_SCALE_MARGINS.top,
          bottom: laneH > 0 ? PRICE_SCALE_MARGINS.bottom + laneH / paneH : PRICE_SCALE_MARGINS.bottom,
        },
      });
      timeBandsPrimRef.current?.setBottomInset((h) => lensLaneHeight(laneRows, h));
    });
    return () => cancelAnimationFrame(raf);
    // Every key the chart itself is rebuilt on, since a rebuilt chart starts from
    // the default margins.
  }, [
    laneRows,
    chartHeight,
    hasValueLane,
    showChart,
    style,
    fixedHeight,
    groupingKey,
    groupMode,
    priceUnit,
    chartTimezone,
  ]);

  const onHighlightLensMatchRef = useRef(onHighlightLensMatch);
  onHighlightLensMatchRef.current = onHighlightLensMatch;
  useEffect(() => {
    onHighlightLensMatchRef.current?.(lensMatches);
  }, [lensMatches]);


  useEffect(() => {
    if (prevIdRef.current !== id || prevGroupingKeyRef.current !== groupingKey) {
      shouldFitContentRef.current = true;
      openingViewAppliedRef.current = false;
      visibleViewportRef.current = null;
      renderedBarsShapeRef.current = null;
      mountedSeriesStyleRef.current = null;
      prevIdRef.current = id;
      prevGroupingKeyRef.current = groupingKey;
      setSliderWindow(null);
      // Range bounds are in the old grouping's units (slot vs bucket-sec) — drop them.
      setSelectedRange(null);
      setHostRange(null);
    }
  }, [id, groupingKey]);

  // A different token, bucketing, unit, metric or style is a different axis: the
  // previous hand-set Y zoom means nothing there, so the one price axis refits.
  // A new trade or a flow-line toggle is not - the library keeps a dragged zoom
  // until the axis is double-clicked.
  useEffect(() => {
    if (!showChart) return;
    chartRef.current?.priceScale('right').applyOptions({ autoScale: true });
  }, [id, groupingKey, priceUnit, metric, style, showChart]);

  // The host range's default, (re)set when the span, the token or the grouping
  // changes — after the reset above, which runs first in the same flush. Not on
  // new trades: a live print must not overwrite the host range's own drag.
  const defaultFrom = defaultRange?.from ?? null;
  const defaultTo = defaultRange?.to ?? null;
  const hasTrades = sortedTrades.length > 0;
  useEffect(() => {
    if (defaultFrom == null || defaultTo == null || !hasTrades) return;
    setHostRange(
      rangeForSpan(sortedTradesRef.current, { from: defaultFrom, to: defaultTo }, groupMode, intervalSec),
    );
  }, [defaultFrom, defaultTo, id, groupingKey, groupMode, intervalSec, hasTrades]);

  /** The opening view: a host default range focused (centered, a quarter of the
   *  width), otherwise the whole tape. Queued by the library until its next paint. */
  const applyOpeningView = (ts: ITimeScaleApi<Time>, bars: readonly OhlcBar[]) => {
    const focus =
      defaultFrom != null && defaultTo != null
        ? rangeForSpan(sortedTradesRef.current, { from: defaultFrom, to: defaultTo }, groupMode, intervalSec)
        : null;
    const view = focus ? focusLogicalRange(bars, focus.lo, focus.hi, DEFAULT_RANGE_VIEW_SHARE) : null;
    if (view) ts.setVisibleLogicalRange(view);
    else ts.fitContent();
    openingViewAppliedRef.current = true;
    visibleViewportRef.current = null;
  };

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
      createChartOptions(width, initialHeight, groupMode, priceUnit, chartTimezone),
    );
    chartRef.current = chart;
    openingViewAppliedRef.current = false;

    // The flow lines share the candles' price axis: each is drawn at its cohort
    // curve price (`cohortCurvePriceSol`), so its axis label reads a price in the
    // candles' unit and the legend carries the cohort's net.
    const taggedSeries = chart.addSeries(LineSeries, {
      color: FLOW_VOL_LINE_COLOR,
      lineWidth: 2,
      title: 'Vol makers',
      lastValueVisible: true,
      priceLineVisible: false,
      visible: false,
    });
    const untaggedSeries = chart.addSeries(LineSeries, {
      color: FLOW_NON_VOL_LINE_COLOR,
      lineWidth: 2,
      title: 'Non-tagged',
      lastValueVisible: true,
      priceLineVisible: false,
      visible: false,
    });
    taggedSeriesRef.current = taggedSeries;
    untaggedSeriesRef.current = untaggedSeries;

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
      setLaneTooltip(next.laneTooltip);
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
        laneTooltip: null as ChartLensLaneTooltipState | null,
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
      const setLaneTooltip = (v: ChartLensLaneTooltipState | null) => {
        next.laneTooltip = v;
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
        : { tagged: null, untagged: null };
      const info: ChartCrosshairInfo = {
        open: bar.open,
        high: bar.high,
        low: bar.low,
        close: bar.close,
        volume: bar.volume,
        inflow: bar.inflow,
        outflow: bar.outflow,
        liquiditySol: bar.liquiditySol,
        flowTagged: flow.tagged,
        flowUntagged: flow.untagged,
        flowTagName: flowTagNameRef.current,
      };
      setCrosshair(info);
      // Resolve wall-clock seconds for sibling panes (metric series): the instant
      // the hovered candle's coverage ENDS, in both grouping modes.
      //
      // This used to answer with the bar key in time mode (the bar's START — the
      // state before anything in the candle happened) and with a per-slot trade
      // lookup in slot mode, which returned `null` on every empty slot. A `null`
      // reads downstream as "pointer is off the plot", so the strip fell back to its
      // pinned readout and every gap bar showed the SAME borrowed numbers — which
      // reads exactly like a metric that never moved. `buildBarWallEndSec` has an
      // answer for every bar, so `null` now means only what it says.
      setCrosshairTimeSec(barWallEndSecRef.current.get(Number(param.time)) ?? null);
      // The highlight lane answers for its own strip: a mark lists only its row's
      // trades in this bar, and the candle's numbers stay off anywhere in the lane.
      const inPricePane = (param.paneIndex ?? 0) === 0;
      const lane = inPricePane && param.point ? lensLanePrimRef.current : null;
      if (lane && param.point && lane.containsY(param.point.y)) {
        const hit = lane.hitAt(param.point.y, bar.time as number);
        setLaneTooltip(hit ? { ...hit, point: param.point, barTime: bar.time } : null);
        setBarTooltip(null);
        setWalletMarkersTooltip(null);
        return;
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
      // While a drag-select is armed the pointer-drag handler owns clicks; don't
      // also toggle a bar selection.
      if (dragArmedRef.current) return;

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
      if (isRestoringViewportRef.current || logical == null) return;
      if (shouldFitContentRef.current) {
        // The opening view is queued until the chart paints, so it is done only
        // when the chart reports it. A report before the apply is the library's
        // default view; a chart torn down before its first paint (StrictMode's
        // double mount on a cached token) applies the opening view again.
        if (!openingViewAppliedRef.current) return;
        shouldFitContentRef.current = false;
      }
      if (!seriesRef.current) return;
      visibleViewportRef.current = captureChartViewport(logical, renderedBarsShapeRef.current);
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
      if (crosshairRafRef.current != null) {
        cancelAnimationFrame(crosshairRafRef.current);
        crosshairRafRef.current = null;
      }
      pendingCrosshairRef.current = null;
      ro.disconnect();
      markersPluginRef.current?.detach();
      markersPluginRef.current = null;
      walletMarkersPrimRef.current = null;
      barTintPrimRef.current = null;
      lensLanePrimRef.current = null;
      rangeSelectPrimRef.current = null;
      hostRangePrimRef.current = null;
      seriesRef.current = null;
      taggedSeriesRef.current = null;
      untaggedSeriesRef.current = null;
      chart.remove();
      chartRef.current = null;
      setCrosshair(null);
      setBarTooltip(null);
      onCrosshairTimeChangeRef.current?.(null);
      onVisibleTimeRangeChangeRef.current?.(null);
      setRangeTooltip(null);
      setWalletMarkersTooltip(null);
      setLaneTooltip(null);
    };
  }, [showChart, fixedHeight, groupingKey, groupMode, priceUnit, chartTimezone]);

  useEffect(() => {
    const series = seriesRef.current;
    if (!series || !showChart) return;
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
    const nextShape = barsShape(bars);

    const existing = seriesRef.current;
    const styleChanged = mountedSeriesStyleRef.current !== style;

    if (existing && !styleChanged) {
      const savedViewport = snapshotVisibleViewport(chart);
      if (savedViewport) visibleViewportRef.current = savedViewport;

      if (style === 'line') {
        existing.setData(barsToLineData(bars));
      } else {
        existing.setData(barsToCandleData(bars, highlightBarTimes));
      }
      renderedBarsShapeRef.current = nextShape;

      if (shouldFitContentRef.current) {
        applyOpeningView(ts, bars);
      } else if (savedViewport) {
        isRestoringViewportRef.current = true;
        reapplyChartViewport(ts, savedViewport, bars);
        requestAnimationFrame(() => {
          isRestoringViewportRef.current = false;
        });
      }
      return;
    }

    const savedViewport =
      existing != null
        ? snapshotVisibleViewport(chart)
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
      if (hostRangePrimRef.current) {
        existing.detachPrimitive(asRangePrimitive(hostRangePrimRef.current));
        hostRangePrimRef.current = null;
      }
      if (timeBandsPrimRef.current) {
        existing.detachPrimitive(asTimeBandsPrimitive(timeBandsPrimRef.current));
        timeBandsPrimRef.current = null;
      }
      if (barTintPrimRef.current) {
        existing.detachPrimitive(asBarTintPrimitive(barTintPrimRef.current));
        barTintPrimRef.current = null;
      }
      if (lensLanePrimRef.current) {
        existing.detachPrimitive(asLensLanePrimitive(lensLanePrimRef.current));
        lensLanePrimRef.current = null;
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

    const tintPrim = new BarTintPlugin();
    series.attachPrimitive(asBarTintPrimitive(tintPrim));
    barTintPrimRef.current = tintPrim;

    const walletPrim = new WalletMarkersPlugin();
    series.attachPrimitive(asSeriesPrimitive(walletPrim));
    walletMarkersPrimRef.current = walletPrim;

    const rangePrim = new RangeSelectPlugin();
    series.attachPrimitive(asRangePrimitive(rangePrim));
    rangeSelectPrimRef.current = rangePrim;

    const hostPrim = new RangeSelectPlugin(HOST_RANGE_COLORS);
    series.attachPrimitive(asRangePrimitive(hostPrim));
    hostRangePrimRef.current = hostPrim;

    const bandsPrim = new TimeBandsPlugin();
    series.attachPrimitive(asTimeBandsPrimitive(bandsPrim));
    timeBandsPrimRef.current = bandsPrim;

    const lanePrim = new LensLanePlugin();
    series.attachPrimitive(asLensLanePrimitive(lanePrim));
    lensLanePrimRef.current = lanePrim;

    if (style === 'line') {
      series.setData(barsToLineData(bars));
    } else {
      series.setData(barsToCandleData(bars, highlightBarTimes));
    }

    renderedBarsShapeRef.current = nextShape;

    if (shouldFitContentRef.current) {
      applyOpeningView(ts, bars);
    } else if (savedViewport) {
      isRestoringViewportRef.current = true;
      reapplyChartViewport(ts, savedViewport, bars);
      requestAnimationFrame(() => {
        isRestoringViewportRef.current = false;
      });
    }
  }, [bars, style, showChart, groupingKey, priceUnit, highlightBarKey, snapshotVisibleViewport]);

  // `@tag` / `@!tag` cumulative overlay, on the candles' own price axis. No tag
  // that classifies ⇒ no lines, and the toolbar toggle says why.
  const flowLines = useMemo(() => {
    if (!classifyOpts) return EMPTY_FLOW_LINES;
    return buildFlowLines(sortedTrades, groupMode, intervalSec, flowBasis as FlowBasis, classifyOpts);
  }, [sortedTrades, groupMode, intervalSec, flowBasis, classifyOpts]);
  const alignedFlowLines = useMemo(() => alignFlowToBars(flowLines, bars), [flowLines, bars]);
  alignedFlowLinesRef.current = alignedFlowLines;

  useEffect(() => {
    if (!showChart) return;
    // Each point is drawn at its cohort curve price, converted exactly as the bars
    // are - so net 0 sits on the first candle's open and one zoom moves every
    // series. A point with no curve price (a token reserve driven past 0) is a gap.
    const toData = (pts: readonly FlowLinePoint[]) =>
      pts.map((p) => {
        const value = priceSolChartValue(p.priceSol, metric, toValue);
        return Number.isFinite(value) ? { time: p.time, value } : { time: p.time };
      });
    const priceFormat = createChartPriceFormat(priceUnit);
    taggedSeriesRef.current?.applyOptions({ priceFormat });
    untaggedSeriesRef.current?.applyOptions({ priceFormat });
    applyFlowLineVisibility({
      taggedSeries: taggedSeriesRef.current,
      untaggedSeries: untaggedSeriesRef.current,
      visibility: flowLineVis,
      available: flowLinesAvailable,
    });
    taggedSeriesRef.current?.setData(toData(alignedFlowLines.tagged));
    untaggedSeriesRef.current?.setData(toData(alignedFlowLines.untagged));
  }, [
    alignedFlowLines,
    metric,
    priceUnit,
    toValue,
    flowLineVis,
    flowLinesAvailable,
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

    // ONE toggle covers the whole entry/exit overlay — arrows here and the dashed
    // fill-price lines below. Gating only the lines left the arrows unturnoffable,
    // which on a scale-out ladder is the densest layer on the chart.
    if (showEventMarkers && eventMarkers && eventMarkers.length > 0) {
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
    showEventMarkers,
  ]);

  // ── Condition-value pane ────────────────────────────────────────────────────
  //
  // A SEPARATE pane, not another overlay on the price scale: this is the quantity a
  // decision was actually taken on, and it must be readable against its own threshold
  // rather than squeezed onto an axis it shares nothing with.
  //
  // Points arrive in wall-clock seconds and are snapped onto whatever bars this chart
  // is drawing — one point per bar, the last reading at or before that bar's end
  // (`barWallEndSec`), which is the same instant the crosshair reports. That keeps the
  // line, the hovered chips and the engine's own decision describing one moment.
  useEffect(() => {
    if (!showChart) return;
    const chart = chartRef.current;
    if (!chart) return;

    if (!valueLane || valueLane.points.length === 0 || bars.length === 0) {
      if (valueLaneSeriesRef.current) {
        chart.removeSeries(valueLaneSeriesRef.current);
        valueLaneSeriesRef.current = null;
        valueLaneLinesRef.current = [];
      }
      return;
    }

    let series = valueLaneSeriesRef.current;
    if (!series) {
      // Pane 1 — created on demand so a chart with no lane keeps its full height.
      series = chart.addSeries(
        LineSeries,
        { lineWidth: 2, priceLineVisible: false, lastValueVisible: true },
        1,
      );
      valueLaneSeriesRef.current = series;
    }
    series.applyOptions({ color: valueLane.color, title: valueLane.label });

    // Walk bars and points together — both ascending, so this stays linear.
    const points = valueLane.points;
    // Coverage ends with the last recorded point. Past it there is no reading, and
    // carrying the last one forward would draw a flat line across the rest of the
    // chart that the reconstruction never observed — the same "metric frozen at its
    // final value" misreading the condition chips refuse to show.
    const lastSec = points[points.length - 1].timeSec;
    const data: { time: UTCTimestamp; value: number }[] = [];
    let p = 0;
    let carried: number | null = null;
    for (const bar of bars) {
      const end = barWallEndSec.get(bar.time as number);
      if (end == null) continue;
      while (p < points.length && points[p].timeSec <= end) {
        carried = points[p].value;
        p += 1;
      }
      // A `null` reading is unreadable (NaN in the engine), which satisfies nothing —
      // so the line breaks rather than drawing a flat zero that looks like a value.
      if (carried != null && Number.isFinite(carried)) {
        data.push({ time: bar.time, value: carried });
      }
      // This bar is the one holding the last point, so it is the last bar with a
      // reading — stop rather than repeating it rightward.
      if (end >= lastSec) break;
    }
    series.setData(data);

    for (const line of valueLaneLinesRef.current) series.removePriceLine(line);
    // One line per threshold, so a BAND (`> 20, < 50`) draws both of its edges — a
    // single line cannot say where a two-sided condition stops holding.
    valueLaneLinesRef.current = (valueLane.thresholds ?? [])
      .filter((t) => Number.isFinite(t))
      .map((t) =>
        series.createPriceLine({
          price: t,
          color: CHART_COLORS.text,
          lineWidth: 1,
          lineStyle: LineStyle.Dashed,
          axisLabelVisible: true,
          title: 'threshold',
        }),
      );
  }, [valueLane, bars, barWallEndSec, showChart, style, groupingKey]);

  // Bottom-pane on/off lanes. Snapping happens HERE rather than in the primitive
  // because `timeToCoordinate` resolves only times the scale already knows, and
  // `bars` — the thing that decides what it knows — lives in this component.
  useEffect(() => {
    const prim = timeBandsPrimRef.current;
    if (!prim || !showChart) return;
    if (!timeBands?.length || bars.length === 0) {
      prim.setLanes([], null);
      return;
    }
    // Snap through each bar's WALL-CLOCK end rather than its key, so slot mode works
    // too: a slot number is not a clock, but every bar now has an instant
    // (`barWallEndSec`, empty slots included). Slot mode used to drop the lanes
    // entirely — silently, and exactly in the view a launch is inspected in.
    const keys = bars.map((b) => Number(b.time));
    const walls = keys.map((k) => barWallEndSec.get(k));
    const usable = walls.every((w) => w != null);
    if (!usable) {
      prim.setLanes([], null);
      return;
    }
    const times = walls as number[];
    // Built once rather than scanned per span end: the snap answers in wall seconds
    // and the scale keys on bar keys, and a lane can hold as many spans as the token
    // has crossings.
    const barKeyByWall = new Map<number, number>();
    times.forEach((wall, i) => barKeyByWall.set(wall, keys[i]));
    const toBarKey = (wall: number) => barKeyByWall.get(wall) ?? wall;
    const lanes: TimeBandLane[] = timeBands.map((lane) => ({
      key: lane.key,
      label: lane.label,
      color: lane.color,
      spans: lane.spans.flatMap((s) => {
        const snapped = snapSpanToBars(times, s.from, s.to);
        return snapped
          ? [
              {
                from: toBarKey(snapped.from) as UTCTimestamp,
                to: toBarKey(snapped.to) as UTCTimestamp,
              },
            ]
          : [];
      }),
    }));
    const track = timeBandCoverage
      ? snapSpanToBars(times, timeBandCoverage.from, timeBandCoverage.to)
      : null;
    prim.setLanes(
      lanes,
      track
        ? {
            from: toBarKey(track.from) as UTCTimestamp,
            to: toBarKey(track.to) as UTCTimestamp,
          }
        : null,
    );
  }, [timeBands, timeBandCoverage, bars, barWallEndSec, showChart, style, groupingKey]);

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

  // The host range's band: indigo, its chip `<label> · <length>`. Same keys as the
  // range band above, for the same reasons.
  useEffect(() => {
    const prim = hostRangePrimRef.current;
    if (!prim || !showChart) return;
    prim.setBand(
      hostRange
        ? {
            loTime: Math.min(hostRange.lo, hostRange.hi) as UTCTimestamp,
            hiTime: Math.max(hostRange.lo, hostRange.hi) as UTCTimestamp,
            label: hostStats ? `${hostRangeLabel} · ${formatRangeDuration(hostStats.durationMs)}` : hostRangeLabel,
            dashed: false,
          }
        : null,
    );
  }, [hostRange, hostStats, hostRangeLabel, hostPickMode, showChart, style, groupingKey]);

  useEffect(() => {
    onHostRangeChangeRef.current?.(
      hostRange ? { lo: hostRange.lo, hi: hostRange.hi, groupMode, intervalSec } : null,
    );
  }, [hostRange, groupMode, intervalSec]);

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

  // Drag-to-select a time range, into the reader's range (range-select mode) or
  // the host range (host pick mode): disable the chart's pan/zoom so a horizontal
  // drag draws a band instead of scrolling, and snap both edges to the nearest
  // bar via the logical coordinate.
  useEffect(() => {
    if (!showChart || !dragTarget) return;
    const el = containerRef.current;
    const chart = chartRef.current;
    if (!el || !chart) return;
    const isHost = dragTarget === 'host';
    const primRef = isHost ? hostRangePrimRef : rangeSelectPrimRef;
    const commit = isHost ? setHostRange : setSelectedRange;

    chart.applyOptions({ handleScroll: false, handleScale: false });
    el.style.cursor = 'crosshair';

    const coordToBarTime = (clientX: number): number | null =>
      barTimeAtClientX(chart, el, barsRef.current, clientX);

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
      primRef.current?.setBand({
        loTime: t as UTCTimestamp,
        hiTime: t as UTCTimestamp,
        dashed: true,
      });
    };

    const onPointerMove = (e: PointerEvent) => {
      if (!dragging || startTime == null) return;
      const t = coordToBarTime(e.clientX);
      if (t == null) return;
      primRef.current?.setBand({
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
      // the draft band directly too: if no selection existed, the commit is a
      // no-op and the band effect won't fire to clear the pointerdown dot.
      if (startTime == null || t == null || Math.abs(e.clientX - startX) < 4) {
        startTime = null;
        primRef.current?.setBand(null);
        commit(null);
        return;
      }
      commit({ lo: Math.min(startTime, t), hi: Math.max(startTime, t) });
      startTime = null;
    };

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') commit(null);
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
          handleScale: { ...CHART_HANDLE_SCALE },
        });
      }
    };
  }, [showChart, fixedHeight, groupingKey, groupMode, priceUnit, chartTimezone, dragTarget]);

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
          title: m.lineLabel ?? (isEntry ? 'Entry' : 'Exit'),
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
        chrome={chrome}
        toolsOpen={toolsOpen}
        onToolsOpenChange={setToolsOpen}
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
        flowLines={flowLineVis}
        flowLinesAvailable={flowLinesAvailable}
        flowTagName={flowTagName}
        flowTagText={flowTagText}
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
        onFlowLinesChange={handleFlowLinesChange}
        onRangeSelectModeChange={setRangeSelectMode}
      />
      {toolbarRow && (
        <div
          className="flex flex-wrap items-center gap-2 border-b px-3 py-1.5"
          style={{ borderColor: CHART_COLORS.border }}
        >
          {toolbarRow({
            picking: hostPickMode,
            setPicking: setHostPickMode,
            clear: () => setHostRange(null),
            selectSpan: (span) =>
              setHostRange(rangeForSpan(sortedTradesRef.current, span, groupMode, intervalSec)),
          })}
        </div>
      )}
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
        {laneTooltip && (
          <LensLaneTooltip
            tooltip={laneTooltip}
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
          marks={sliderMarks}
        />
      )}
    </div>
  );
}
