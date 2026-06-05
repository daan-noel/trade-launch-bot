import { useCallback, useEffect, useMemo, useState } from 'react';
import { useDispatch, useSelector } from 'react-redux';
import { DataTable } from '../../components/table/DataTable';
import { tokenTradeColumns } from '../../components/transactions/tokenTradeColumns';
import {
  TokenPriceChart,
  WALLET_MARKER_COLORS,
  tradeBarTime,
  type ChartBarSelection,
  type ChartMetric,
  type ChartSwingLeg,
  type ProfileWalletInfo,
} from '../../components/token-price-chart';
import { swingLegKey } from '../../components/token-price-chart/swingOverlay';
import { FilterPanel } from '../../components/tokens/FilterPanel';
import {
  activeFilterCount,
  defaultFilters,
  filtersEmpty,
  loadStoredTokenFilters,
  saveStoredTokenFilters,
  tokenPassesFilters,
  type TokenFilters,
} from '../../components/tokens/filters';
import { tokenColumns } from '../../components/tokens/tokenColumns';
import { usePriceUnit } from '../../context/PriceUnitContext';
import { useTimezone } from '../../context/TimezoneContext';
import { formatTimestampMs } from '../../utils/date';
import { usePriceDisplay } from '../../hooks/usePriceDisplay';
import { swingColumns } from '../../components/analysis/swingColumns';
import {
  DEFAULT_SWING_PARAMS,
  SWING_PARAM_INT_KEYS,
  SwingParamsGrid,
  isFiniteNumber,
  mergeSwingParams,
  swingParamsFromForm,
  swingParamLabelClassName as labelClassName,
  type SwingParamsForm,
} from '../../components/analysis/swingParams';
import { computeChainStats, type SwingChainStats } from '../../components/analysis/swingChains';
import { swingChainColumns } from '../../components/analysis/swingChainColumns';
import {
  DEFAULT_SWING_FILTER,
  filterSwings,
  hasActiveSwingFilter,
  parseSwingFilterField,
  swingFilterFromForm,
  type SwingFilterCriteria,
  type SwingFilterForm,
} from '../../components/analysis/swingFilter';
import { Badge } from '../../components/ui/Badge';
import { Button } from '../../components/ui/Button';
import { Input } from '../../components/ui/Input';
import { Select } from '../../components/ui/Select';
import { Tabs, TabsList, TabsPanel, TabsTrigger } from '../../components/ui/Tabs';
import { VisibilityToggleButton } from '../../components/ui/VisibilityToggleButton';
import { fetchProfiles, fetchTokenSwings, fetchTokenSwingsBatch } from '../../services/api';
import {
  apiErrorMessage,
  useGetTokenTradesQuery,
  useGetTokensQuery,
} from '../../store/apiSlice';
import type { AppDispatch, RootState } from '../../store';
import {
  clearCreatedRange,
  setCreatedFrom,
  setCreatedTo,
  setSelectedMint,
  setSelectedSwingKey,
  setSwingAllResults,
  setSwingResult,
  setTokensFetched,
} from '../../store/swingDetectionSlice';
import type {
  SwingLegRecord,
  SwingParams,
  TokenRecord,
  TradeRecord,
  WalletProfile,
} from '../../types';

/** Stable empty references so derived memos don't recompute every render. */
const EMPTY_TOKENS: TokenRecord[] = [];
const EMPTY_TRADES: TradeRecord[] = [];

function filterByCreatedRange(
  tokens: TokenRecord[],
  from: string,
  to: string,
): TokenRecord[] {
  const fromMs = from ? Date.parse(from) : NaN;
  const toMs = to ? Date.parse(to) : NaN;
  return tokens.filter((t) => {
    const created = Date.parse(t.created_at);
    if (Number.isNaN(created)) return true;
    if (!Number.isNaN(fromMs) && created < fromMs) return false;
    if (!Number.isNaN(toMs) && created > toMs) return false;
    return true;
  });
}

function filterDisplayedTokens(
  tokens: TokenRecord[],
  createdFrom: string,
  createdTo: string,
  filters: TokenFilters,
): TokenRecord[] {
  let rows = filterByCreatedRange(tokens, createdFrom, createdTo);
  if (!filtersEmpty(filters)) {
    rows = rows.filter((t) => tokenPassesFilters(filters, t));
  }
  return rows;
}

