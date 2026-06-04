import { useCallback, useEffect, useMemo, useState } from 'react';
import { DataTable } from '../../components/table/DataTable';
import { tokenTradeColumns } from '../../components/transactions/tokenTradeColumns';
import { TokenPriceChart, tradeBarTime, type ChartBarSelection, type ChartMetric } from '../../components/token-price-chart';
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
import { usePriceDisplay } from '../../hooks/usePriceDisplay';
import { swingColumns } from '../../components/analysis/swingColumns';
import {
  DEFAULT_SWING_FILTER,
  filterSwings,
  hasActiveSwingFilter,
  parseSwingFilterField,
  type SwingFilterCriteria,
} from '../../components/analysis/swingFilter';
import { Badge } from '../../components/ui/Badge';
import { Button } from '../../components/ui/Button';
import { Input } from '../../components/ui/Input';
import { Select } from '../../components/ui/Select';
import { Tabs, TabsList, TabsPanel, TabsTrigger } from '../../components/ui/Tabs';
import { VisibilityToggleButton } from '../../components/ui/VisibilityToggleButton';
import { fetchTokenSwings, fetchTokenTrades, fetchTokens } from '../../services/api';
import type {
  SwingDetectionResult,
  SwingParams,
  TokenRecord,
  TradeRecord,
} from '../../types';

function toDatetimeLocalValue(date: Date): string {
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

function startOfToday(): Date {
  const d = new Date();
  d.setHours(0, 0, 0, 0);
  return d;
}

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

const DEFAULT_SWING_PARAMS: SwingParams = {
  high_to_low_threshold_sol: 5,
  high_to_low_threshold_pct: 50,
  low_to_high_threshold_sol: 5,
  low_to_high_threshold_pct: 50,
  min_leg_trades: 2,
  min_leg_duration_ms: 0,
  min_leg_volume: 0,
  min_leg_net_flow: 0,
  max_leg_trades: 0,
  max_leg_duration_ms: 0,
  max_leg_volume: 0,
  max_leg_net_flow: 0,
};

const SWING_PARAM_INT_KEYS = new Set<keyof SwingParams>([
  'min_leg_trades',
  'min_leg_duration_ms',
  'max_leg_trades',
  'max_leg_duration_ms',
]);

const LS_SWING_DETECTION_KEY = 'swing_detection_criteria';

const SWING_PARAM_KEYS = Object.keys(DEFAULT_SWING_PARAMS) as (keyof SwingParams)[];

function isFiniteNumber(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value);
}

function mergeSwingParams(partial: Partial<SwingParams> | undefined): SwingParams {
  if (!partial) return DEFAULT_SWING_PARAMS;
  const merged = { ...DEFAULT_SWING_PARAMS };
  for (const key of SWING_PARAM_KEYS) {
    const value = partial[key];
    if (isFiniteNumber(value)) merged[key] = value;
  }
  return merged;
}

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
} {
  try {
    const raw = localStorage.getItem(LS_SWING_DETECTION_KEY);
    if (!raw) {
      return {
        params: DEFAULT_SWING_PARAMS,
        filter: DEFAULT_SWING_FILTER,
        appliedFilter: DEFAULT_SWING_FILTER,
      };
    }
    const parsed = JSON.parse(raw) as {
      params?: Partial<SwingParams>;
      filter?: Partial<SwingFilterCriteria>;
      appliedFilter?: Partial<SwingFilterCriteria>;
    };
    return {
      params: mergeSwingParams(parsed.params),
      filter: mergeSwingFilter(parsed.filter),
      appliedFilter: mergeSwingFilter(parsed.appliedFilter ?? parsed.filter),
    };
  } catch {
    return {
      params: DEFAULT_SWING_PARAMS,
      filter: DEFAULT_SWING_FILTER,
      appliedFilter: DEFAULT_SWING_FILTER,
    };
  }
}

function saveStoredSwingCriteria(
  params: SwingParams,
  filter: SwingFilterCriteria,
  appliedFilter: SwingFilterCriteria,
): void {
  try {
    localStorage.setItem(
      LS_SWING_DETECTION_KEY,
      JSON.stringify({ params, filter, appliedFilter }),
    );
  } catch {
    /* ignore */
  }
}

const labelClassName =
  'flex flex-col gap-1 text-[10px] font-bold uppercase tracking-widest text-text-dim';

