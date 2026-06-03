import { useCallback, useEffect, useMemo, useState } from 'react';
import { DataTable } from '../components/table/DataTable';
import { tokenTradeColumns } from '../components/transactions/tokenTradeColumns';
import { TokenPriceChart, tradeBarTime, type ChartBarSelection, type ChartMetric } from '../components/token-price-chart';
import { tokenColumns } from '../components/tokens/tokenColumns';
import { Button } from '../components/ui/Button';
import { usePriceUnit } from '../context/PriceUnitContext';
import { usePriceDisplay } from '../hooks/usePriceDisplay';
import { fetchTokenTrades, fetchTokens } from '../services/api';
import type { TokenRecord, TradeRecord } from '../types';

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

function SectionDivider() {
  return <div role="separator" className="my-6 border-t border-white/6" />;
}

export function PeakTroughAnalysisPage() {
  const price = usePriceDisplay();
  const { unit, usdRate } = usePriceUnit();
  const columns = useMemo(() => tokenColumns(price), [price]);
  const tradeTableColumns = useMemo(() => tokenTradeColumns(price), [price]);

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

  const [selectedMint, setSelectedMint] = useState<string | null>(null);
  const [trades, setTrades] = useState<TradeRecord[]>([]);
  const [tradesLoading, setTradesLoading] = useState(false);
  const [tradesError, setTradesError] = useState<string | null>(null);
  const [selectedBar, setSelectedBar] = useState<ChartBarSelection | null>(null);
  const [chartMetric, setChartMetric] = useState<ChartMetric>('price');

  const displayed = useMemo(
    () => filterByCreatedRange(tokens, createdFrom, createdTo),
    [tokens, createdFrom, createdTo],
  );

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
        const stillVisible = filterByCreatedRange(
          result.items,
          createdFrom,
          createdTo,
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
  }, [selectedMint, createdFrom, createdTo, loadTrades]);

  useEffect(() => {
    if (!selectedMint) {
      setTrades([]);
      setTradesError(null);
      setTradesLoading(false);
      setSelectedBar(null);
      return;
    }
    setSelectedBar(null);
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

  return (
    <div>
      <div className="mb-3.5 flex flex-wrap items-center gap-3">
        <h2 className="text-lg font-extrabold text-text">Peak / Trough Analysis</h2>
        {loaded && (
          <span className="rounded-md border border-primary bg-primary/15 px-2.5 py-0.5 font-mono text-[11px] font-bold tracking-wide text-primary">
            {displayed.length}
            {displayed.length !== total ? ` / ${total}` : ''} tokens
          </span>
        )}
      </div>

      <SectionDivider />

      <div className="mb-3 flex flex-wrap items-end gap-3">
        <label className="flex flex-col gap-1 text-[10px] font-bold uppercase tracking-widest text-text-dim">
          Created from
          <input
            type="datetime-local"
            value={createdFrom}
            onChange={(e) => setCreatedFrom(e.target.value)}
            className="rounded-md border border-white/10 bg-bg-card px-2 py-1.5 text-[13px] font-normal normal-case tracking-normal text-text"
          />
        </label>
        <label className="flex flex-col gap-1 text-[10px] font-bold uppercase tracking-widest text-text-dim">
          Created to
          <input
            type="datetime-local"
            value={createdTo}
            onChange={(e) => setCreatedTo(e.target.value)}
            className="rounded-md border border-white/10 bg-bg-card px-2 py-1.5 text-[13px] font-normal normal-case tracking-normal text-text"
          />
        </label>
        <Button
          variant="primary"
          onClick={handleRefresh}
          disabled={loading}
          className="mb-0.5"
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
            className="mb-1 text-[11px] text-text-dim hover:text-text"
          >
            Clear dates
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
          pageSizeOptions={[10, 25, 50, 100]}
          searchable
          colFilters
          colToggle
          hoverable
          storageKey="peak_trough_visible_cols"
          emptyMessage="No tokens in this date range."
        />
      )}

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
            <span className="rounded-md border border-primary/35 bg-primary/12 px-2 py-0.5 font-mono text-[11px] text-primary">
              {barTrades.length} trade{barTrades.length === 1 ? '' : 's'}
            </span>
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
            pageSizeOptions={[10, 25, 50, 100]}
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
