import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useDispatch, useSelector } from 'react-redux';
import { STORAGE_KEYS, getString, setString } from 'lib/storage';
import { DataTable } from 'components/table/DataTable';
import type { TableQuery } from 'components/table/types';
import { tokenTradeColumns } from 'components/transactions/tokenTradeColumns';
import {
  TokenPriceChart,
  WALLET_MARKER_COLORS,
  tradeBarTime,
  type ChartBarSelection,
  type ChartMetric,
  type ChartSwingLeg,
  type ProfileWalletInfo,
} from 'components/token-price-chart';
import { swingLegKey } from 'components/token-price-chart/swingOverlay';
import { FilterPanel } from 'components/tokens/FilterPanel';
import {
  activeFilterCount,
  defaultFilters,
  loadStoredTokenFilters,
  saveStoredTokenFilters,
  type TokenFilters,
} from 'components/tokens/filters';
import { tokenColumns } from 'components/tokens/tokenColumns';
import { usePriceUnit } from 'context/PriceUnitContext';
import { useTimezone } from 'context/TimezoneContext';
import { formatTimestampMs } from 'utils/date';
import { usePriceDisplay } from 'hooks/usePriceDisplay';
import { swingColumns } from 'components/analysis/swingColumns';
import {
  DEFAULT_SWING_PARAMS,
  SWING_PARAM_INT_KEYS,
  RangeInputs,
  SwingParamsGrid,
  SwingRangeField,
  isFiniteNumber,
  mergeSwingParams,
  swingParamsFromForm,
  swingParamLabelClassName as labelClassName,
  type SwingParamsForm,
} from 'components/analysis/swingParams';
import { computeChainStats, type SwingChainStats } from 'components/analysis/swingChains';
import { swingChainColumns } from 'components/analysis/swingChainColumns';
import {
  DEFAULT_SWING_FILTER,
  filterSwings,
  hasActiveSwingFilter,
  parseSwingFilterField,
  swingFilterFromForm,
  type SwingFilterCriteria,
  type SwingFilterForm,
} from 'components/analysis/swingFilter';
import { Badge } from 'components/ui/Badge';
import { SectionDivider } from 'components/ui/SectionDivider';
import { Button } from 'components/ui/Button';
import { Checkbox } from 'components/ui/Checkbox';
import { Input } from 'components/ui/Input';
import { Select } from 'components/ui/Select';
import { Tabs, TabsList, TabsPanel, TabsTrigger } from 'components/ui/Tabs';
import { VisibilityToggleButton } from 'components/ui/VisibilityToggleButton';
import { fetchTokenSwings, fetchTokenSwingsBatch } from 'services/api';
import {
  apiSlice,
  apiErrorMessage,
  TOKENS_LIST_LIMIT,
  useGetProfilesQuery,
  useGetTokenDetailQuery,
  useGetTokenTradesQuery,
  useGetTokensPageQuery,
} from 'store/apiSlice';
import type { AppDispatch, RootState } from '../../store';
import {
  clearCreatedRange,
  setCreatedFrom,
  setCreatedTo,
  setSelectedMint,
  setSelectedSwingKey,
  setSwingAllResults,
  setSwingResult,
  setSwingRunId,
  setTokensFetched,
} from 'store/swingDetectionSlice';
import type {
  SwingBatchEntry,
  SwingLegRecord,
  SwingParams,
  TokenRecord,
  TradeRecord,
  WalletProfile,
} from 'types';

/** Stable empty references so derived memos don't recompute every render. */
const EMPTY_TOKENS: TokenRecord[] = [];
const EMPTY_TRADES: TradeRecord[] = [];
const EMPTY_PROFILES: WalletProfile[] = [];

/**
 * Column keys of the browser-derived "Chain of Swings" columns (see
 * `swingChainColumns`). Sorting by one of these routes through the backend's
 * swing-run sort path; any other column sorts on a real `TokenSummary` field.
 */
const SWING_CHAIN_SORT_COLS = new Set(['swing_pairs', 'max_seq_pairs', 'chain_count']);

/** Initial table view-state; pageSize matches the DataTable default (10). */
const INITIAL_QUERY: TableQuery = {
  page: 1,
  pageSize: 10,
  sortKeys: [],
  search: '',
  colFilters: {},
};


type AnalysisKind = 'swing';
type SwingPanelTab = 'analysis' | 'chain' | 'timerange' | 'thresholds' | 'filter';
type SwingAllTab = 'analysis' | 'chain' | 'timerange' | 'thresholds';

const LS_SWING_DETECTION_KEY = STORAGE_KEYS.swingCriteria;

/** Default max idle gap (ms) for two swings to count as the same chain. */
const DEFAULT_CHAIN_LATENCY_MS = 60_000;

/**
 * Max mints per `/api/tokens/swings/batch` request. The backend rejects batches
 * larger than 200 (each mint is a serial DB load + scan on one worker thread),
 * so "Swing Detection All" splits the filtered set into chunks of this size and
 * runs them sequentially, merging the results. Keep this ≤ the backend cap.
 */
const SWING_BATCH_CHUNK_SIZE = 200;

/**
 * Max swing-detection batch requests in flight at once. The backend serves each
 * on a shared HTTP worker (only 2) and fans its DB loads out internally, so a
 * few concurrent chunks overlap the round-trips while leaving a worker free for
 * the rest of the app. Kept small on purpose — higher just queues on the workers.
 */
const SWING_BATCH_REQUEST_CONCURRENCY = 3;

function mergeSwingFilter(partial: Partial<SwingFilterCriteria> | undefined): SwingFilterCriteria {
  if (!partial) return DEFAULT_SWING_FILTER;
  const legType = partial.leg_type;
  const merged: SwingFilterCriteria = {
    ...DEFAULT_SWING_FILTER,
    leg_type:
      legType === 'all' || legType === 'swing_high' || legType === 'swing_low'
        ? legType
        : DEFAULT_SWING_FILTER.leg_type,
  };
  for (const key of Object.keys(DEFAULT_SWING_FILTER) as (keyof SwingFilterCriteria)[]) {
    if (key === 'leg_type') continue;
    const value = partial[key];
    if (isFiniteNumber(value)) merged[key] = value;
  }
  return merged;
}

interface StoredSwingCriteria {
  // Global ("Swing Detection All") panel state.
  params: SwingParams;
  filter: SwingFilterCriteria;
  appliedFilter: SwingFilterCriteria;
  connectSwings: boolean;
  chainLatencyMs: number;
  windowStartSec: number | null;
  windowEndSec: number | null;
  curveOnly: boolean;
  // Single-token panel state — kept fully independent of the global panel so
  // tuning one never affects the other.
  singleParams: SwingParams;
  singleChainLatencyMs: number;
  singleWindowStartSec: number | null;
  singleWindowEndSec: number | null;
  singleCurveOnly: boolean;
}

const DEFAULT_SWING_CRITERIA: StoredSwingCriteria = {
  params: DEFAULT_SWING_PARAMS,
  filter: DEFAULT_SWING_FILTER,
  appliedFilter: DEFAULT_SWING_FILTER,
  connectSwings: true,
  chainLatencyMs: DEFAULT_CHAIN_LATENCY_MS,
  windowStartSec: null,
  windowEndSec: null,
  curveOnly: false,
  singleParams: DEFAULT_SWING_PARAMS,
  singleChainLatencyMs: DEFAULT_CHAIN_LATENCY_MS,
  singleWindowStartSec: null,
  singleWindowEndSec: null,
  singleCurveOnly: false,
};

