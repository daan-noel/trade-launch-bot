import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
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
  type UTCTimestamp,
} from 'lightweight-charts';
import { usePriceUnit } from 'context/PriceUnitContext';
import {
  aggregateTradesToBars,
  aggregateTradesToBarsBySlot,
  athChartValue,
  barAgeSec,
  barSelectionMarker,
  barsToCandleData,
  barsToLineData,
  buildBarEarliestTradeSec,
  compareTradesChronologically,
  computeRangeStats,
  dropEmptyBars,
  migrationChartValue,
  tokenCreatedAtSec,
  tradeBarSlot,
  tradeBarTime,
} from 'components/token-price-chart/chartBars';
import { BarCrosshairFields } from 'components/token-price-chart/BarCrosshairFields';
import { BarCrosshairTooltip } from 'components/token-price-chart/BarCrosshairTooltip';
import { createChartTimeFormatters } from 'components/token-price-chart/chartTimezone';
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
  DUAL_CHART_HANDLE_SCALE,
  createChartPriceFormat,
  createChartPriceFormatter,
  LINE_SERIES_OPTIONS,
  responsiveChartHeight,
} from 'components/token-price-chart/constants';
import { createChartOptions } from 'components/token-price-chart/chartOptions';
import type {
  ChartBarSelection,
  ChartBarTooltipState,
  ChartCrosshairInfo,
  ChartGroupMode,
  ChartInterval,
  ChartRangeSelection,
  ChartRangeStats,
  ChartRangeTooltipState,
  ChartStyle,
  ChartWalletMarkersTooltipState,
  OhlcBar,
  ProfileWalletInfo,
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
import type { ColumnDef } from 'components/table/types';
import { tokenTradeColumns } from 'components/tokens/tokenTradeColumns';
import { Badge } from 'components/ui/Badge';
import { Checkbox } from 'components/ui/Checkbox';
import { cn } from 'lib/cn';
import { useTimezone } from 'context/TimezoneContext';
import { useProfileWallets } from 'hooks/useProfileWallets';
import { formatTimestampMs } from 'utils/date';
import { classifyFlowTrades } from 'lib/flow/classifyFlow';
import {
  alignFlowToBars,
  buildFlowLines,
  flowAt,
  formatFlowTokenCount,
  FLOW_NON_VOL_LINE_COLOR,
  FLOW_VOL_LINE_COLOR,
  type FlowBasis,
  type FlowLinePoint,
} from 'lib/flow/flowChartData';
import {
  attachDualPriceScaleSync,
  type DualPriceScaleSync,
} from 'components/token-price-chart/dualPriceScaleSync';
import { getString, setString, STORAGE_KEYS } from 'lib/storage';
import type { TradeRecord } from 'types';

const flowTradeRowKey = (t: TradeRecord) => t.id;

interface FlowPreviewChartPrefs {
  style: ChartStyle;
  groupMode: ChartGroupMode;
  interval: ChartInterval;
  basis: FlowBasis;
  trimEmptyBars: boolean;
  seedCreatorAsVol: boolean;
  showTradeMarkers: boolean;
  showWalletMarkers: boolean;
  showDevMarkers: boolean;
  devMarkersBoundariesOnly: boolean;
  showAthLine: boolean;
  showMigrationLine: boolean;
  highlightVolumeBars: boolean;
  showFlowLines: boolean;
}

const DEFAULT_FLOW_CHART_PREFS: FlowPreviewChartPrefs = {
  style: 'candles',
  groupMode: 'time',
  interval: '1s',
  basis: 'cost_sol',
  trimEmptyBars: true,
  // Live ALWAYS classifies the creator wallet as volume (flow_split.rs
  // FlowState::classify), so default ON to mirror live. Turn OFF only to
  // isolate what the checked ix-patterns alone would catch.
  seedCreatorAsVol: true,
  showTradeMarkers: false,
  showWalletMarkers: false,
  showDevMarkers: false,
  // Default OFF: show every dev trade. ON keeps only the first_buy/sell_all
  // boundaries when the dev's manufactured-volume trades clutter the chart.
  devMarkersBoundariesOnly: false,
  showAthLine: true,
  showMigrationLine: true,
  // Default ON: this chart exists to separate manufactured volume from organic
  // flow, and the bar highlight is what makes that split visible at a glance.
  highlightVolumeBars: true,
  showFlowLines: true,
};

/** Toolbar toggles persist across sessions (mirrors `TokenPriceChart`'s
 *  `loadPrefs`/`savePrefs`); transient view state (range-select mode, the
 *  "More" panel, bar/range selection) does not. */
function loadFlowChartPrefs(): FlowPreviewChartPrefs {
  try {
    const raw = getString(STORAGE_KEYS.flowPreviewChartPrefs);
    if (raw) {
      const parsed = JSON.parse(raw) as Partial<FlowPreviewChartPrefs>;
      const merged = { ...DEFAULT_FLOW_CHART_PREFS, ...parsed };
      // Guard the basis against a stale persisted value — earlier builds stored
      // 'sol' (two-way toggle) and later 'net_sol'/'real_sol', none of which
      // exist now (basis is cost_sol/token/value_sol). Fall back to the default.
      if (!BASIS_OPTIONS.some((o) => o.value === merged.basis)) {
        merged.basis = DEFAULT_FLOW_CHART_PREFS.basis;
      }
      return merged;
    }
  } catch {
    /* ignore */
  }
  return DEFAULT_FLOW_CHART_PREFS;
}

function saveFlowChartPrefs(prefs: FlowPreviewChartPrefs) {
  setString(STORAGE_KEYS.flowPreviewChartPrefs, JSON.stringify(prefs));
}

/** Trades within the clicked bar — same bucket-key matching `TokenTradeChart`
 *  uses for its own bar-click selection. */
function tradesInBar(trades: TradeRecord[], bar: ChartBarSelection): TradeRecord[] {
  if (bar.groupMode === 'slot') {
    return trades.filter((t) => t.slot === bar.slot);
  }
  const intervalSec = bar.intervalSec ?? 60;
  return trades.filter((t) => tradeBarTime(t.block_time, intervalSec) === bar.barTime);
}

/** Vol/non-vol overlay line colors, by what each side represents:
 *  - VOL is dev-generated volume → red (`#EF5350`).
 *  - NON_VOL is real trader volume → gold (`#F5C542`).
 *  Both are high-luminance so they read at a glance over the candle field. */
const VOL_LINE_COLOR = FLOW_VOL_LINE_COLOR;
const NON_VOL_COLOR = FLOW_NON_VOL_LINE_COLOR;

/** Pure-volume (dev-generated) candles are DE-EMPHASISED rather than
 *  highlighted: their body + border + wick are painted a dark grey just a hair
 *  above the chart background (`CHART_COLORS.background` = #1a1a1a) — close
 *  enough to nearly recede, different enough to still read as a bar — leaving
 *  the colored organic (real-trader) candles to stand out. Applied only when
 *  EVERY trade in the bar is volume. Same color tints the toolbar toggle + the
 *  line-mode marker so they all read as one thing. */
const VOL_GHOST_COLOR = '#35363d';

/** lightweight-charts rejects any line value outside ±9.007e13. The token basis
 *  is a cumulative whole-token count that runs to 1e14+ for a normal pump.fun
 *  token (≈1e15 raw supply, traded over repeatedly), so its two flow lines are
 *  charted divided by this scale (multiplied back for every axis/readout label);
 *  the two SOL bases (cost_sol / value_sol) are orders of magnitude smaller and
 *  chart 1:1. */
const TOKEN_FLOW_SERIES_SCALE = 1e6;

function flowSeriesScale(basis: FlowBasis): number {
  return basis === 'token' ? TOKEN_FLOW_SERIES_SCALE : 1;
}

/** Three bars with the middle one lit — "highlight the flow-split odd-ones-out". */
function VolumeBarsIcon() {
  return (
    <svg viewBox="0 0 20 20" fill="none" aria-hidden className="size-3.5">
      <rect x="3" y="8" width="3" height="8" rx="0.5" fill="currentColor" opacity="0.4" />
      <rect x="8.5" y="4" width="3" height="12" rx="0.5" fill="currentColor" />
      <rect x="14" y="9.5" width="3" height="6.5" rx="0.5" fill="currentColor" opacity="0.4" />
    </svg>
  );
}

/** Two overlaid cumulative curves — the vol/non-vol flow-line pair. */
function FlowLinesIcon() {
  return (
    <svg viewBox="0 0 20 20" fill="none" aria-hidden className="size-3.5">
      <path d="M3 15 C7 14 9 8 17 5" stroke={VOL_LINE_COLOR} strokeWidth="1.6" strokeLinecap="round" />
      <path d="M3 16.5 C8 16 11 12 17 11" stroke={NON_VOL_COLOR} strokeWidth="1.6" strokeLinecap="round" />
    </svg>
  );
}

/** Upward triangle with a "D" — the dev/creator marker silhouette. */
function DevMarkersIcon() {
  return (
    <svg viewBox="0 0 20 20" fill="none" aria-hidden className="size-3.5">
      <path d="M10 3.5 L17 16 L3 16 Z" fill="currentColor" opacity="0.25" stroke="currentColor" strokeWidth="1.4" strokeLinejoin="round" />
      <text x="10" y="14.5" textAnchor="middle" fontSize="7" fontWeight="700" fill="currentColor">D</text>
    </svg>
  );
}

/** Ghost the pure-volume candles into the background so the organic ones stand
 *  out — body + border + wick all get the shallow translucent dark, sacrificing
 *  this bar's up/down color to read it as "dev-generated volume, not a real
 *  move". A bar is ghosted only if EVERY trade in it is volume
 *  (`pureVolumeBarTimes`); bars with any organic trade keep their normal color. */
function outlineVolumeCandles(
  data: ReturnType<typeof barsToCandleData>,
  pureVolumeBarTimes: ReadonlySet<number>,
) {
  return data.map((c) => {
    if (!pureVolumeBarTimes.has(c.time as number)) return c;
    return {
      ...c,
      color: VOL_GHOST_COLOR,
      wickColor: VOL_GHOST_COLOR,
      borderColor: VOL_GHOST_COLOR,
    };
  });
}

const BASIS_OPTIONS: { value: FlowBasis; label: string }[] = [
  { value: 'cost_sol', label: 'Cost SOL' },
  { value: 'value_sol', label: 'Value SOL' },
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

export interface FlowPreviewChartProps {
  trades: TradeRecord[];
  /** `JSON.stringify(labels)` keys of the checked volume_ix_patterns rows —
   *  redraws the two overlay lines whenever this set changes. */
  patternKeys: ReadonlySet<string>;
  /** Toggle a trade's ix-structure in/out of the draft volume_ix_patterns —
   *  wired to the same `draftPatterns` the ranked structure table mutates, so
   *  the Bar-Trades table can flag a shape in-context. Omit to hide the Vol
   *  checkbox (ix_labels stays read-only). */
  onTogglePattern?: (labels: string[]) => void;
  /** Token creator wallet address, when known — offered as a toggle since the
   *  real classifier always treats the creator as volume-side. */
  creatorWallet?: string | null;
  /** ATH price (SOL) for the ATH reference line — same field the shared
   *  token detail chart uses. */
  athPriceInSol?: number | null;
  /** Drives the "Migration" reference line the same way the shared chart's
   *  toggle does (the line itself is a fixed pump.fun constant either way). */
  isMigrated?: boolean;
  /** Token `created_at` (ISO) — the zero point for the crosshair tooltip's
   *  "+age since launch". Omit and the age line is simply absent. */
  tokenCreatedAt?: string | null;
  /** Fixed pixel height. Omit (the default) to let the chart size its height
   *  to its width via {@link responsiveChartHeight} — the chart width fills
   *  the column, so a fixed height renders wide-and-flat on a big monitor. */
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
  onTogglePattern,
  creatorWallet,
  athPriceInSol = null,
  isMigrated = false,
  tokenCreatedAt = null,
  height: fixedHeight,
}: FlowPreviewChartProps) {
  const { timezone } = useTimezone();
  const { unit: priceUnit, usdRate } = usePriceUnit();
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
  const scaleSyncRef = useRef<DualPriceScaleSync | null>(null);
  /** Non-data inputs that change what the price axes MEAN — only these justify
   *  dropping a hand-set Y zoom (see dualPriceScaleSync). */
  const flowScaleResetKeyRef = useRef<string | null>(null);

  // Height tracks width unless the caller pins it (`fixedHeight`). Kept in a ref
  // too so the resize observer can compare without re-running the create effect.
  const [chartHeight, setChartHeight] = useState(() => fixedHeight ?? responsiveChartHeight(0));
  const chartHeightRef = useRef(chartHeight);
  useEffect(() => {
    chartHeightRef.current = chartHeight;
  }, [chartHeight]);

  const initialPrefs = useRef(loadFlowChartPrefs()).current;
  const [style, setStyle] = useState<ChartStyle>(initialPrefs.style);
  const [groupMode, setGroupMode] = useState<ChartGroupMode>(initialPrefs.groupMode);
  const [interval, setInterval] = useState<ChartInterval>(initialPrefs.interval);
  const [basis, setBasis] = useState<FlowBasis>(initialPrefs.basis);
  const [trimEmptyBars, setTrimEmptyBars] = useState(initialPrefs.trimEmptyBars);
  const [seedCreatorAsVol, setSeedCreatorAsVol] = useState(initialPrefs.seedCreatorAsVol);
  const [showTradeMarkers, setShowTradeMarkers] = useState(initialPrefs.showTradeMarkers);
  const [showWalletMarkers, setShowWalletMarkers] = useState(initialPrefs.showWalletMarkers);
  const [showDevMarkers, setShowDevMarkers] = useState(initialPrefs.showDevMarkers);
  const [devMarkersBoundariesOnly, setDevMarkersBoundariesOnly] = useState(
    initialPrefs.devMarkersBoundariesOnly,
  );
  const [showAthLine, setShowAthLine] = useState(initialPrefs.showAthLine);
  const [showMigrationLine, setShowMigrationLine] = useState(initialPrefs.showMigrationLine);
  const [highlightVolumeBars, setHighlightVolumeBars] = useState(initialPrefs.highlightVolumeBars);
  const [showFlowLines, setShowFlowLines] = useState(initialPrefs.showFlowLines);
  const [rangeSelectMode, setRangeSelectMode] = useState(false);
  const [showMore, setShowMore] = useState(false);
  const [selectedRange, setSelectedRange] = useState<ChartRangeSelection | null>(null);
  const [selectedBar, setSelectedBar] = useState<ChartBarSelection | null>(null);

  const [crosshair, setCrosshair] = useState<ChartCrosshairInfo | null>(null);
  const [flowCrosshair, setFlowCrosshair] = useState<{ vol: number | null; nonVol: number | null } | null>(
    null,
  );
  const [barTooltip, setBarTooltip] = useState<ChartBarTooltipState | null>(null);
  const [chartWidth, setChartWidth] = useState(0);
  const [rangeTooltip, setRangeTooltip] = useState<ChartRangeTooltipState | null>(null);
  const [walletTooltip, setWalletTooltip] = useState<ChartWalletMarkersTooltipState | null>(null);

  const formatPrice = useMemo(() => createChartPriceFormatter(priceUnit), [priceUnit]);
  // Vol / Liq / Net / In / Out are raw SOL amounts — `aggregateTradesToBars`
  // applies `toValue` to PRICES only — so they never follow the SOL/USD toggle
  // (same as the shared token chart, which pins this formatter to 'SOL').
  const formatVol = useMemo(() => createChartPriceFormatter('SOL'), []);
  // Same SOL→display conversion TokenTradeChart uses so the header SOL/USD
  // toggle moves candles and SOL-basis flow lines together.
  const toValue = useCallback(
    (sol: number) => (priceUnit === 'USD' && usdRate != null ? sol * usdRate : sol),
    [priceUnit, usdRate],
  );
  const solUnitScale = priceUnit === 'USD' && usdRate != null ? usdRate : 1;

  const intervalSec = CHART_INTERVALS[interval];
  const intervalsDisabled = groupMode === 'slot';

  const sortedTrades = useMemo(() => [...trades].sort(compareTradesChronologically), [trades]);

  const bars = useMemo(() => {
    const raw =
      groupMode === 'slot'
        ? aggregateTradesToBarsBySlot(sortedTrades, toValue, 'price')
        : aggregateTradesToBars(sortedTrades, intervalSec, toValue, 'price');
    return trimEmptyBars ? dropEmptyBars(raw) : raw;
  }, [sortedTrades, groupMode, intervalSec, trimEmptyBars, toValue]);

  const classifyOpts = useMemo(
    () => ({ patternKeys, creatorWallet: seedCreatorAsVol ? creatorWallet : null }),
    [patternKeys, creatorWallet, seedCreatorAsVol],
  );

  const lines = useMemo(
    () => buildFlowLines(trades, groupMode, intervalSec, basis, classifyOpts),
    [trades, groupMode, intervalSec, basis, classifyOpts],
  );

  // 1:1 with candle bar times so crosshair / X-zoom always hit both series.
  const alignedLines = useMemo(() => alignFlowToBars(lines, bars), [lines, bars]);

  // Per-bar "+age since launch" for the crosshair tooltip — same shared resolver
  // the token detail chart uses (SSOT), read through a ref because the crosshair
  // handler is installed once at mount.
  const createdAtSec = useMemo(() => tokenCreatedAtSec(tokenCreatedAt), [tokenCreatedAt]);
  const barEarliestTradeSec = useMemo(
    () => buildBarEarliestTradeSec(sortedTrades, groupMode, intervalSec),
    [sortedTrades, groupMode, intervalSec],
  );
  const computeBarAgeSec = useCallback(
    (barTime: number) => barAgeSec(barTime, createdAtSec, barEarliestTradeSec, groupMode),
    [createdAtSec, barEarliestTradeSec, groupMode],
  );
  const computeBarAgeSecRef = useRef(computeBarAgeSec);
  computeBarAgeSecRef.current = computeBarAgeSec;

  const formatBarTime = useCallback(
    (barTime: UTCTimestamp) =>
      groupMode === 'slot'
        ? `Slot ${barTime}`
        : createChartTimeFormatters(timezone).timeFormatter(barTime),
    [groupMode, timezone],
  );

  const rangeStats: ChartRangeStats | null = useMemo(() => {
    if (!selectedRange) return null;
    return computeRangeStats(sortedTrades, selectedRange, groupMode, intervalSec);
  }, [selectedRange, sortedTrades, groupMode, intervalSec]);

  useEffect(() => {
    saveFlowChartPrefs({
      style,
      groupMode,
      interval,
      basis,
      trimEmptyBars,
      seedCreatorAsVol,
      showTradeMarkers,
      showWalletMarkers,
      showDevMarkers,
      devMarkersBoundariesOnly,
      showAthLine,
      showMigrationLine,
      highlightVolumeBars,
      showFlowLines,
    });
  }, [
    style,
    groupMode,
    interval,
    basis,
    trimEmptyBars,
    seedCreatorAsVol,
    showTradeMarkers,
    showWalletMarkers,
    showDevMarkers,
    devMarkersBoundariesOnly,
    showAthLine,
    showMigrationLine,
    highlightVolumeBars,
    showFlowLines,
  ]);

  // Bar time-keys whose trades are ALL volume-classified (checked ix-patterns +
  // creator seed + forward contagion — the same classifier the overlay lines
  // use). A bar with even one organic trade is excluded. Drives the "highlight
  // volume bars" spotlight. Empty (cheap) while the toggle is off.
  const pureVolumeBarTimes = useMemo(() => {
    const set = new Set<number>();
    if (!highlightVolumeBars) return set;
    const classified = classifyFlowTrades(
      sortedTrades.map((t) => ({
        wallet_address: t.wallet_address,
        sol: t.amount_sol,
        ix_labels: t.instruction_labels,
        block_time: t.block_time,
        slot: t.slot,
      })),
      classifyOpts,
    );
    // Per bar, tally total vs volume trades; keep only the bars where they match.
    const total = new Map<number, number>();
    const volCount = new Map<number, number>();
    for (const c of classified) {
      const key = groupMode === 'slot' ? tradeBarSlot(c) : tradeBarTime(c.block_time, intervalSec);
      if (key == null) continue;
      const k = key as number;
      total.set(k, (total.get(k) ?? 0) + 1);
      if (c.isVol) volCount.set(k, (volCount.get(k) ?? 0) + 1);
    }
    for (const [k, t] of total) {
      if ((volCount.get(k) ?? 0) === t) set.add(k);
    }
    return set;
  }, [highlightVolumeBars, sortedTrades, classifyOpts, groupMode, intervalSec]);

  useEffect(() => {
    barsRef.current = bars;
  }, [bars]);
  useEffect(() => {
    linesRef.current = alignedLines;
  }, [alignedLines]);
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
    const initialHeight = fixedHeight ?? responsiveChartHeight(el.clientWidth);
    chartHeightRef.current = initialHeight;
    setChartHeight(initialHeight);
    setChartWidth(el.clientWidth);
    const chart = createChart(
      el,
      createChartOptions(el.clientWidth, initialHeight, groupMode, priceUnit, timezone, {
        dualPriceScale: true,
      }),
    );
    chartRef.current = chart;

    const priceFormat = createChartPriceFormat(priceUnit);
    const mainSeries =
      style === 'candles'
        ? chart.addSeries(CandlestickSeries, {
            ...CANDLE_SERIES_OPTIONS,
            priceScaleId: 'right',
            priceFormat,
          })
        : chart.addSeries(LineSeries, {
            ...LINE_SERIES_OPTIONS,
            priceScaleId: 'right',
            priceFormat,
          });
    mainSeriesRef.current = mainSeries;

    const walletPrim = new WalletMarkersPlugin();
    mainSeries.attachPrimitive(asWalletSeriesPrimitive(walletPrim));
    walletMarkersPrimRef.current = walletPrim;

    const rangePrim = new RangeSelectPlugin();
    mainSeries.attachPrimitive(asRangePrimitive(rangePrim));
    rangeSelectPrimRef.current = rangePrim;

    const volSeries = chart.addSeries(LineSeries, {
      color: VOL_LINE_COLOR,
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

    const handleCrosshairMove = (param: MouseEventParams) => {
      const onRangeLabel =
        param.point != null &&
        (rangeSelectPrimRef.current?.containsLabelPoint(param.point.x, param.point.y) ?? false);
      if (onRangeLabel && rangeStats && param.point) {
        setRangeTooltip({ stats: rangeStats, point: param.point });
        setWalletTooltip(null);
        setCrosshair(null);
        setFlowCrosshair(null);
        setBarTooltip(null);
        return;
      }
      setRangeTooltip(null);

      if (!param.time) {
        setCrosshair(null);
        setFlowCrosshair(null);
        setBarTooltip(null);
        setWalletTooltip(null);
        return;
      }
      const bar = barsRef.current.find((b) => b.time === param.time);
      if (!bar) {
        setCrosshair(null);
        setFlowCrosshair(null);
        setBarTooltip(null);
        setWalletTooltip(null);
        return;
      }
      const flow = flowAt(linesRef.current, param.time);
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
      setFlowCrosshair(flow);

      // Wallet-marker tooltip wins over the bar tooltip — never stack the two
      // boxes on one point (same precedence as the shared token chart).
      const onWalletMarker =
        param.point != null &&
        (walletMarkersPrimRef.current?.containsPoint(param.point.x, param.point.y) ?? false);
      if (onWalletMarker && param.point) {
        const activity = walletActivityMapRef.current.get(bar.time as number);
        setWalletTooltip(activity && activity.length > 0 ? { point: param.point, wallets: activity } : null);
        setBarTooltip(null);
        return;
      }
      setWalletTooltip(null);
      setBarTooltip(
        param.point
          ? {
              ...info,
              barTime: bar.time,
              ageSec: computeBarAgeSecRef.current(bar.time as number),
              style,
              point: param.point,
            }
          : null,
      );
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

    const scaleSync = attachDualPriceScaleSync(chart, el, {
      isPaused: () => rangeSelectModeRef.current,
    });
    scaleSyncRef.current = scaleSync;

    const observer = new ResizeObserver(() => {
      const node = containerRef.current;
      if (!node) return;
      const width = node.clientWidth;
      if (!(width > 0)) return;
      // Feeds the crosshair tooltip's edge flip, so it must track the live width.
      setChartWidth(width);
      // Height is derived from WIDTH only (never the observed height) so there's
      // no height->width->height feedback loop.
      const nextHeight = fixedHeight ?? responsiveChartHeight(width);
      if (nextHeight !== chartHeightRef.current) {
        chartHeightRef.current = nextHeight;
        setChartHeight(nextHeight);
        chart.applyOptions({ width, height: nextHeight });
      } else {
        chart.applyOptions({ width });
      }
    });
    observer.observe(el);

    return () => {
      observer.disconnect();
      scaleSync.detach();
      scaleSyncRef.current = null;
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
      setBarTooltip(null);
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
  }, [style, groupMode, intervalSec, fixedHeight, timezone, priceUnit]);

  useEffect(() => {
    if (style === 'candles') {
      const highlight = selectedBar ? new Set([selectedBar.barTime as number]) : undefined;
      const data = barsToCandleData(bars, highlight);
      // Candles support per-bar colors → ghost the pure-volume ones into the
      // background so the organic candles stand out. Only when there's such a
      // bar. Line mode can't per-point recolor, so it uses markers instead.
      const finalData =
        highlightVolumeBars && pureVolumeBarTimes.size > 0
          ? outlineVolumeCandles(data, pureVolumeBarTimes)
          : data;
      (mainSeriesRef.current as ISeriesApi<'Candlestick'> | null)?.setData(finalData);
    } else {
      (mainSeriesRef.current as ISeriesApi<'Line'> | null)?.setData(barsToLineData(bars));
    }
    // Data-driven refit only — a hand-set price zoom survives it.
    scaleSyncRef.current?.rearm();
  }, [bars, style, selectedBar, highlightVolumeBars, pureVolumeBarTimes]);

  // The token basis is a cumulative whole-token count that runs past
  // lightweight-charts' ±9.007e13 series-value ceiling, so it's charted divided
  // by TOKEN_FLOW_SERIES_SCALE with a custom axis formatter that multiplies back;
  // the SOL bases (cost_sol/value_sol) are small and chart 1:1 (× USD rate when
  // the header toggle is USD). Header/crosshair readouts always read the true
  // (unscaled) values straight off `alignedLines`, then apply display-unit conversion.
  useEffect(() => {
    const tokenScale = flowSeriesScale(basis);
    const displayScale = basis === 'token' ? 1 : solUnitScale;
    const priceFormat =
      basis === 'token'
        ? {
            type: 'custom' as const,
            formatter: (v: number) => formatFlowTokenCount(v * tokenScale),
            minMove: 0.01,
          }
        : {
            type: 'custom' as const,
            formatter: createChartPriceFormatter(priceUnit),
            minMove: 0.01,
          };
    const toData = (pts: FlowLinePoint[]) =>
      pts.map((p) => ({
        time: p.time,
        value: (p.value / tokenScale) * displayScale,
      }));
    volSeriesRef.current?.applyOptions({ priceFormat });
    nonVolSeriesRef.current?.applyOptions({ priceFormat });
    // Only a basis/unit switch changes what the left axis means; `alignedLines`
    // churning is a data update and must not drop a hand-set Y zoom.
    const resetKey = `${basis}|${priceUnit}|${solUnitScale}`;
    if (flowScaleResetKeyRef.current !== resetKey) {
      flowScaleResetKeyRef.current = resetKey;
      scaleSyncRef.current?.reset();
    }
    volSeriesRef.current?.setData(toData(alignedLines.vol));
    nonVolSeriesRef.current?.setData(toData(alignedLines.nonVol));
  }, [alignedLines, basis, priceUnit, solUnitScale]);

  // Show/hide the two flow-split overlay lines (and their shared left price
  // scale) as one unit. Re-applied after any series recreation (structural deps)
  // so the toggle survives a style/group/interval change.
  useEffect(() => {
    volSeriesRef.current?.applyOptions({ visible: showFlowLines });
    nonVolSeriesRef.current?.applyOptions({ visible: showFlowLines });
    chartRef.current?.priceScale('left').applyOptions({ visible: showFlowLines });
  }, [showFlowLines, style, groupMode, intervalSec]);

  // Trade (buy/sell count) markers + the selected-bar marker + line-mode
  // volume-bar highlight (candle mode spotlights via the setData effect above).
  useEffect(() => {
    const series = mainSeriesRef.current;
    if (!series) return;
    const markers: SeriesMarker<UTCTimestamp>[] = showTradeMarkers
      ? buildTradeMarkers(sortedTrades, groupMode, intervalSec)
      : [];
    if (highlightVolumeBars && style === 'line' && pureVolumeBarTimes.size > 0) {
      // Mark the pure-volume points — every trade in them is volume. Line mode
      // can't per-point recolor, so a below-bar grey square IS the highlight;
      // candle mode instead repaints the whole bar grey (setData effect above).
      for (const bar of bars) {
        if (!pureVolumeBarTimes.has(bar.time as number)) continue;
        markers.push({
          time: bar.time,
          position: 'belowBar',
          color: VOL_GHOST_COLOR,
          shape: 'square',
          size: 1,
        });
      }
    }
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
  }, [
    showTradeMarkers,
    sortedTrades,
    groupMode,
    intervalSec,
    bars,
    style,
    selectedBar,
    highlightVolumeBars,
    pureVolumeBarTimes,
  ]);

  // The token's dev/creator as a synthetic tracked wallet, so the existing
  // wallet-marker pipeline tags its first_buy/sell_all lifecycle and renders its
  // triangle silhouette + static dev color — no parallel dev-marker logic (SSOT).
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

  // Tracked-wallet + dev markers share ONE plugin: compose the wallet list from
  // the two independent toggles (dev appended LAST so a dev that's also a tracked
  // wallet dedups to the dev entry — `buildWalletMarkerDefs` keys wallets by
  // address, last write wins), then run the shared marker pipeline once.
  useEffect(() => {
    const wallets: ProfileWalletInfo[] = [];
    if (showWalletMarkers) wallets.push(...profileWallets);
    if (showDevMarkers && devWallet) wallets.push(devWallet);
    if (wallets.length > 0) {
      let defs = buildWalletMarkerDefs(sortedTrades, wallets, bars, groupMode, intervalSec);
      if (devMarkersBoundariesOnly) {
        // Keep only the dev's lifecycle boundaries (first_buy/sell_all) — drop its
        // mid-position adds/trims. Dev defs are the only triangles; tracked-wallet
        // markers (other shapes) pass through untouched.
        defs = defs.filter((d) => d.shape !== 'triangle' || d.role != null);
      }
      walletMarkersPrimRef.current?.setMarkers(defs);
      walletActivityMapRef.current = buildWalletBarActivityMap(
        sortedTrades,
        wallets,
        groupMode,
        intervalSec,
      );
    } else {
      walletMarkersPrimRef.current?.setMarkers([]);
      walletActivityMapRef.current = new Map();
    }
  }, [
    showWalletMarkers,
    showDevMarkers,
    devMarkersBoundariesOnly,
    devWallet,
    sortedTrades,
    profileWallets,
    bars,
    groupMode,
    intervalSec,
    style,
  ]);

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
  }, [showAthLine, athPriceInSol, style, toValue]);

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
  }, [showMigrationLine, style, toValue]);

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
        chart.applyOptions({
          handleScroll: true,
          handleScale: { ...DUAL_CHART_HANDLE_SCALE },
        });
        scaleSyncRef.current?.rearm();
      }
    };
  }, [rangeSelectMode, style, groupMode, intervalSec, fixedHeight, timezone]);

  // True (unscaled) cumulative readouts — token counts render compact (with a
  // trillions tier); SOL bases follow the header SOL/USD toggle. Mirrors the
  // scaled series above.
  const formatFlow = (v: number) =>
    basis === 'token' ? formatFlowTokenCount(v) : formatPrice(toValue(v));
  const readVol = flowCrosshair?.vol ?? alignedLines.vol.at(-1)?.value ?? null;
  const readNonVol = flowCrosshair?.nonVol ?? alignedLines.nonVol.at(-1)?.value ?? null;
  const moreActive = showMore || showAthLine || showMigrationLine || rangeSelectMode;

  // Shared columns already include ix_labels (+ optional Vol badge). Flow
  // discovery prepends only the interactive draft-pattern checkbox — never a
  // second ix column. Pass no flowPatternKeys so the read-only badge stays
  // hidden here (checkbox is the draft-edit control).
  const baseTradeColumns = useMemo(() => tokenTradeColumns('SOL'), []);
  const tradeColumns = useMemo<ColumnDef<TradeRecord>[]>(() => {
    if (!onTogglePattern) return baseTradeColumns;
    const volToggle: ColumnDef<TradeRecord> = {
      key: 'vol_pattern',
      label: 'Vol',
      tooltip:
        'Flag this trade’s ix-structure as manufactured volume — adds it to the draft ' +
        'volume_ix_patterns (applies to EVERY trade with this exact shape, not just this row).',
      render: (t) => {
        const labels = t.instruction_labels;
        if (!labels || labels.length === 0) {
          return <span className="text-text-dim/40">—</span>;
        }
        return (
          <Checkbox
            checked={patternKeys.has(JSON.stringify(labels))}
            onChange={() => onTogglePattern(labels)}
          />
        );
      },
      searchValue: () => '',
    };
    return [volToggle, ...baseTradeColumns];
  }, [baseTradeColumns, patternKeys, onTogglePattern]);
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
          active={showDevMarkers}
          onClick={() => setShowDevMarkers((v) => !v)}
          disabled={!creatorWallet}
          label="Toggle dev/creator markers"
          tooltip={
            creatorWallet
              ? 'Dev/creator wallet markers — triangle silhouette with heavier first_buy (entry) and sell_all (full exit) markers'
              : 'No creator wallet known for this token'
          }
          activeColor={CHART_COLORS.dev}
        >
          <DevMarkersIcon />
        </IconToggleButton>

        <IconToggleButton
          active={trimEmptyBars}
          onClick={() => setTrimEmptyBars((v) => !v)}
          label="Toggle trimming of empty bars"
          tooltip="Hide flat bars for intervals with no trades"
        >
          <TrimGapsIcon />
        </IconToggleButton>

        <IconToggleButton
          active={showFlowLines}
          onClick={() => setShowFlowLines((v) => !v)}
          label="Toggle vol/non-vol flow lines"
          tooltip="Show/hide the cumulative volume-maker (red) vs non-volume (gold) overlay lines"
        >
          <FlowLinesIcon />
        </IconToggleButton>

        <IconToggleButton
          active={highlightVolumeBars}
          onClick={() => setHighlightVolumeBars((v) => !v)}
          label="Toggle flow-split candle highlight"
          tooltip="Fade the pure-volume candles into the background (shallow dark) — bars whose every trade is volume-classified (checked patterns + creator + contagion). Bars with any organic trade keep their normal up/down color"
          activeColor={VOL_GHOST_COLOR}
        >
          <VolumeBarsIcon />
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

          <label
            className={cn(
              'flex cursor-pointer items-center gap-1.5 rounded-md px-2 py-1 text-[11px] font-semibold',
              (!showDevMarkers || !creatorWallet) && 'cursor-not-allowed opacity-40',
            )}
            style={{ backgroundColor: CHART_COLORS.grid, color: CHART_COLORS.panelTextDim }}
            title={
              creatorWallet
                ? 'Show only the dev first_buy (entry) and sell_all (full exit) markers — hides the dev’s mid-position buys/sells when manufactured volume clutters the chart. Enable the Dev toggle first.'
                : 'No creator wallet known for this token'
            }
          >
            <Checkbox
              boxSize="sm"
              checked={devMarkersBoundariesOnly}
              disabled={!showDevMarkers || !creatorWallet}
              onChange={(e) => setDevMarkersBoundariesOnly(e.target.checked)}
            />
            <span
              style={
                devMarkersBoundariesOnly && showDevMarkers ? { color: CHART_COLORS.dev } : undefined
              }
            >
              Dev boundaries only
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
                ? 'ON (default) mirrors live — the live classifier always treats the creator wallet as volume-side. Turn OFF only to isolate what the checked ix-patterns alone would catch.'
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
        <span style={{ color: VOL_LINE_COLOR }}>
          <span className="font-semibold">VolMk</span>{' '}
          {readVol != null ? formatFlow(readVol) : '—'}
        </span>
        <span style={{ color: NON_VOL_COLOR }}>
          <span className="font-semibold">NonVol</span>{' '}
          {readNonVol != null ? formatFlow(readNonVol) : '—'}
        </span>
      </div>

      <div className="relative">
        <div ref={containerRef} style={{ height: chartHeight }} />

        {rangeTooltip && (
          <RangeSelectTooltip
            tooltip={rangeTooltip}
            formatAmount={(sol) => formatVol(sol)}
            formatPrice={(p) => formatPrice(p)}
            containerWidth={chartWidth}
          />
        )}
        {!rangeTooltip && walletTooltip && (
          <WalletMarkersTooltip tooltip={walletTooltip} containerWidth={chartWidth} />
        )}
        {/* The O/H/L/C/Vol/Liq readout lives in the header above — the tooltip
            carries what the header CAN'T: which bar you're on (time + age since
            launch) and its per-bar order flow (Net/In/Out/Δ). Same split, same
            component, as the shared token charts. */}
        {!rangeTooltip && !walletTooltip && barTooltip && (
          <BarCrosshairTooltip
            tooltip={barTooltip}
            formatVol={formatVol}
            formatFlow={formatFlow}
            formatTime={formatBarTime}
            containerWidth={chartWidth}
          />
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
            rowKey={flowTradeRowKey}
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