function SectionDivider() {
  return <div role="separator" className="my-6 border-t border-white/6" />;
}

type AnalysisKind = 'swing';
type SwingPanelTab = 'analysis' | 'filter';
type SwingAllTab = 'analysis' | 'chain';

const LS_SWING_DETECTION_KEY = 'swing_detection_criteria';

/** Default max idle gap (ms) for two swings to count as the same chain. */
const DEFAULT_CHAIN_LATENCY_MS = 60_000;

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

function loadStoredSwingCriteria(): {
  params: SwingParams;
  filter: SwingFilterCriteria;
  appliedFilter: SwingFilterCriteria;
  connectSwings: boolean;
  chainLatencyMs: number;
} {
  try {
    const raw = localStorage.getItem(LS_SWING_DETECTION_KEY);
    if (!raw) {
      return {
        params: DEFAULT_SWING_PARAMS,
        filter: DEFAULT_SWING_FILTER,
        appliedFilter: DEFAULT_SWING_FILTER,
        connectSwings: true,
        chainLatencyMs: DEFAULT_CHAIN_LATENCY_MS,
      };
    }
    const parsed = JSON.parse(raw) as {
      params?: Partial<SwingParams>;
      filter?: Partial<SwingFilterCriteria>;
      appliedFilter?: Partial<SwingFilterCriteria>;
      connectSwings?: boolean;
      chainLatencyMs?: number;
    };
    return {
      params: mergeSwingParams(parsed.params),
      filter: mergeSwingFilter(parsed.filter),
      appliedFilter: mergeSwingFilter(parsed.appliedFilter ?? parsed.filter),
      connectSwings: parsed.connectSwings !== false,
      chainLatencyMs: isFiniteNumber(parsed.chainLatencyMs)
        ? parsed.chainLatencyMs
        : DEFAULT_CHAIN_LATENCY_MS,
    };
  } catch {
    return {
      params: DEFAULT_SWING_PARAMS,
      filter: DEFAULT_SWING_FILTER,
      appliedFilter: DEFAULT_SWING_FILTER,
      connectSwings: true,
      chainLatencyMs: DEFAULT_CHAIN_LATENCY_MS,
    };
  }
}