function loadStoredSwingCriteria(): StoredSwingCriteria {
  try {
    const raw = getString(LS_SWING_DETECTION_KEY);
    if (!raw) return DEFAULT_SWING_CRITERIA;
    const parsed = JSON.parse(raw) as {
      params?: Partial<SwingParams>;
      filter?: Partial<SwingFilterCriteria>;
      appliedFilter?: Partial<SwingFilterCriteria>;
      connectSwings?: boolean;
      chainLatencyMs?: number;
      windowStartSec?: number | null;
      windowEndSec?: number | null;
      curveOnly?: boolean;
      singleParams?: Partial<SwingParams>;
      singleChainLatencyMs?: number;
      singleWindowStartSec?: number | null;
      singleWindowEndSec?: number | null;
      singleCurveOnly?: boolean;
    };
    return {
      params: mergeSwingParams(parsed.params),
      filter: mergeSwingFilter(parsed.filter),
      appliedFilter: mergeSwingFilter(parsed.appliedFilter ?? parsed.filter),
      connectSwings: parsed.connectSwings !== false,
      chainLatencyMs: isFiniteNumber(parsed.chainLatencyMs)
        ? parsed.chainLatencyMs
        : DEFAULT_CHAIN_LATENCY_MS,
      windowStartSec: isFiniteNumber(parsed.windowStartSec) ? parsed.windowStartSec : null,
      windowEndSec: isFiniteNumber(parsed.windowEndSec) ? parsed.windowEndSec : null,
      curveOnly: parsed.curveOnly === true,
      // Migration fallback: seed the single panel from the (formerly shared)
      // global params so existing setups don't reset to defaults on first load.
      singleParams: mergeSwingParams(parsed.singleParams ?? parsed.params),
      singleChainLatencyMs: isFiniteNumber(parsed.singleChainLatencyMs)
        ? parsed.singleChainLatencyMs
        : DEFAULT_CHAIN_LATENCY_MS,
      singleWindowStartSec: isFiniteNumber(parsed.singleWindowStartSec)
        ? parsed.singleWindowStartSec
        : null,
      singleWindowEndSec: isFiniteNumber(parsed.singleWindowEndSec)
        ? parsed.singleWindowEndSec
        : null,
      singleCurveOnly: parsed.singleCurveOnly === true,
    };
  } catch {
    return DEFAULT_SWING_CRITERIA;
  }
}

function saveStoredSwingCriteria(criteria: StoredSwingCriteria): void {
  setString(LS_SWING_DETECTION_KEY, JSON.stringify(criteria));
}