export function SwingDetectionPage() {
  const [storedSwingCriteria] = useState(loadStoredSwingCriteria);

  const price = usePriceDisplay();
  const { unit, usdRate } = usePriceUnit();
  const columns = useMemo(() => tokenColumns(price), [price]);
  const tradeTableColumns = useMemo(() => tokenTradeColumns(price), [price]);
  const swingTableColumns = useMemo(() => swingColumns(price), [price]);

  const toChartValue = useCallback(
    (sol: number) => (unit === 'USD' && usdRate != null ? sol * usdRate : sol),
    [unit, usdRate],
  );

  const [tokens, setTokens] = useState<TokenRecord[]>([]);
  const [total, setTotal] = useState(0);
  const [loaded, setLoaded] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [createdFrom, setCreatedFrom] = useState(() =>
    toDatetimeLocalValue(startOfToday()),
  );
  const [createdTo, setCreatedTo] = useState(() =>
    toDatetimeLocalValue(new Date()),
  );

  const [showFilters, setShowFilters] = useState(false);
  const [filters, setFilters] = useState<TokenFilters>(loadStoredTokenFilters);

  const [selectedMint, setSelectedMint] = useState<string | null>(null);
  const [trades, setTrades] = useState<TradeRecord[]>([]);
  const [tradesLoading, setTradesLoading] = useState(false);
  const [tradesError, setTradesError] = useState<string | null>(null);
  const [selectedBar, setSelectedBar] = useState<ChartBarSelection | null>(null);
  const [chartMetric, setChartMetric] = useState<ChartMetric>('price');

  const [activeAnalysis, setActiveAnalysis] = useState<AnalysisKind | null>(null);
  const [swingParams, setSwingParams] = useState<SwingParams>(storedSwingCriteria.params);
  const [swingResult, setSwingResult] = useState<SwingDetectionResult | null>(null);
  const [swingLoading, setSwingLoading] = useState(false);
  const [swingError, setSwingError] = useState<string | null>(null);
  const [swingPanelTab, setSwingPanelTab] = useState<SwingPanelTab>('analysis');
  const [swingFilter, setSwingFilter] = useState<SwingFilterCriteria>(storedSwingCriteria.filter);
  const [appliedSwingFilter, setAppliedSwingFilter] = useState<SwingFilterCriteria>(
    storedSwingCriteria.appliedFilter,
  );
  const [showSwingResultsTable, setShowSwingResultsTable] = useState(false);

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
        [key]: Number.isFinite(parsed) ? parsed : prev[key],
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
      const result = await fetchTokenSwings(selectedMint, swingParams);
      setSwingResult(result);
    } catch (e) {
      setSwingResult(null);
      setSwingError(e instanceof Error ? e.message : 'Swing detection failed');
    } finally {
      setSwingLoading(false);
    }
  }, [selectedMint, swingParams]);

  const displayed = useMemo(
    () => filterDisplayedTokens(tokens, createdFrom, createdTo, filters),
    [tokens, createdFrom, createdTo, filters],
  );

  const filterCount = activeFilterCount(filters);

  const selectedToken = useMemo(
    () => displayed.find((t) => t.mint_address === selectedMint) ?? null,
    [displayed, selectedMint],
  );

  const loadTrades = useCallback(async (mint: string) => {
    setTradesLoading(true);
    setTradesError(null);
    try {
      const data = await fetchTokenTrades(mint);
      setTrades(data);
    } catch (e) {
      setTrades([]);
      setTradesError(e instanceof Error ? e.message : 'Failed to load trades');
    } finally {
      setTradesLoading(false);
    }
  }, []);

  const handleRefresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const result = await fetchTokens('', 5000, 0);
      setTokens(result.items);
      setTotal(result.total);
      setLoaded(true);
      if (selectedMint) {
        const stillVisible = filterDisplayedTokens(
          result.items,
          createdFrom,
          createdTo,
          filters,
        ).some((t) => t.mint_address === selectedMint);
        if (stillVisible) {
          await loadTrades(selectedMint);
        } else {
          setSelectedMint(null);
          setTrades([]);
        }
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to load tokens');
    } finally {
      setLoading(false);
    }
  }, [selectedMint, createdFrom, createdTo, filters, loadTrades]);

  useEffect(() => {
    saveStoredSwingCriteria(swingParams, swingFilter, appliedSwingFilter);
  }, [swingParams, swingFilter, appliedSwingFilter]);

  useEffect(() => {
    if (!selectedMint) {
      setTrades([]);
      setTradesError(null);
      setTradesLoading(false);
      setSelectedBar(null);
      setSwingResult(null);
      setSwingError(null);
      return;
    }
    setSelectedBar(null);
    setSwingResult(null);
    setSwingError(null);
    loadTrades(selectedMint);
  }, [selectedMint, loadTrades]);

  useEffect(() => {
    if (!selectedMint) return;
    if (!displayed.some((t) => t.mint_address === selectedMint)) {
      setSelectedMint(null);
    }
  }, [displayed, selectedMint]);

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
      : new Date(Number(selectedBar.barTime) * 1000).toISOString().replace('T', ' ').slice(0, 19) + ' UTC'
    : '';

  const filteredSwings = useMemo(() => {
    if (!swingResult) return [];
    return filterSwings(swingResult.swings, appliedSwingFilter);
  }, [swingResult, appliedSwingFilter]);

  const swingOverlay = useMemo(() => {
    if (!swingResult || swingResult.mint !== selectedMint) return null;
    const filterActive =
      swingPanelTab === 'filter' && hasActiveSwingFilter(appliedSwingFilter);
    return {
      legs: filterActive ? filteredSwings : swingResult.swings,
      segmentMode: filterActive ? ('perLeg' as const) : ('connected' as const),
    };
  }, [swingResult, selectedMint, swingPanelTab, appliedSwingFilter, filteredSwings]);

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

      <div className="mb-1.5 flex gap-1.5">
        <Button
          variant="subtle"
          size="sm"
          active={showFilters || filterCount > 0}
          onClick={() => setShowFilters((v) => !v)}
        >
          {filterCount > 0 ? `Global Filters (${filterCount})` : 'Global Filters'}
        </Button>
      </div>

      {showFilters && (
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
      )}

      <SectionDivider />

      <div className="mb-3 flex flex-wrap items-end gap-3">
        <label className="flex flex-col gap-1 text-[10px] font-bold uppercase tracking-widest text-text-dim">
          Created from
          <Input
            type="datetime-local"
            fieldSize="md"
            variant="card"
            value={createdFrom}
            onChange={(e) => setCreatedFrom(e.target.value)}
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
            onChange={(e) => setCreatedTo(e.target.value)}
            className="min-w-0 font-normal normal-case tracking-normal"
          />
        </label>
        <Button
          variant="primary"
          onClick={handleRefresh}
          disabled={loading}
        >
          {loading ? 'Refreshing…' : 'Fetch'}
        </Button>
        {(createdFrom || createdTo) && (
          <Button
            variant="ghost"
            onClick={() => {
              setCreatedFrom('');
              setCreatedTo('');
            }}
          >
            Clear
          </Button>
        )}
      </div>

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
          onSelect={setSelectedMint}
          searchable
          colFilters
          colToggle
          hoverable
          storageKey="swing_detection_visible_cols"
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
                <div className="mb-4 grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
                  <label className={labelClassName}>
                    High → low (SOL)
                    <Input
                      fieldSize="md"
                      variant="card"
                      className="min-w-0 font-normal normal-case tracking-normal"
                      type="number"
                      min={0}
                      step="any"
                      value={swingParams.high_to_low_threshold_sol}
                      onChange={(e) =>
                        updateSwingParam('high_to_low_threshold_sol', e.target.value)
                      }
                    />
                  </label>
                  <label className={labelClassName}>
                    High → low (%)
                    <Input
                      fieldSize="md"
                      variant="card"
                      className="min-w-0 font-normal normal-case tracking-normal"
                      type="number"
                      min={0}
                      max={100}
                      step="any"
                      value={swingParams.high_to_low_threshold_pct}
                      onChange={(e) =>
                        updateSwingParam('high_to_low_threshold_pct', e.target.value)
                      }
                    />
                  </label>
                  <label className={labelClassName}>
                    Low → high (SOL)
                    <Input
                      fieldSize="md"
                      variant="card"
                      className="min-w-0 font-normal normal-case tracking-normal"
                      type="number"
                      min={0}
                      step="any"
                      value={swingParams.low_to_high_threshold_sol}
                      onChange={(e) =>
                        updateSwingParam('low_to_high_threshold_sol', e.target.value)
                      }
                    />
                  </label>
                  <label className={labelClassName}>
                    Low → high (%)
                    <Input
                      fieldSize="md"
                      variant="card"
                      className="min-w-0 font-normal normal-case tracking-normal"
                      type="number"
                      min={0}
                      max={100}
                      step="any"
                      value={swingParams.low_to_high_threshold_pct}
                      onChange={(e) =>
                        updateSwingParam('low_to_high_threshold_pct', e.target.value)
                      }
                    />
                  </label>
                  <label className={labelClassName}>
                    Min leg trades
                    <Input
                      fieldSize="md"
                      variant="card"
                      className="min-w-0 font-normal normal-case tracking-normal"
                      type="number"
                      min={0}
                      step={1}
                      value={swingParams.min_leg_trades}
                      onChange={(e) => updateSwingParam('min_leg_trades', e.target.value)}
                    />
                  </label>
                  <label className={labelClassName}>
                    Min leg duration (ms)
                    <Input
                      fieldSize="md"
                      variant="card"
                      className="min-w-0 font-normal normal-case tracking-normal"
                      type="number"
                      min={0}
                      step={1}
                      value={swingParams.min_leg_duration_ms}
                      onChange={(e) => updateSwingParam('min_leg_duration_ms', e.target.value)}
                    />
                  </label>
                  <label className={labelClassName}>
                    Min leg volume (SOL)
                    <Input
                      fieldSize="md"
                      variant="card"
                      className="min-w-0 font-normal normal-case tracking-normal"
                      type="number"
                      min={0}
                      step="any"
                      value={swingParams.min_leg_volume}
                      onChange={(e) => updateSwingParam('min_leg_volume', e.target.value)}
                    />
                  </label>
                  <label className={labelClassName}>
                    Min leg net flow (SOL)
                    <Input
                      fieldSize="md"
                      variant="card"
                      className="min-w-0 font-normal normal-case tracking-normal"
                      type="number"
                      min={0}
                      step="any"
                      value={swingParams.min_leg_net_flow}
                      onChange={(e) => updateSwingParam('min_leg_net_flow', e.target.value)}
                    />
                  </label>
                  <label className={labelClassName}>
                    Max leg trades
                    <Input
                      fieldSize="md"
                      variant="card"
                      className="min-w-0 font-normal normal-case tracking-normal"
                      type="number"
                      min={0}
                      step={1}
                      value={swingParams.max_leg_trades}
                      onChange={(e) => updateSwingParam('max_leg_trades', e.target.value)}
                    />
                  </label>
                  <label className={labelClassName}>
                    Max leg duration (ms)
                    <Input
                      fieldSize="md"
                      variant="card"
                      className="min-w-0 font-normal normal-case tracking-normal"
                      type="number"
                      min={0}
                      step={1}
                      value={swingParams.max_leg_duration_ms}
                      onChange={(e) => updateSwingParam('max_leg_duration_ms', e.target.value)}
                    />
                  </label>
                  <label className={labelClassName}>
                    Max leg volume (SOL)
                    <Input
                      fieldSize="md"
                      variant="card"
                      className="min-w-0 font-normal normal-case tracking-normal"
                      type="number"
                      min={0}
                      step="any"
                      value={swingParams.max_leg_volume}
                      onChange={(e) => updateSwingParam('max_leg_volume', e.target.value)}
                    />
                  </label>
                  <label className={labelClassName}>
                    Max leg net flow (SOL)
                    <Input
                      fieldSize="md"
                      variant="card"
                      className="min-w-0 font-normal normal-case tracking-normal"
                      type="number"
                      min={0}
                      step="any"
                      value={swingParams.max_leg_net_flow}
                      onChange={(e) => updateSwingParam('max_leg_net_flow', e.target.value)}
                    />
                  </label>
                </div>

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
                          rowKey={(leg) => `${leg.type}-${leg.start_at}-${leg.end_at}`}
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
                        onClick={() => setAppliedSwingFilter(swingFilter)}
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
                      {!hasActiveSwingFilter(swingFilter) && (
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
                        rowKey={(leg) => `${leg.type}-${leg.start_at}-${leg.end_at}`}
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
          onBarClick={setSelectedBar}
          swingOverlay={swingOverlay}
          athPriceInSol={selectedToken?.ath_price ?? null}
          isMigrated={selectedToken?.is_migrated}
          isMayhemMode={selectedToken?.is_mayhem_mode}
          isCashbackEnabled={selectedToken?.is_cashback_enabled}
        />
      </div>

      {selectedBar && selectedMint && (
        <>
          <SectionDivider />
          <div>
            <div className="mb-2 flex flex-wrap items-center gap-2">
              <h3 className="text-sm font-bold text-text">Bar trades</h3>
              <span className="font-mono text-[11px] text-text-dim">{barTimeLabel}</span>
              <Badge variant="primary" className="font-mono font-normal">
                {barTrades.length} trade{barTrades.length === 1 ? '' : 's'}
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
              columns={tradeTableColumns}
              rows={barTrades}
              rowKey={(t) => t.id}
              defaultPageSize={25}
              searchable
              colFilters
              hoverable
              emptyMessage="No trades in this bar."
            />
          </div>
        </>
      )}
    </div>
  );
}