function saveStoredSwingCriteria(
  params: SwingParams,
  filter: SwingFilterCriteria,
  appliedFilter: SwingFilterCriteria,
  connectSwings: boolean,
  chainLatencyMs: number,
): void {
  try {
    localStorage.setItem(
      LS_SWING_DETECTION_KEY,
      JSON.stringify({ params, filter, appliedFilter, connectSwings, chainLatencyMs }),
    );
  } catch {
    /* ignore */
  }
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

  const price = usePriceDisplay();
  const { unit, usdRate } = usePriceUnit();
  const { timezone } = useTimezone();
  const tradeTableColumns = useMemo(() => tokenTradeColumns(price), [price]);
  const swingTableColumns = useMemo(() => swingColumns(price, timezone), [price, timezone]);

  const toChartValue = useCallback(
    (sol: number) => (unit === 'USD' && usdRate != null ? sol * usdRate : sol),
    [unit, usdRate],
  );

  // Lazily loaded on the "Fetch" button, but shares TokensPage's cache key —
  // if that page already loaded the list, flipping `skip` off is instant. The
  // `tokensFetched` flag lives in Redux so the table re-appears on return.
  const {
    data: tokensData,
    isFetching: loading,
    error: tokensError,
    refetch: refetchTokens,
  } = useGetTokensQuery(
    { search: '', limit: 5000, offset: 0 },
    { skip: !tokensFetched },
  );
  const tokens = tokensData?.items ?? EMPTY_TOKENS;
  const total = tokensData?.total ?? 0;
  const loaded = tokensData !== undefined;
  const error = apiErrorMessage(tokensError, 'Failed to load tokens');
  const [profiles, setProfiles] = useState<WalletProfile[]>([]);

  useEffect(() => {
    fetchProfiles().then(setProfiles).catch(() => { });
  }, []);

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

  const [showFilters, setShowFilters] = useState(false);
  const [filters, setFilters] = useState<TokenFilters>(loadStoredTokenFilters);

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

  // "Swing Detection All" — global detection across every filtered token. Raw
  // swings per mint are kept so tuning the chain latency re-groups instantly
  // without re-fetching.
  const [showSwingAll, setShowSwingAll] = useState(false);
  const [swingAllTab, setSwingAllTab] = useState<SwingAllTab>('analysis');
  const [chainLatencyMs, setChainLatencyMs] = useState<number | ''>(
    storedSwingCriteria.chainLatencyMs,
  );
  const [swingAllLoading, setSwingAllLoading] = useState(false);
  const [swingAllError, setSwingAllError] = useState<string | null>(null);

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

  const chainLatencyValue = typeof chainLatencyMs === 'number' ? chainLatencyMs : 0;

  const chainStatsByMint = useMemo(() => {
    const map = new Map<string, SwingChainStats>();
    for (const [mint, swings] of swingsByMint) {
      map.set(mint, computeChainStats(swings, chainLatencyValue));
    }
    return map;
  }, [swingsByMint, chainLatencyValue]);

  const columns = useMemo(
    () => [...tokenColumns(price), ...swingChainColumns(chainStatsByMint)],
    [price, chainStatsByMint],
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
      const result = await fetchTokenSwings(selectedMint, swingParamsFromForm(swingParams));
      dispatch(setSwingResult(result));
    } catch (e) {
      dispatch(setSwingResult(null));
      setSwingError(e instanceof Error ? e.message : 'Swing detection failed');
    } finally {
      setSwingLoading(false);
    }
  }, [selectedMint, swingParams, dispatch]);

  const displayed = useMemo(
    () => filterDisplayedTokens(tokens, createdFrom, createdTo, filters),
    [tokens, createdFrom, createdTo, filters],
  );

  // Run detection across all currently-filtered tokens in one batch request,
  // then keep the raw swings per mint for client-side chain grouping.
  const handleRunAllSwings = useCallback(async () => {
    const mints = displayed.map((t) => t.mint_address);
    if (mints.length === 0) return;
    setSwingAllLoading(true);
    setSwingAllError(null);
    try {
      const resp = await fetchTokenSwingsBatch(mints, swingParamsFromForm(swingParams));
      dispatch(setSwingAllResults(resp.results));
    } catch (e) {
      setSwingAllError(e instanceof Error ? e.message : 'Swing detection failed');
    } finally {
      setSwingAllLoading(false);
    }
  }, [displayed, swingParams, dispatch]);

  const filterCount = activeFilterCount(filters);

  const selectedToken = useMemo(
    () => displayed.find((t) => t.mint_address === selectedMint) ?? null,
    [displayed, selectedMint],
  );

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
    saveStoredSwingCriteria(
      swingParamsFromForm(swingParams),
      swingFilterFromForm(swingFilter),
      appliedSwingFilter,
      connectSwings,
      chainLatencyValue,
    );
  }, [swingParams, swingFilter, appliedSwingFilter, connectSwings, chainLatencyValue]);

  // Reset local selection-derived UI when the chosen token changes. The Redux
  // swing result and leg selection are cleared by the setSelectedMint reducer;
  // trades themselves are fetched/cached by the useGetTokenTradesQuery hook.
  useEffect(() => {
    setSelectedBar(null);
    setSwingError(null);
  }, [selectedMint]);

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

  // A selection that scrolls out of the date range is cleared — but only once
  // the list has loaded, so the persisted selection survives the initial fetch
  // after navigating back (when `displayed` is briefly empty).
  useEffect(() => {
    if (!selectedMint || !loaded) return;
    if (!displayed.some((t) => t.mint_address === selectedMint)) {
      dispatch(setSelectedMint(null));
    }
  }, [displayed, selectedMint, loaded, dispatch]);

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

  // Inline element generators — plain functions that read this component's
  // scope directly, so there are no props to thread through.
  function renderControlsBar() {
    return (
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
        {renderSwingDetectionAllButton()}
        {renderGobalTokenFiltersButton()}
        {renderSwingDetectionAllPanel()}
        {renderGlobalTokenFiltersPanel()}
      </div>
    );
  }

  function renderGobalTokenFiltersButton() {
    return (
      <Button
        variant="subtle"
        size="sm"
        active={showFilters || filterCount > 0}
        onClick={() => setShowFilters((v) => !v)}
      >
        {filterCount > 0 ? `Global Filters (${filterCount})` : 'Global Filters'}
      </Button>
    );
  }
  function renderGlobalTokenFiltersPanel() {
    return (
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
      )
    );
  }

  function renderSwingDetectionAllButton() {
    return (
      loaded && (
        <>
          <Button
            variant={showSwingAll ? 'primary' : 'subtle'}
            size="sm"
            active={showSwingAll}
            onClick={() => setShowSwingAll((v) => !v)}
          >
            {swingAllRanCount != null ? `Swing Detection All (${swingAllRanCount})` : "Swing Detection All"}
          </Button>
        </>
      )
    )
  };
  function renderSwingDetectionAllPanel() {
    const tokenCount = displayed.length;
    const runLabel = swingAllLoading
      ? 'Running…'
      : `Run on ${tokenCount} token${tokenCount === 1 ? '' : 's'}`;

    return (
      loaded && showSwingAll &&
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
          </TabsList>

          <TabsPanel value="analysis" className="px-4">
            <SwingParamsGrid params={swingParams} onChange={updateSwingParam} />

            <div className="flex flex-wrap items-center gap-3">
              <Button
                variant="primary"
                disabled={tokenCount === 0 || swingAllLoading}
                onClick={handleRunAllSwings}
              >
                {runLabel}
              </Button>
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

            <div className="flex flex-wrap items-center gap-3">
              <Button
                variant="primary"
                disabled={tokenCount === 0 || swingAllLoading}
                onClick={handleRunAllSwings}
              >
                {runLabel}
              </Button>
              {swingAllRanCount == null && (
                <span className="text-[11px] text-text-dim">
                  Run detection to populate the chain columns.
                </span>
              )}
            </div>
          </TabsPanel>
        </Tabs>

        {swingAllError && <p className="mt-4 text-sm text-red">{swingAllError}</p>}
        {swingAllLoading && (
          <p className="mt-4 text-[12px] text-text-dim">
            Running swing detection on {tokenCount} tokens…
          </p>
        )}
      </div>

    );
  }

  return (
    <div>
      <div className="mb-3.5 flex flex-wrap items-center gap-3">
        <h2 className="text-lg font-extrabold text-text">Swing detection</h2>
        {loaded && (
          <Badge variant="primary" className="font-mono">
            {displayed.length}
            {displayed.length !== total ? ` / ${total}` : ''} tokens
          </Badge>
        )}
      </div>

      <SectionDivider />

      {renderControlsBar()}

      {error && <p className="mb-2 text-sm text-red">{error}</p>}

      {!loaded && !loading && !error && (
        <p className="mb-3 text-sm text-text-dim">Click Refresh to load tokens.</p>
      )}

      <SectionDivider />


      {loaded && (
        <DataTable
          columns={columns}
          rows={displayed}
          rowKey={(r) => r.mint_address}
          selectedKey={selectedMint}
          onSelect={handleSelectMint}
          searchable
          colFilters
          colToggle
          hoverable
          storageKey="swing_detection_visible_cols_v2"
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
                <TabsTrigger value="filter">Filter</TabsTrigger>
              </TabsList>

              <TabsPanel value="analysis" className="px-4">
                <SwingParamsGrid params={swingParams} onChange={updateSwingParam} />

                <div className="flex flex-wrap items-center gap-3">
                  <Button
                    variant="primary"
                    disabled={!selectedMint || swingLoading}
                    onClick={handleRunSwing}
                  >
                    {swingLoading ? 'Running…' : 'Run'}
                  </Button>
                  <Button
                    variant="ghost"
                    className="text-[11px] text-text-dim"
                    onClick={() => setSwingParams(DEFAULT_SWING_PARAMS)}
                  >
                    Reset defaults
                  </Button>
                </div>

                {swingError && (
                  <p className="mt-4 text-sm text-red">{swingError}</p>
                )}

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
              </TabsPanel>

              <TabsPanel value="filter" className="px-4">
                {!swingResult && !swingLoading && (
                  <p className="rounded-md border border-dashed border-white/10 px-3 py-6 text-center text-[12px] text-text-dim">
                    Run swing detection on the Analysis tab first.
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
                      <label className={labelClassName}>
                        Filter min duration (ms)
                        <Input
                          fieldSize="md"
                          variant="card"
                          className="min-w-0 font-normal normal-case tracking-normal"
                          type="number"
                          min={0}
                          step={1}
                          value={swingFilter.filter_min_duration_ms}
                          onChange={(e) =>
                            updateSwingFilter('filter_min_duration_ms', e.target.value)
                          }
                        />
                      </label>
                      <label className={labelClassName}>
                        Filter max duration (ms)
                        <Input
                          fieldSize="md"
                          variant="card"
                          className="min-w-0 font-normal normal-case tracking-normal"
                          type="number"
                          min={0}
                          step={1}
                          value={swingFilter.filter_max_duration_ms}
                          onChange={(e) =>
                            updateSwingFilter('filter_max_duration_ms', e.target.value)
                          }
                        />
                      </label>
                      <label className={labelClassName}>
                        Filter min trades
                        <Input
                          fieldSize="md"
                          variant="card"
                          className="min-w-0 font-normal normal-case tracking-normal"
                          type="number"
                          min={0}
                          step={1}
                          value={swingFilter.filter_min_trades}
                          onChange={(e) =>
                            updateSwingFilter('filter_min_trades', e.target.value)
                          }
                        />
                      </label>
                      <label className={labelClassName}>
                        Filter max trades
                        <Input
                          fieldSize="md"
                          variant="card"
                          className="min-w-0 font-normal normal-case tracking-normal"
                          type="number"
                          min={0}
                          step={1}
                          value={swingFilter.filter_max_trades}
                          onChange={(e) =>
                            updateSwingFilter('filter_max_trades', e.target.value)
                          }
                        />
                      </label>
                      <label className={labelClassName}>
                        Filter min volume (SOL)
                        <Input
                          fieldSize="md"
                          variant="card"
                          className="min-w-0 font-normal normal-case tracking-normal"
                          type="number"
                          min={0}
                          step="any"
                          value={swingFilter.filter_min_volume_sol}
                          onChange={(e) =>
                            updateSwingFilter('filter_min_volume_sol', e.target.value)
                          }
                        />
                      </label>
                      <label className={labelClassName}>
                        Filter max volume (SOL)
                        <Input
                          fieldSize="md"
                          variant="card"
                          className="min-w-0 font-normal normal-case tracking-normal"
                          type="number"
                          min={0}
                          step="any"
                          value={swingFilter.filter_max_volume_sol}
                          onChange={(e) =>
                            updateSwingFilter('filter_max_volume_sol', e.target.value)
                          }
                        />
                      </label>
                      <label className={labelClassName}>
                        Filter min net flow (SOL)
                        <Input
                          fieldSize="md"
                          variant="card"
                          className="min-w-0 font-normal normal-case tracking-normal"
                          type="number"
                          step="any"
                          value={swingFilter.filter_min_net_flow_sol}
                          onChange={(e) =>
                            updateSwingFilter('filter_min_net_flow_sol', e.target.value)
                          }
                        />
                      </label>
                      <label className={labelClassName}>
                        Filter max net flow (SOL)
                        <Input
                          fieldSize="md"
                          variant="card"
                          className="min-w-0 font-normal normal-case tracking-normal"
                          type="number"
                          step="any"
                          value={swingFilter.filter_max_net_flow_sol}
                          onChange={(e) =>
                            updateSwingFilter('filter_max_net_flow_sol', e.target.value)
                          }
                        />
                      </label>
                      <label className={labelClassName}>
                        Filter min change (%)
                        <Input
                          fieldSize="md"
                          variant="card"
                          className="min-w-0 font-normal normal-case tracking-normal"
                          type="number"
                          step="any"
                          value={swingFilter.filter_min_change_pct}
                          onChange={(e) =>
                            updateSwingFilter('filter_min_change_pct', e.target.value)
                          }
                        />
                      </label>
                      <label className={labelClassName}>
                        Filter max change (%)
                        <Input
                          fieldSize="md"
                          variant="card"
                          className="min-w-0 font-normal normal-case tracking-normal"
                          type="number"
                          step="any"
                          value={swingFilter.filter_max_change_pct}
                          onChange={(e) =>
                            updateSwingFilter('filter_max_change_pct', e.target.value)
                          }
                        />
                      </label>
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
          selectedSwingLegKey={selectedSwingKey}
          onSwingLegClick={handleSwingLegClick}
          connectSwings={connectSwings}
          onConnectSwingsChange={setConnectSwings}
          athPriceInSol={selectedToken?.ath_price ?? null}
          isMigrated={selectedToken?.is_migrated}
          isMayhemMode={selectedToken?.is_mayhem_mode}
          isCashbackEnabled={selectedToken?.is_cashback_enabled}
          profileWallets={profileWallets}
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