export function SwingDetectionPage() {
  const [storedSwingCriteria] = useState(loadStoredSwingCriteria);

  // Persisted in Redux so the page restores its state across navigation
  // (routes unmount on leave): fetched token results, time range, selected
  // token, swing result, and selected swing leg.
  const dispatch = useDispatch<AppDispatch>();
  const tokensFetched = useSelector((s: RootState) => s.swingDetection.tokensFetched);
  const createdFrom = useSelector((s: RootState) => s.swingDetection.createdFrom);
  const createdTo = useSelector((s: RootState) => s.swingDetection.createdTo);
  const selectedMint = useSelector((s: RootState) => s.swingDetection.selectedMint);
  const swingResult = useSelector((s: RootState) => s.swingDetection.swingResult);
  const selectedSwingKey = useSelector((s: RootState) => s.swingDetection.selectedSwingKey);
  const swingAllResults = useSelector((s: RootState) => s.swingDetection.swingAllResults);
  const swingRunId = useSelector((s: RootState) => s.swingDetection.swingRunId);

  const price = usePriceDisplay();
  const { unit, usdRate } = usePriceUnit();
  const { timezone } = useTimezone();
  const tradeTableColumns = useMemo(() => tokenTradeColumns(price.unitLabel), [price.unitLabel]);
  const swingTableColumns = useMemo(() => swingColumns(price, timezone), [price, timezone]);

  const toChartValue = useCallback(
    (sol: number) => (unit === 'USD' && usdRate != null ? sol * usdRate : sol),
    [unit, usdRate],
  );

  const [showFilters, setShowFilters] = useState(false);
  const [filters, setFilters] = useState<TokenFilters>(loadStoredTokenFilters);

  // View-state emitted by the DataTable (page/sort/search/col-filters). The
  // backend does the filtering/sorting/paging now, so only one page crosses the
  // wire instead of the whole 20k-row list.
  const [tableQuery, setTableQuery] = useState<TableQuery>(INITIAL_QUERY);

  // The page's dedicated Created from/to controls fold into the same server
  // filter set as the global panel (the backend parses both as `f_created_*`),
  // so the date range now reduces the query server-side too.
  const effectiveFilters = useMemo<TokenFilters>(
    () => ({
      ...filters,
      created_from: createdFrom || filters.created_from,
      created_to: createdTo || filters.created_to,
    }),
    [filters, createdFrom, createdTo],
  );

  // Chain-of-swings latency budget (ms) for the global "All" run. Declared here
  // (above queryArgs) so the tokens-page query can send it when sorting a chain
  // column — the backend re-groups the run's raw legs at this latency.
  const [chainLatencyMs, setChainLatencyMs] = useState<number | ''>(
    storedSwingCriteria.chainLatencyMs,
  );
  const chainLatencyValue = typeof chainLatencyMs === 'number' ? chainLatencyMs : 0;

  // Debounced copy of the global chain latency. The raw input stays responsive
  // (it drives `value={chainLatencyMs}`), but the expensive consumers — the
  // O(n) `chainStatsByMint` grouping over every mint and the server-side query
  // re-group — read this settled value so a burst of keystrokes coalesces into
  // one recompute instead of one per character.
  const [debouncedChainLatency, setDebouncedChainLatency] = useState(chainLatencyValue);
  useEffect(() => {
    const id = setTimeout(() => setDebouncedChainLatency(chainLatencyValue), 200);
    return () => clearTimeout(id);
  }, [chainLatencyValue]);

  // Whether the table is currently ordered by a browser-derived chain column.
  // Only then are the run id + latency sent (and folded into the cache key), so
  // tweaking latency while sorting a normal column doesn't trigger a refetch.
  const swingSortActive = tableQuery.sortKeys.some((s) => SWING_CHAIN_SORT_COLS.has(s.col));

  const queryArgs = useMemo(
    () => ({
      page: tableQuery.page,
      pageSize: tableQuery.pageSize,
      sortKeys: tableQuery.sortKeys,
      search: tableQuery.search,
      colFilters: tableQuery.colFilters,
      filters: effectiveFilters,
      timezone,
      swingRunId: swingSortActive ? swingRunId : undefined,
      swingChainLatencyMs: swingSortActive ? debouncedChainLatency : undefined,
    }),
    [tableQuery, effectiveFilters, timezone, swingSortActive, swingRunId, debouncedChainLatency],
  );
  // Resets the table to page 1 whenever the server-side reduction changes.
  const filtersResetKey = useMemo(() => JSON.stringify(effectiveFilters), [effectiveFilters]);

  // Lazily enabled on the "Fetch" button (the `tokensFetched` flag lives in
  // Redux so the table re-appears on return). Server-side paged: each
  // page/sort/filter change fetches just that page.
  const {
    data: tokensData,
    isFetching: loading,
    error: tokensError,
    refetch: refetchTokens,
  } = useGetTokensPageQuery(queryArgs, { skip: !tokensFetched });

  // Keep-previous-data: a page/sort/filter change targets a cache key with no
  // data yet, so `tokensData` is briefly undefined. Hold the last rendered page
  // until the new one lands rather than blanking the grid each interaction.
  const lastItemsRef = useRef<TokenRecord[]>(EMPTY_TOKENS);
  if (tokensData?.items) lastItemsRef.current = tokensData.items;
  const pageTokens = tokensData?.items ?? lastItemsRef.current;
  const total = tokensData?.total ?? 0;
  // Once they've clicked Fetch the analysis UI stays mounted; the query's own
  // `loading` flag drives the in-table busy state across page changes.
  const loaded = tokensFetched;
  const error = apiErrorMessage(tokensError, 'Failed to load tokens');
  // Tracked-wallet markers for the chart. Read from the shared RTK cache so this
  // and the Sync page dedupe one fetch and reuse it across navigation.
  const { data: profiles = EMPTY_PROFILES } = useGetProfilesQuery();

  const profileWallets = useMemo<ProfileWalletInfo[]>(() => {
    const result: ProfileWalletInfo[] = [];
    let colorIdx = 0;
    for (const profile of profiles) {
      for (const wallet of profile.wallets) {
        if (!wallet.is_tracked) continue;
        result.push({
          address: wallet.address,
          label: wallet.comment
            ? wallet.comment.slice(0, 12)
            : `${wallet.address.slice(0, 4)}…${wallet.address.slice(-4)}`,
          color: WALLET_MARKER_COLORS[colorIdx % WALLET_MARKER_COLORS.length],
          profileName: profile.name,
          tags: profile.tags.map((t) => ({ name: t.name, color: t.color })),
        });
        colorIdx++;
      }
    }
    return result;
  }, [profiles]);

  // Selected token's detail (symbol / flags / ATH / created_at for the chart).
  // Fetched by mint so the chart keeps its metadata even after paging away from
  // the row in server-side mode, where the selected token may not be on screen.
  const { data: selectedDetail } = useGetTokenDetailQuery(selectedMint ?? '', {
    skip: !selectedMint,
  });

  // Per-mint trades cached by mint, so re-selecting a token doesn't re-pull.
  const {
    data: tradesData,
    isFetching: tradesLoading,
    error: tradesErrorRaw,
  } = useGetTokenTradesQuery(selectedMint ?? '', { skip: !selectedMint });
  const trades = tradesData ?? EMPTY_TRADES;
  const tradesError = selectedMint
    ? apiErrorMessage(tradesErrorRaw, 'Failed to load trades')
    : null;
  const [selectedBar, setSelectedBar] = useState<ChartBarSelection | null>(null);
  const [chartMetric, setChartMetric] = useState<ChartMetric>('price');

  const [activeAnalysis, setActiveAnalysis] = useState<AnalysisKind | null>(null);
  const [swingParams, setSwingParams] = useState<SwingParamsForm>(storedSwingCriteria.params);
  const [swingLoading, setSwingLoading] = useState(false);
  const [swingError, setSwingError] = useState<string | null>(null);
  const [swingPanelTab, setSwingPanelTab] = useState<SwingPanelTab>('analysis');
  const [swingFilter, setSwingFilter] = useState<SwingFilterForm>(storedSwingCriteria.filter);
  const [appliedSwingFilter, setAppliedSwingFilter] = useState<SwingFilterCriteria>(
    storedSwingCriteria.appliedFilter,
  );
  const [showSwingResultsTable, setShowSwingResultsTable] = useState(false);
  const [connectSwings, setConnectSwings] = useState(storedSwingCriteria.connectSwings);

  // Single-token panel parameters — independent copies of the global panel's
  // controls (Analysis / Chain of Swings / Time Range / Leg Thresholds) so the
  // single token can be tuned and run on its own.
  const [singleSwingParams, setSingleSwingParams] = useState<SwingParamsForm>(
    storedSwingCriteria.singleParams,
  );
  const [singleChainLatencyMs, setSingleChainLatencyMs] = useState<number | ''>(
    storedSwingCriteria.singleChainLatencyMs,
  );
  const [singleWindowStartSec, setSingleWindowStartSec] = useState<number | ''>(
    storedSwingCriteria.singleWindowStartSec ?? '',
  );
  const [singleWindowEndSec, setSingleWindowEndSec] = useState<number | ''>(
    storedSwingCriteria.singleWindowEndSec ?? '',
  );
  const [singleCurveOnly, setSingleCurveOnly] = useState(storedSwingCriteria.singleCurveOnly);

  // "Swing Detection All" — global detection across every filtered token. Raw
  // swings per mint are kept so tuning the chain latency re-groups instantly
  // without re-fetching.
  const [showSwingAll, setShowSwingAll] = useState(false);
  const [swingAllTab, setSwingAllTab] = useState<SwingAllTab>('analysis');
  // Launch-relative detection window (seconds from each token's first trade);
  // '' = open on that side. Applied to every "Swing Detection All" run.
  const [windowStartSec, setWindowStartSec] = useState<number | ''>(
    storedSwingCriteria.windowStartSec ?? '',
  );
  const [windowEndSec, setWindowEndSec] = useState<number | ''>(
    storedSwingCriteria.windowEndSec ?? '',
  );
  // When set, the window is fixed to each token's bonding-curve phase
  // (creation → migration), overriding the manual start/end seconds.
  const [curveOnly, setCurveOnly] = useState(storedSwingCriteria.curveOnly);
  const [swingAllLoading, setSwingAllLoading] = useState(false);
  const [swingAllError, setSwingAllError] = useState<string | null>(null);
  // Progress across the sequential batch chunks (done/total mints), so a run
  // over thousands of tokens shows forward movement instead of a frozen spinner.
  const [swingAllProgress, setSwingAllProgress] = useState<{ done: number; total: number } | null>(
    null,
  );

  // Batch result lives in Redux (as the serializable array the API returns);
  // index it by mint here for lookups and chain-stat grouping.
  const swingsByMint = useMemo(() => {
    const map = new Map<string, SwingLegRecord[]>();
    if (swingAllResults) {
      for (const entry of swingAllResults) map.set(entry.mint, entry.swings);
    }
    return map;
  }, [swingAllResults]);
  const swingAllRanCount = swingAllResults ? swingAllResults.length : null;

  const singleChainLatencyValue =
    typeof singleChainLatencyMs === 'number' ? singleChainLatencyMs : 0;

  const chainStatsByMint = useMemo(() => {
    const map = new Map<string, SwingChainStats>();
    for (const [mint, swings] of swingsByMint) {
      map.set(mint, computeChainStats(swings, debouncedChainLatency));
    }
    return map;
  }, [swingsByMint, debouncedChainLatency]);

  // Base token columns built once (their price cells read the rate from context);
  // only the appended chain columns rebuild when a global run updates the stats.
  // The chain columns ARE sortable: a header click sorts server-side from the
  // run's raw legs (see `swingRunId` + the backend's swing-sort path), so the
  // ordering spans the whole filtered set under pagination, not just this page.
  const baseTokenColumns = useMemo(() => tokenColumns(), []);
  const columns = useMemo(
    () => [...baseTokenColumns, ...swingChainColumns(chainStatsByMint, true)],
    [baseTokenColumns, chainStatsByMint],
  );

  const toggleAnalysis = useCallback((kind: AnalysisKind) => {
    setActiveAnalysis((prev) => (prev === kind ? null : kind));
  }, []);

  const updateSwingParam = useCallback(
    <K extends keyof SwingParams>(key: K, raw: string) => {
      const parsed = SWING_PARAM_INT_KEYS.has(key)
        ? parseInt(raw, 10)
        : parseFloat(raw);
      setSwingParams((prev) => ({
        ...prev,
        [key]: Number.isFinite(parsed) ? parsed : '',
      }));
    },
    [],
  );

  const updateSingleSwingParam = useCallback(
    <K extends keyof SwingParams>(key: K, raw: string) => {
      const parsed = SWING_PARAM_INT_KEYS.has(key)
        ? parseInt(raw, 10)
        : parseFloat(raw);
      setSingleSwingParams((prev) => ({
        ...prev,
        [key]: Number.isFinite(parsed) ? parsed : '',
      }));
    },
    [],
  );

  const updateSwingFilter = useCallback(
    <K extends keyof SwingFilterCriteria>(key: K, raw: string) => {
      setSwingFilter((prev) => ({
        ...prev,
        [key]: parseSwingFilterField(key, raw, prev),
      }));
    },
    [],
  );

  const handleRunSwing = useCallback(async () => {
    if (!selectedMint) return;
    setSwingLoading(true);
    setSwingError(null);
    try {
      const result = await fetchTokenSwings(
        selectedMint,
        swingParamsFromForm(singleSwingParams),
        {
          startMs:
            singleCurveOnly || singleWindowStartSec === ''
              ? null
              : Math.round(singleWindowStartSec * 1000),
          endMs:
            singleCurveOnly || singleWindowEndSec === ''
              ? null
              : Math.round(singleWindowEndSec * 1000),
          curveOnly: singleCurveOnly,
        },
      );
      dispatch(setSwingResult(result));
    } catch (e) {
      dispatch(setSwingResult(null));
      setSwingError(e instanceof Error ? e.message : 'Swing detection failed');
    } finally {
      setSwingLoading(false);
    }
  }, [
    selectedMint,
    singleSwingParams,
    singleWindowStartSec,
    singleWindowEndSec,
    singleCurveOnly,
    dispatch,
  ]);

  // Monotonic id identifying the in-flight "Run All". Starting a new run or
  // unmounting the page bumps it; the running workers check it after every await
  // and bail before applying stale results — so a re-run or navigating away never
  // keeps firing batch calls against the old set or dispatches a superseded result.
  const swingAllRunIdRef = useRef(0);
  useEffect(
    () => () => {
      swingAllRunIdRef.current++;
    },
    [],
  );

  // Run detection across all currently-filtered tokens, then keep the raw swings
  // per mint for client-side chain grouping. With server-side paging only the
  // visible page is in memory, so the full filtered mint set is fetched here on
  // demand (one large request, reusing the table's exact filter args) — the heavy
  // full-list load now happens only on an explicit Run, never on page view. The
  // backend caps each batch detection request at SWING_BATCH_CHUNK_SIZE mints, so
  // the set is split into chunks run sequentially and their results merged.
  const handleRunAllSwings = useCallback(async () => {
    const myRun = ++swingAllRunIdRef.current;
    const isStale = () => swingAllRunIdRef.current !== myRun;
    setSwingAllLoading(true);
    setSwingAllError(null);
    setSwingAllProgress(null);
    try {
      // Pull every filtered mint (sort doesn't matter for the set; cap at the
      // shared list limit). Materialised once, then dropped.
      const sub = dispatch(
        apiSlice.endpoints.getTokensPage.initiate(
          { ...queryArgs, page: 1, pageSize: TOKENS_LIST_LIMIT, sortKeys: [] },
          { forceRefetch: true },
        ),
      );
      let mints: string[];
      try {
        const data = await sub.unwrap();
        mints = data.items.map((t) => t.mint_address);
      } finally {
        sub.unsubscribe();
      }
      if (isStale() || mints.length === 0) return;
      setSwingAllProgress({ done: 0, total: mints.length });
      const params = swingParamsFromForm(swingParams);
      // One id shared by every chunk of this run; the backend stashes the raw
      // legs under it so the tokens list can sort the chain columns from them.
      const runId = crypto.randomUUID();
      const opts = {
        startMs: curveOnly || windowStartSec === '' ? null : Math.round(windowStartSec * 1000),
        endMs: curveOnly || windowEndSec === '' ? null : Math.round(windowEndSec * 1000),
        curveOnly,
        runId,
      };
      // Split into ≤200-mint chunks and run a bounded number concurrently. The
      // backend now fans each chunk's DB loads out internally and runs the CPU
      // scan off the HTTP worker, so a few in-flight chunks overlap their
      // round-trips without swamping the 2 HTTP workers / connection pool.
      const chunks: string[][] = [];
      for (let i = 0; i < mints.length; i += SWING_BATCH_CHUNK_SIZE) {
        chunks.push(mints.slice(i, i + SWING_BATCH_CHUNK_SIZE));
      }
      const chunkResults: SwingBatchEntry[][] = new Array(chunks.length);
      let nextChunk = 0;
      let doneMints = 0;
      const worker = async () => {
        for (;;) {
          const idx = nextChunk++;
          if (idx >= chunks.length) return;
          const resp = await fetchTokenSwingsBatch(chunks[idx], params, opts);
          if (isStale()) return; // re-run / unmount superseded this run
          chunkResults[idx] = resp.results; // indexed write keeps result order
          doneMints += chunks[idx].length;
          setSwingAllProgress({ done: doneMints, total: mints.length });
        }
      };
      await Promise.all(
        Array.from({ length: Math.min(SWING_BATCH_REQUEST_CONCURRENCY, chunks.length) }, worker),
      );
      if (isStale()) return;
      dispatch(setSwingAllResults(chunkResults.flat()));
      // Publish the run id only after every chunk landed, so a chain-column sort
      // reads a fully-populated server-side run.
      dispatch(setSwingRunId(runId));
    } catch (e) {
      if (!isStale()) setSwingAllError(e instanceof Error ? e.message : 'Swing detection failed');
    } finally {
      // Only the current run owns the busy state — a superseded run (re-run /
      // unmount) leaves it for the new owner and avoids a post-unmount setState.
      if (!isStale()) {
        setSwingAllLoading(false);
        setSwingAllProgress(null);
      }
    }
  }, [queryArgs, swingParams, windowStartSec, windowEndSec, curveOnly, dispatch]);

  const filterCount = activeFilterCount(filters);

  const selectedToken = selectedDetail ?? null;

  // First click loads the shared token cache; later clicks force a refetch.
  // A selection that scrolls out of the date range is cleared by the effect
  // below that watches `displayed`.
  const handleRefresh = useCallback(() => {
    if (!tokensFetched) {
      dispatch(setTokensFetched(true));
    } else {
      refetchTokens();
    }
  }, [tokensFetched, refetchTokens, dispatch]);

  const handleSelectMint = useCallback(
    (mint: string | null) => {
      dispatch(setSelectedMint(mint));
    },
    [dispatch],
  );

  useEffect(() => {
    saveStoredSwingCriteria({
      params: swingParamsFromForm(swingParams),
      filter: swingFilterFromForm(swingFilter),
      appliedFilter: appliedSwingFilter,
      connectSwings,
      chainLatencyMs: chainLatencyValue,
      windowStartSec: windowStartSec === '' ? null : windowStartSec,
      windowEndSec: windowEndSec === '' ? null : windowEndSec,
      curveOnly,
      singleParams: swingParamsFromForm(singleSwingParams),
      singleChainLatencyMs: singleChainLatencyValue,
      singleWindowStartSec: singleWindowStartSec === '' ? null : singleWindowStartSec,
      singleWindowEndSec: singleWindowEndSec === '' ? null : singleWindowEndSec,
      singleCurveOnly,
    });
  }, [
    swingParams,
    swingFilter,
    appliedSwingFilter,
    connectSwings,
    chainLatencyValue,
    windowStartSec,
    windowEndSec,
    curveOnly,
    singleSwingParams,
    singleChainLatencyValue,
    singleWindowStartSec,
    singleWindowEndSec,
    singleCurveOnly,
  ]);

  // Reset local selection-derived UI when the chosen token changes. The Redux
  // swing result and leg selection are cleared by the setSelectedMint reducer;
  // trades themselves are fetched/cached by the useGetTokenTradesQuery hook.
  useEffect(() => {
    setSelectedBar(null);
    setSwingError(null);
  }, [selectedMint]);

  // Selecting a token after a global ("Swing Detection All") run promotes that
  // token's batch swings into `swingResult`, so the per-token Analysis/Filter
  // panel and chart treat them exactly like a single-token "Run" — which is what
  // lets the user filter them. A genuine per-token Run for this mint already sets
  // `swingResult` (mint matches), so it's left untouched here.
  useEffect(() => {
    if (!selectedMint) return;
    if (swingResult && swingResult.mint === selectedMint) return;
    const batch = swingsByMint.get(selectedMint);
    if (!batch) return;
    dispatch(
      setSwingResult({
        mint: selectedMint,
        params: swingParamsFromForm(swingParams),
        count: batch.length,
        swings: batch,
      }),
    );
  }, [selectedMint, swingResult, swingsByMint, swingParams, dispatch]);

  const handleSwingSelect = useCallback(
    (key: string | null) => {
      dispatch(setSelectedSwingKey(key));
      if (key) setSelectedBar(null);
    },
    [dispatch],
  );

  const handleSwingLegClick = useCallback(
    (leg: ChartSwingLeg | null) => {
      handleSwingSelect(leg ? swingLegKey(leg) : null);
    },
    [handleSwingSelect],
  );

  const handleBarClick = useCallback(
    (selection: ChartBarSelection | null) => {
      setSelectedBar(selection);
      if (selection) dispatch(setSelectedSwingKey(null));
    },
    [dispatch],
  );

  const chartSymbol =
    selectedToken?.symbol || selectedToken?.name || selectedMint || '';

  const chartPriceLabel = chartMetric === 'mc' ? `MC (${unit})` : unit;

  const barTrades = useMemo(() => {
    if (!selectedBar) return [];
    if (selectedBar.groupMode === 'slot') {
      return trades.filter((t) => t.slot === selectedBar.slot);
    }
    return trades.filter(
      (t) =>
        tradeBarTime(t.block_time, selectedBar.intervalSec ?? 60) === selectedBar.barTime,
    );
  }, [trades, selectedBar]);

  const barTimeLabel = selectedBar
    ? selectedBar.groupMode === 'slot'
      ? `Slot ${selectedBar.slot}`
      : formatTimestampMs(Number(selectedBar.barTime) * 1000, timezone)
    : '';

  // Swings to draw on the chart for the selected token. The per-token "Run"
  // result takes precedence (it honors the Analysis/Filter panel); otherwise
  // fall back to the "Swing Detection All" batch result so selecting a token
  // after a batch run still shows its swings on the chart.
  const selectedMintSwings = useMemo<SwingLegRecord[] | null>(() => {
    if (!selectedMint) return null;
    if (swingResult && swingResult.mint === selectedMint) return swingResult.swings;
    return swingsByMint.get(selectedMint) ?? null;
  }, [selectedMint, swingResult, swingsByMint]);

  const selectedSwingLeg = useMemo(() => {
    if (!selectedSwingKey || !selectedMintSwings) return null;
    return selectedMintSwings.find((leg) => swingLegKey(leg) === selectedSwingKey) ?? null;
  }, [selectedSwingKey, selectedMintSwings]);

  // Chain-of-swings stats for the selected token, grouped with the single
  // panel's own chain-latency budget (independent of the global "All" panel).
  const singleChainStats = useMemo(
    () =>
      selectedMintSwings ? computeChainStats(selectedMintSwings, singleChainLatencyValue) : null,
    [selectedMintSwings, singleChainLatencyValue],
  );

  // Longest swing chain for the selected token, highlighted on the chart as a
  // band. Driven by the single panel's chain latency so the per-token view is
  // controlled entirely from the single panel.
  const longestChainHighlight = useMemo(
    () => singleChainStats?.longestChain ?? null,
    [singleChainStats],
  );

  const swingTrades = useMemo(() => {
    if (!selectedSwingLeg) return [];
    const lo = Math.min(selectedSwingLeg.start_at, selectedSwingLeg.end_at);
    const hi = Math.max(selectedSwingLeg.start_at, selectedSwingLeg.end_at);
    return trades.filter((t) => {
      const tms = Date.parse(t.block_time);
      return tms >= lo && tms <= hi;
    });
  }, [trades, selectedSwingLeg]);

  const swingTimeLabel = selectedSwingLeg
    ? `${formatTimestampMs(selectedSwingLeg.start_at, timezone)} → ${formatTimestampMs(selectedSwingLeg.end_at, timezone)}`
    : '';

  const filteredSwings = useMemo(() => {
    if (!swingResult) return [];
    return filterSwings(swingResult.swings, appliedSwingFilter);
  }, [swingResult, appliedSwingFilter]);

  const swingOverlay = useMemo(() => {
    if (!selectedMintSwings || !selectedMintSwings.length) return null;
    // The Analysis/Filter panel only applies to the per-token "Run" result; the
    // batch ("Swing Detection All") fallback is shown as a plain connected path.
    const isRunResult = swingResult != null && swingResult.mint === selectedMint;
    const filterActive =
      isRunResult && swingPanelTab === 'filter' && hasActiveSwingFilter(appliedSwingFilter);
    const legs = filterActive ? filteredSwings : selectedMintSwings;
    if (!connectSwings) {
      return { legs, segmentMode: 'perLeg' as const };
    }
    if (filterActive) {
      return {
        legs,
        allLegs: selectedMintSwings,
        segmentMode: 'connectedSequential' as const,
      };
    }
    return { legs, segmentMode: 'connected' as const };
  }, [
    selectedMintSwings,
    swingResult,
    selectedMint,
    swingPanelTab,
    appliedSwingFilter,
    filteredSwings,
    connectSwings,
  ]);

  // Memoized subtrees — each reads this component's scope directly (no props to
  // thread) but only rebuilds when the inputs it actually reads change, so a
  // keystroke in an unrelated field (e.g. chain latency) doesn't re-render the
  // ~300-line "Swing Detection All" panel.
  const globalTokenFiltersButton = useMemo(
    () => (
      <Button
        variant="subtle"
        size="sm"
        active={showFilters || filterCount > 0}
        onClick={() => setShowFilters((v) => !v)}
      >
        {filterCount > 0 ? `Global Filters (${filterCount})` : 'Global Filters'}
      </Button>
    ),
    [showFilters, filterCount],
  );

  const globalTokenFiltersPanel = useMemo(
    () =>
      showFilters && (
        <FilterPanel
          filters={filters}
          onApply={(next) => {
            setFilters(next);
            saveStoredTokenFilters(next);
          }}
          onClear={() => {
            const empty = defaultFilters();
            setFilters(empty);
            saveStoredTokenFilters(empty);
          }}
        />
      ),
    [showFilters, filters],
  );

  const swingDetectionAllButton = useMemo(
    () =>
      loaded && (
        <Button
          variant={showSwingAll ? 'primary' : 'subtle'}
          size="sm"
          active={showSwingAll}
          onClick={() => setShowSwingAll((v) => !v)}
        >
          {swingAllRanCount != null
            ? `Swing Detection All (${swingAllRanCount})`
            : 'Swing Detection All'}
        </Button>
      ),
    [loaded, showSwingAll, swingAllRanCount],
  );

  const swingDetectionAllPanel = useMemo(() => {
    if (!loaded || !showSwingAll) return false;
    const tokenCount = total;
    const runLabel = swingAllLoading
      ? 'Running…'
      : `Run on ${tokenCount} token${tokenCount === 1 ? '' : 's'}`;

    return (
      <div className="w-full mb-3 rounded-lg border border-white/8 bg-bg-card/40 p-4">
        <p className="mb-3 text-[12px] text-text-dim">
          Runs swing detection on all{' '}
          <span className="font-mono text-text">{tokenCount}</span> filtered token
          {tokenCount === 1 ? '' : 's'}, groups each token's swings into chains, and
          fills the Swing Pairs / Max Seq Pairs / Chains columns below.
        </p>

        <Tabs
          value={swingAllTab}
          onValueChange={(v) => setSwingAllTab(v as SwingAllTab)}
          variant="contained"
          className="-mx-4"
        >
          <TabsList className="px-4">
            <TabsTrigger value="analysis">Analysis</TabsTrigger>
            <TabsTrigger value="chain">Chain of Swings</TabsTrigger>
            <TabsTrigger value="timerange">Time Range</TabsTrigger>
            <TabsTrigger value="thresholds">Leg Thresholds</TabsTrigger>
          </TabsList>

          <TabsPanel value="analysis" className="px-4">
            <SwingParamsGrid params={swingParams} onChange={updateSwingParam} />

            <div className="flex flex-wrap items-center gap-3">
              <Button
                variant="ghost"
                className="text-[11px] text-text-dim"
                onClick={() => setSwingParams(DEFAULT_SWING_PARAMS)}
              >
                Reset defaults
              </Button>
            </div>
          </TabsPanel>

          <TabsPanel value="chain" className="px-4">
            <p className="mb-3 text-[11px] text-text-dim">
              Two consecutive high→low pairs stay in the same chain when the idle gap
              between them (one pair's low ending to the next pair's high starting) is
              within this latency. A chain needs at least 2 linked pairs. Changing it
              re-groups instantly — no need to re-run detection.
            </p>
            <div className="mb-4 grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
              <label className={labelClassName}>
                Chain latency (ms)
                <Input
                  fieldSize="md"
                  variant="card"
                  className="min-w-0 font-normal normal-case tracking-normal"
                  type="number"
                  min={0}
                  step={1}
                  value={chainLatencyMs}
                  onChange={(e) => {
                    const parsed = parseInt(e.target.value, 10);
                    setChainLatencyMs(Number.isFinite(parsed) ? parsed : '');
                  }}
                />
              </label>
            </div>

            {swingAllRanCount == null && (
              <p className="text-[11px] text-text-dim">
                Run detection to populate the chain columns.
              </p>
            )}
          </TabsPanel>

          <TabsPanel value="timerange" className="px-4">
            <p className="mb-3 text-[11px] text-text-dim">
              Restrict detection to a window measured in seconds from each token's
              first trade (its launch). Leave a field blank to leave that side
              open. This window applies to every Run on this panel — re-run
              detection after changing it.
            </p>
            <label className="mb-4 flex items-center gap-2 text-[12px] text-text">
              <Checkbox
                checked={curveOnly}
                onChange={(e) => setCurveOnly(e.target.checked)}
              />
              Token create → migration (bonding-curve trades only)
              <span className="text-[11px] text-text-dim">
                — ignores the seconds window below
              </span>
            </label>
            <div className="mb-4 grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
              <RangeInputs
                label="Window (s after launch)"
                left={{
                  value: windowStartSec,
                  placeholder: 'start',
                  disabled: curveOnly,
                  onChange: (raw) => {
                    const parsed = parseFloat(raw);
                    setWindowStartSec(Number.isFinite(parsed) ? parsed : '');
                  },
                }}
                right={{
                  value: windowEndSec,
                  placeholder: 'end',
                  disabled: curveOnly,
                  onChange: (raw) => {
                    const parsed = parseFloat(raw);
                    setWindowEndSec(Number.isFinite(parsed) ? parsed : '');
                  },
                }}
              />
            </div>

            <div className="flex flex-wrap items-center gap-3">
              <Button
                variant="ghost"
                className="text-[11px] text-text-dim"
                onClick={() => {
                  setWindowStartSec('');
                  setWindowEndSec('');
                }}
              >
                Clear range
              </Button>
            </div>
          </TabsPanel>

          <TabsPanel value="thresholds" className="px-4">
            <p className="mb-3 text-[11px] text-text-dim">
              Bound each detected leg by its price change (delta %) and net flow
              rate (SOL per second), compared by magnitude — enter positive
              values (e.g. 30 keeps swing lows that drop ≥ 30%). 0 = ignore that
              bound. Applies during detection — re-run after changing.
            </p>
            <div className="mb-4 grid gap-6 sm:grid-cols-2">
              <div>
                <h4 className="mb-3 text-[12px] font-bold text-text">Swing High</h4>
                <div className="grid gap-3 sm:grid-cols-2">
                  <SwingRangeField
                    label="Delta (%)"
                    left={{ key: 'swing_high_min_delta_pct', placeholder: 'min' }}
                    right={{ key: 'swing_high_max_delta_pct', placeholder: 'max' }}
                    params={swingParams}
                    onChange={updateSwingParam}
                  />
                  <SwingRangeField
                    label="Velocity (NetFlowSOL / Second)"
                    left={{ key: 'swing_high_min_net_flow_per_sec', placeholder: 'min' }}
                    right={{ key: 'swing_high_max_net_flow_per_sec', placeholder: 'max' }}
                    params={swingParams}
                    onChange={updateSwingParam}
                  />
                </div>
              </div>
              <div>
                <h4 className="mb-3 text-[12px] font-bold text-text">Swing Low</h4>
                <div className="grid gap-3 sm:grid-cols-2">
                  <SwingRangeField
                    label="Delta (%)"
                    left={{ key: 'swing_low_min_delta_pct', placeholder: 'min' }}
                    right={{ key: 'swing_low_max_delta_pct', placeholder: 'max' }}
                    params={swingParams}
                    onChange={updateSwingParam}
                  />
                  <SwingRangeField
                    label="Velocity (NetFlowSOL / Second)"
                    left={{ key: 'swing_low_min_net_flow_per_sec', placeholder: 'min' }}
                    right={{ key: 'swing_low_max_net_flow_per_sec', placeholder: 'max' }}
                    params={swingParams}
                    onChange={updateSwingParam}
                  />
                </div>
              </div>
            </div>

            <div className="flex flex-wrap items-center gap-3">
              <Button
                variant="ghost"
                className="text-[11px] text-text-dim"
                onClick={() =>
                  setSwingParams((prev) => ({
                    ...prev,
                    swing_high_min_delta_pct: 0,
                    swing_high_max_delta_pct: 0,
                    swing_high_min_net_flow_per_sec: 0,
                    swing_high_max_net_flow_per_sec: 0,
                    swing_low_min_delta_pct: 0,
                    swing_low_max_delta_pct: 0,
                    swing_low_min_net_flow_per_sec: 0,
                    swing_low_max_net_flow_per_sec: 0,
                  }))
                }
              >
                Clear thresholds
              </Button>
            </div>
          </TabsPanel>
        </Tabs>

        <div className="mt-4 flex flex-wrap items-center gap-3 border-t border-white/8 pt-4">
          <Button
            variant="primary"
            disabled={tokenCount === 0 || swingAllLoading}
            onClick={handleRunAllSwings}
          >
            {runLabel}
          </Button>
        </div>

        {swingAllError && <p className="mt-4 text-sm text-red">{swingAllError}</p>}
        {swingAllLoading && (
          <p className="mt-4 text-[12px] text-text-dim">
            {swingAllProgress
              ? `Running swing detection… ${swingAllProgress.done} / ${swingAllProgress.total} tokens`
              : `Running swing detection on ${tokenCount} tokens…`}
          </p>
        )}
      </div>
    );
  }, [
    loaded,
    showSwingAll,
    total,
    swingAllTab,
    swingParams,
    updateSwingParam,
    chainLatencyMs,
    swingAllRanCount,
    curveOnly,
    windowStartSec,
    windowEndSec,
    handleRunAllSwings,
    swingAllLoading,
    swingAllError,
    swingAllProgress,
  ]);

  const controlsBar = useMemo(
    () => (
      <div className="mb-3 flex flex-wrap items-end gap-3">
        <label className="flex flex-col gap-1 text-[10px] font-bold uppercase tracking-widest text-text-dim">
          Created from
          <Input
            type="datetime-local"
            fieldSize="md"
            variant="card"
            value={createdFrom}
            onChange={(e) => dispatch(setCreatedFrom(e.target.value))}
            className="min-w-0 font-normal normal-case tracking-normal"
          />
        </label>
        <label className="flex flex-col gap-1 text-[10px] font-bold uppercase tracking-widest text-text-dim">
          Created to
          <Input
            type="datetime-local"
            fieldSize="md"
            variant="card"
            value={createdTo}
            onChange={(e) => dispatch(setCreatedTo(e.target.value))}
            className="min-w-0 font-normal normal-case tracking-normal"
          />
        </label>
        <Button variant="primary" onClick={handleRefresh} disabled={loading}>
          {loading ? 'Refreshing…' : 'Fetch'}
        </Button>
        {(createdFrom || createdTo) && (
          <Button variant="ghost" onClick={() => dispatch(clearCreatedRange())}>
            Clear
          </Button>
        )}
        <span className="flex-1" />
        {swingDetectionAllButton}
        {globalTokenFiltersButton}
        {swingDetectionAllPanel}
        {globalTokenFiltersPanel}
      </div>
    ),
    [
      createdFrom,
      createdTo,
      handleRefresh,
      loading,
      dispatch,
      swingDetectionAllButton,
      globalTokenFiltersButton,
      swingDetectionAllPanel,
      globalTokenFiltersPanel,
    ],
  );

  return (
    <div>
      <div className="mb-3.5 flex flex-wrap items-center gap-3">
        <h2 className="text-lg font-extrabold text-text">Swing detection</h2>
        {loaded && (
          <Badge variant="primary" className="font-mono">
            {total} tokens
          </Badge>
        )}
      </div>

      <SectionDivider />

      {controlsBar}

      {error && <p className="mb-2 text-sm text-red">{error}</p>}

      {!loaded && !loading && !error && (
        <p className="mb-3 text-sm text-text-dim">Click Fetch to load tokens.</p>
      )}

      <SectionDivider />


      {loaded && (
        <DataTable
          columns={columns}
          rows={pageTokens}
          rowKey={(r) => r.mint_address}
          selectedKey={selectedMint}
          onSelect={handleSelectMint}
          serverSide
          serverTotal={total}
          onQueryChange={setTableQuery}
          loading={loading}
          resetKey={filtersResetKey}
          searchable
          colFilters
          colToggle
          hoverable
          tableId="swing"
          emptyMessage="No tokens in this date range."
        />
      )}

      <SectionDivider />

      <section className="mb-1">
        <h3 className="mb-3 text-sm font-bold text-text">Analysis</h3>
        <div className="flex flex-wrap gap-2">
          <Button
            variant={activeAnalysis === 'swing' ? 'primary' : 'ghost'}
            onClick={() => toggleAnalysis('swing')}
          >
            Swing detection
          </Button>
        </div>

        {activeAnalysis === 'swing' && (
          <div className="mt-4 rounded-lg border border-white/8 bg-bg-card/40 p-4">
            <p className="mb-3 text-[12px] text-text-dim">
              Token:{' '}
              <span className="font-mono text-text">
                {selectedMint
                  ? chartSymbol
                    ? `${chartSymbol} - ${selectedMint}`
                    : selectedMint
                  : 'Select a token from the table above'}
              </span>
            </p>

            <Tabs
              value={swingPanelTab}
              onValueChange={(v) => setSwingPanelTab(v as SwingPanelTab)}
              variant="contained"
              className="-mx-4"
            >
              <TabsList className="px-4">
                <TabsTrigger value="analysis">Analysis</TabsTrigger>
                <TabsTrigger value="chain">Chain of Swings</TabsTrigger>
                <TabsTrigger value="timerange">Time Range</TabsTrigger>
                <TabsTrigger value="thresholds">Leg Thresholds</TabsTrigger>
                <TabsTrigger value="filter">Filter</TabsTrigger>
              </TabsList>

              <TabsPanel value="analysis" className="px-4">
                <SwingParamsGrid params={singleSwingParams} onChange={updateSingleSwingParam} />

                <div className="flex flex-wrap items-center gap-3">
                  <Button
                    variant="ghost"
                    className="text-[11px] text-text-dim"
                    onClick={() => setSingleSwingParams(DEFAULT_SWING_PARAMS)}
                  >
                    Reset defaults
                  </Button>
                </div>
              </TabsPanel>

              <TabsPanel value="chain" className="px-4">
                <p className="mb-3 text-[11px] text-text-dim">
                  Two consecutive high→low pairs stay in the same chain when the idle gap
                  between them (one pair's low ending to the next pair's high starting) is
                  within this latency. A chain needs at least 2 linked pairs. Changing it
                  re-groups instantly — no need to re-run detection.
                </p>
                <div className="mb-4 grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
                  <label className={labelClassName}>
                    Chain latency (ms)
                    <Input
                      fieldSize="md"
                      variant="card"
                      className="min-w-0 font-normal normal-case tracking-normal"
                      type="number"
                      min={0}
                      step={1}
                      value={singleChainLatencyMs}
                      onChange={(e) => {
                        const parsed = parseInt(e.target.value, 10);
                        setSingleChainLatencyMs(Number.isFinite(parsed) ? parsed : '');
                      }}
                    />
                  </label>
                </div>

                {!swingResult ? (
                  <p className="text-[11px] text-text-dim">
                    Run detection to populate the chain stats.
                  </p>
                ) : (
                  singleChainStats && (
                    <div className="flex flex-wrap items-center gap-2">
                      <Badge variant="primary" className="font-mono font-normal">
                        {singleChainStats.totalPairCount} pair
                        {singleChainStats.totalPairCount === 1 ? '' : 's'}
                      </Badge>
                      <Badge variant="primary" className="font-mono font-normal">
                        max seq {singleChainStats.maxSequentialPairCount}
                      </Badge>
                      <Badge variant="primary" className="font-mono font-normal">
                        {singleChainStats.chainCount} chain
                        {singleChainStats.chainCount === 1 ? '' : 's'}
                      </Badge>
                    </div>
                  )
                )}
              </TabsPanel>

              <TabsPanel value="timerange" className="px-4">
                <p className="mb-3 text-[11px] text-text-dim">
                  Restrict detection to a window measured in seconds from the token's
                  first trade (its launch). Leave a field blank to leave that side open.
                  Applies during detection — re-run after changing it.
                </p>
                <label className="mb-4 flex items-center gap-2 text-[12px] text-text">
                  <Checkbox
                    checked={singleCurveOnly}
                    onChange={(e) => setSingleCurveOnly(e.target.checked)}
                  />
                  Token create → migration (bonding-curve trades only)
                  <span className="text-[11px] text-text-dim">
                    — ignores the seconds window below
                  </span>
                </label>
                <div className="mb-4 grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
                  <RangeInputs
                    label="Window (s after launch)"
                    left={{
                      value: singleWindowStartSec,
                      placeholder: 'start',
                      disabled: singleCurveOnly,
                      onChange: (raw) => {
                        const parsed = parseFloat(raw);
                        setSingleWindowStartSec(Number.isFinite(parsed) ? parsed : '');
                      },
                    }}
                    right={{
                      value: singleWindowEndSec,
                      placeholder: 'end',
                      disabled: singleCurveOnly,
                      onChange: (raw) => {
                        const parsed = parseFloat(raw);
                        setSingleWindowEndSec(Number.isFinite(parsed) ? parsed : '');
                      },
                    }}
                  />
                </div>

                <div className="flex flex-wrap items-center gap-3">
                  <Button
                    variant="ghost"
                    className="text-[11px] text-text-dim"
                    onClick={() => {
                      setSingleWindowStartSec('');
                      setSingleWindowEndSec('');
                    }}
                  >
                    Clear range
                  </Button>
                </div>
              </TabsPanel>

              <TabsPanel value="thresholds" className="px-4">
                <p className="mb-3 text-[11px] text-text-dim">
                  Bound each detected leg by its price change (delta %) and net flow rate
                  (SOL per second), compared by magnitude — enter positive values (e.g. 30
                  keeps swing lows that drop ≥ 30%). 0 = ignore that bound. Applies during
                  detection — re-run after changing.
                </p>
                <div className="mb-4 grid gap-6 sm:grid-cols-2">
                  <div>
                    <h4 className="mb-3 text-[12px] font-bold text-text">Swing High</h4>
                    <div className="grid gap-3 sm:grid-cols-2">
                      <SwingRangeField
                        label="Delta (%)"
                        left={{ key: 'swing_high_min_delta_pct', placeholder: 'min' }}
                        right={{ key: 'swing_high_max_delta_pct', placeholder: 'max' }}
                        params={singleSwingParams}
                        onChange={updateSingleSwingParam}
                      />
                      <SwingRangeField
                        label="Velocity (NetFlowSOL / Second)"
                        left={{ key: 'swing_high_min_net_flow_per_sec', placeholder: 'min' }}
                        right={{ key: 'swing_high_max_net_flow_per_sec', placeholder: 'max' }}
                        params={singleSwingParams}
                        onChange={updateSingleSwingParam}
                      />
                    </div>
                  </div>
                  <div>
                    <h4 className="mb-3 text-[12px] font-bold text-text">Swing Low</h4>
                    <div className="grid gap-3 sm:grid-cols-2">
                      <SwingRangeField
                        label="Delta (%)"
                        left={{ key: 'swing_low_min_delta_pct', placeholder: 'min' }}
                        right={{ key: 'swing_low_max_delta_pct', placeholder: 'max' }}
                        params={singleSwingParams}
                        onChange={updateSingleSwingParam}
                      />
                      <SwingRangeField
                        label="Velocity (NetFlowSOL / Second)"
                        left={{ key: 'swing_low_min_net_flow_per_sec', placeholder: 'min' }}
                        right={{ key: 'swing_low_max_net_flow_per_sec', placeholder: 'max' }}
                        params={singleSwingParams}
                        onChange={updateSingleSwingParam}
                      />
                    </div>
                  </div>
                </div>

                <div className="flex flex-wrap items-center gap-3">
                  <Button
                    variant="ghost"
                    className="text-[11px] text-text-dim"
                    onClick={() =>
                      setSingleSwingParams((prev) => ({
                        ...prev,
                        swing_high_min_delta_pct: 0,
                        swing_high_max_delta_pct: 0,
                        swing_high_min_net_flow_per_sec: 0,
                        swing_high_max_net_flow_per_sec: 0,
                        swing_low_min_delta_pct: 0,
                        swing_low_max_delta_pct: 0,
                        swing_low_min_net_flow_per_sec: 0,
                        swing_low_max_net_flow_per_sec: 0,
                      }))
                    }
                  >
                    Clear thresholds
                  </Button>
                </div>
              </TabsPanel>

              <TabsPanel value="filter" className="px-4">
                {!swingResult && !swingLoading && (
                  <p className="rounded-md border border-dashed border-white/10 px-3 py-6 text-center text-[12px] text-text-dim">
                    Run swing detection first.
                  </p>
                )}
                {swingResult && (
                  <>
                    <p className="mb-3 text-[11px] text-text-dim">
                      Narrow detected legs for display only — does not re-run detection. 0 =
                      ignore that bound.
                    </p>
                    <div className="mb-4 grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
                      <label className={labelClassName}>
                        Filter leg type
                        <Select
                          fieldSize="md"
                          variant="card"
                          className="min-w-0 font-normal normal-case tracking-normal"
                          value={swingFilter.leg_type}
                          onChange={(e) => updateSwingFilter('leg_type', e.target.value)}
                        >
                          <option value="all">All</option>
                          <option value="swing_high">Swing high</option>
                          <option value="swing_low">Swing low</option>
                        </Select>
                      </label>
                      <RangeInputs
                        label="Filter duration (ms)"
                        left={{
                          value: swingFilter.filter_min_duration_ms,
                          placeholder: 'min',
                          step: 1,
                          onChange: (raw) => updateSwingFilter('filter_min_duration_ms', raw),
                        }}
                        right={{
                          value: swingFilter.filter_max_duration_ms,
                          placeholder: 'max',
                          step: 1,
                          onChange: (raw) => updateSwingFilter('filter_max_duration_ms', raw),
                        }}
                      />
                      <RangeInputs
                        label="Filter trades"
                        left={{
                          value: swingFilter.filter_min_trades,
                          placeholder: 'min',
                          step: 1,
                          onChange: (raw) => updateSwingFilter('filter_min_trades', raw),
                        }}
                        right={{
                          value: swingFilter.filter_max_trades,
                          placeholder: 'max',
                          step: 1,
                          onChange: (raw) => updateSwingFilter('filter_max_trades', raw),
                        }}
                      />
                      <RangeInputs
                        label="Filter volume (SOL)"
                        left={{
                          value: swingFilter.filter_min_volume_sol,
                          placeholder: 'min',
                          onChange: (raw) => updateSwingFilter('filter_min_volume_sol', raw),
                        }}
                        right={{
                          value: swingFilter.filter_max_volume_sol,
                          placeholder: 'max',
                          onChange: (raw) => updateSwingFilter('filter_max_volume_sol', raw),
                        }}
                      />
                      <RangeInputs
                        label="Filter net flow (SOL)"
                        left={{
                          value: swingFilter.filter_min_net_flow_sol,
                          placeholder: 'min',
                          min: null,
                          onChange: (raw) => updateSwingFilter('filter_min_net_flow_sol', raw),
                        }}
                        right={{
                          value: swingFilter.filter_max_net_flow_sol,
                          placeholder: 'max',
                          min: null,
                          onChange: (raw) => updateSwingFilter('filter_max_net_flow_sol', raw),
                        }}
                      />
                      <RangeInputs
                        label="Filter change (%)"
                        left={{
                          value: swingFilter.filter_min_change_pct,
                          placeholder: 'min',
                          min: null,
                          onChange: (raw) => updateSwingFilter('filter_min_change_pct', raw),
                        }}
                        right={{
                          value: swingFilter.filter_max_change_pct,
                          placeholder: 'max',
                          min: null,
                          onChange: (raw) => updateSwingFilter('filter_max_change_pct', raw),
                        }}
                      />
                    </div>

                    <div className="mb-4 flex flex-wrap items-center gap-3">
                      <Button
                        variant="primary"
                        onClick={() => setAppliedSwingFilter(swingFilterFromForm(swingFilter))}
                      >
                        Apply
                      </Button>
                      <Button
                        variant="ghost"
                        className="text-[11px] text-text-dim"
                        onClick={() => {
                          setSwingFilter(DEFAULT_SWING_FILTER);
                          setAppliedSwingFilter(DEFAULT_SWING_FILTER);
                        }}
                      >
                        Clear filters
                      </Button>
                      {!hasActiveSwingFilter(swingFilterFromForm(swingFilter)) && (
                        <span className="text-[11px] text-text-dim">
                          Set filter criteria and click Apply (0 = ignore that field).
                        </span>
                      )}
                    </div>

                    <div className="mb-2 flex flex-wrap items-center gap-2">
                      <span className="text-[12px] font-bold text-text">Filtered</span>
                      <Badge variant="primary" className="font-mono font-normal">
                        {filteredSwings.length} / {swingResult.count} swing
                        {swingResult.count === 1 ? '' : 's'}
                      </Badge>
                      <span className="flex-1" />
                      <VisibilityToggleButton
                        visible={showSwingResultsTable}
                        onToggle={() => setShowSwingResultsTable((v) => !v)}
                        label="filtered swing results table"
                      />
                    </div>
                    {showSwingResultsTable && (
                      <DataTable
                        tableId="swing_filtered"
                        columns={swingTableColumns}
                        rows={filteredSwings}
                        rowKey={swingLegKey}
                        selectedKey={selectedSwingKey}
                        onSelect={handleSwingSelect}
                        defaultPageSize={5}
                        searchable
                        colFilters
                        hoverable
                        emptyMessage={
                          hasActiveSwingFilter(appliedSwingFilter)
                            ? 'No swings match these filter criteria.'
                            : 'Set filter criteria and click Apply.'
                        }
                      />
                    )}
                  </>
                )}
              </TabsPanel>
            </Tabs>

            {/* Run + results are shared across the config tabs (the Filter tab
                renders its own filtered table). */}
            {swingPanelTab !== 'filter' && (
              <div className="mt-4 border-t border-white/8 pt-4">
                <div className="flex flex-wrap items-center gap-3">
                  <Button
                    variant="primary"
                    disabled={!selectedMint || swingLoading}
                    onClick={handleRunSwing}
                  >
                    {swingLoading ? 'Running…' : 'Run'}
                  </Button>
                </div>

                {swingError && <p className="mt-4 text-sm text-red">{swingError}</p>}

                <div className="mt-4">
                  {swingLoading && (
                    <p className="text-[12px] text-text-dim">Running swing detection…</p>
                  )}
                  {!swingLoading && !swingError && !swingResult && (
                    <p className="rounded-md border border-dashed border-white/10 px-3 py-6 text-center text-[12px] text-text-dim">
                      Click Run to detect swings for the selected token.
                    </p>
                  )}
                  {swingResult && !swingLoading && (
                    <>
                      <div className="mb-2 flex flex-wrap items-center gap-2">
                        <span className="text-[12px] font-bold text-text">Results</span>
                        <Badge variant="primary" className="font-mono font-normal">
                          {swingResult.count} swing{swingResult.count === 1 ? '' : 's'}
                        </Badge>
                        <span className="flex-1" />
                        <VisibilityToggleButton
                          visible={showSwingResultsTable}
                          onToggle={() => setShowSwingResultsTable((v) => !v)}
                          label="swing results table"
                        />
                      </div>
                      {showSwingResultsTable && (
                        <DataTable
                          tableId="swing_results"
                          columns={swingTableColumns}
                          rows={swingResult.swings}
                          rowKey={swingLegKey}
                          selectedKey={selectedSwingKey}
                          onSelect={handleSwingSelect}
                          defaultPageSize={5}
                          searchable
                          colFilters
                          hoverable
                          emptyMessage="No swings detected with these parameters."
                        />
                      )}
                    </>
                  )}
                </div>
              </div>
            )}
          </div>
        )}
      </section>

      <SectionDivider />

      <div>
        <TokenPriceChart
          symbol={chartSymbol}
          id={selectedMint ?? ''}
          trades={trades}
          loading={tradesLoading}
          error={tradesError}
          toValue={toChartValue}
          priceLabel={chartPriceLabel}
          priceUnit={unit}
          metric={chartMetric}
          onMetricChange={setChartMetric}
          onBarClick={handleBarClick}
          selectedBar={selectedBar}
          swingOverlay={swingOverlay}
          highlightChain={longestChainHighlight}
          selectedSwingLegKey={selectedSwingKey}
          onSwingLegClick={handleSwingLegClick}
          connectSwings={connectSwings}
          onConnectSwingsChange={setConnectSwings}
          athPriceInSol={selectedToken?.ath_price ?? null}
          isMigrated={selectedToken?.is_migrated}
          isMayhemMode={selectedToken?.is_mayhem_mode}
          isCashbackEnabled={selectedToken?.is_cashback_enabled}
          profileWallets={profileWallets}
          tokenCreatedAt={selectedToken?.created_at}
        />
      </div>

      {(selectedBar || selectedSwingKey) && selectedMint && (
        <>
          <SectionDivider />
          <div>
            <div className="mb-2 flex flex-wrap items-center gap-2">
              <h3 className="text-sm font-bold text-text">
                {selectedSwingKey ? 'Swing trades' : 'Bar trades'}
              </h3>
              <span className="font-mono text-[11px] text-text-dim">
                {selectedSwingKey ? swingTimeLabel : barTimeLabel}
              </span>
              <Badge variant="primary" className="font-mono font-normal">
                {(selectedSwingKey ? swingTrades : barTrades).length} trade
                {(selectedSwingKey ? swingTrades : barTrades).length === 1 ? '' : 's'}
              </Badge>
              <button
                type="button"
                onClick={() => {
                  setSelectedBar(null);
                  dispatch(setSelectedSwingKey(null));
                }}
                className="text-[11px] text-text-dim hover:text-text"
              >
                Clear
              </button>
            </div>
            <DataTable
              tableId="swing_trades"
              columns={tradeTableColumns}
              rows={selectedSwingKey ? swingTrades : barTrades}
              rowKey={(t) => t.id}
              defaultPageSize={25}
              searchable
              colFilters
              hoverable
              emptyMessage={
                selectedSwingKey ? 'No trades in this swing.' : 'No trades in this bar.'
              }
            />
          </div>
        </>
      )}
    </div>
  );
}
