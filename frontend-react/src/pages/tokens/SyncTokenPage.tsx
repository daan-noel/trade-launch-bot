import { useCallback, useMemo, useRef, useState } from 'react';
import { DataTable } from '../../components/table/DataTable';
import { tokenTradeColumns } from '../../components/transactions/tokenTradeColumns';
import { TokenPriceChart, type ChartMetric } from '../../components/token-price-chart';
import { TokenDetailPanel } from '../../components/tokens/TokenDetailPanel';
import { Button } from '../../components/ui/Button';
import { usePriceUnit } from '../../context/PriceUnitContext';
import { usePriceDisplay } from '../../hooks/usePriceDisplay';
import { syncToken } from '../../services/api';
import type { SyncProgressEvent, TokenDetailRecord, TradeRecord } from '../../types';
import { cn } from '../../lib/cn';

const STAGE_ORDER = [
  'validating',
  'fetching_signatures',
  'fetching_transactions',
  'processing',
  'recomputing',
] as const;

function stagePercent(stage: string, current: number, total: number): number {
  const idx = STAGE_ORDER.indexOf(stage as (typeof STAGE_ORDER)[number]);
  const base = idx >= 0 ? (idx / STAGE_ORDER.length) * 100 : 0;
  const span = 100 / STAGE_ORDER.length;
  if (total > 0) {
    return Math.min(99, base + (current / total) * span);
  }
  return Math.min(99, base + span * 0.5);
}

function stageLabel(stage: string): string {
  switch (stage) {
    case 'validating':
      return 'Validating';
    case 'fetching_signatures':
      return 'Fetching signatures';
    case 'fetching_transactions':
      return 'Downloading transactions';
    case 'processing':
      return 'Processing';
    case 'recomputing':
      return 'Recomputing metrics';
    default:
      return stage;
  }
}

export function SyncTokenPage() {
  const price = usePriceDisplay();
  const { unit, usdRate } = usePriceUnit();
  const tradeColumns = useMemo(() => tokenTradeColumns(price), [price]);

  const [chartMetric, setChartMetric] = useState<ChartMetric>('price');
  const toChartValue = useCallback(
    (sol: number) => (unit === 'USD' && usdRate != null ? sol * usdRate : sol),
    [unit, usdRate],
  );

  const [mint, setMint] = useState('');
  const [includePostMigrate, setIncludePostMigrate] = useState(false);
  const [syncing, setSyncing] = useState(false);
  const [progress, setProgress] = useState<SyncProgressEvent | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [detail, setDetail] = useState<TokenDetailRecord | null>(null);
  const [trades, setTrades] = useState<TradeRecord[]>([]);
  const syncAbortRef = useRef<AbortController | null>(null);

  const percent = progress ? stagePercent(progress.stage, progress.current, progress.total) : 0;

  const handleCancelSync = useCallback(() => {
    syncAbortRef.current?.abort();
  }, []);

  const handleSync = useCallback(async () => {
    const trimmed = mint.trim();
    if (!trimmed) {
      setError('Enter a token mint address.');
      return;
    }

    syncAbortRef.current?.abort();
    const controller = new AbortController();
    syncAbortRef.current = controller;

    setSyncing(true);
    setError(null);
    setProgress(null);
    setDetail(null);
    setTrades([]);

    try {
      const result = await syncToken(
        trimmed,
        includePostMigrate,
        (ev) => setProgress(ev),
        controller.signal,
      );
      setDetail(result.token);
      setTrades(result.trades);
      setProgress(null);
    } catch (e) {
      if (e instanceof DOMException && e.name === 'AbortError') {
        setProgress(null);
        return;
      }
      setError(e instanceof Error ? e.message : 'Sync failed');
    } finally {
      setSyncing(false);
      if (syncAbortRef.current === controller) {
        syncAbortRef.current = null;
      }
    }
  }, [mint, includePostMigrate]);

  return (
    <div>
      <div className="mb-3.5">
        <h2 className="text-lg font-extrabold text-text">Sync token</h2>
        <p className="mt-1 text-xs text-text-dim">
          Fetch Pump.fun trade history from Helius, merge missing rows into the database, and
          update the tracked tokens cache.
        </p>
      </div>

      <div className="mb-4 flex flex-wrap items-end gap-3 rounded-lg border border-white/6 bg-white/2 p-4">
        <label className="flex min-w-[280px] flex-1 flex-col gap-1">
          <span className="text-[10px] font-bold uppercase tracking-widest text-text-dim">
            Mint address
          </span>
          <input
            type="text"
            value={mint}
            onChange={(e) => setMint(e.target.value)}
            placeholder="Token mint (base58)"
            disabled={syncing}
            className="rounded-md border border-white/10 bg-bg-card px-3 py-2 font-mono text-[13px] text-text placeholder:text-text-dim/60 disabled:opacity-60"
          />
        </label>

        <label className="flex cursor-pointer items-center gap-2 pb-2 text-[13px] text-text-mid">
          <input
            type="checkbox"
            checked={includePostMigrate}
            onChange={(e) => setIncludePostMigrate(e.target.checked)}
            disabled={syncing}
            className="size-4 rounded border-white/20 accent-primary"
          />
          Include post-migrate trades
        </label>

        {syncing ? (
          <Button variant="ghost" onClick={handleCancelSync}>
            Cancel
          </Button>
        ) : (
          <Button variant="primary" onClick={handleSync} disabled={!mint.trim()}>
            Fetch
          </Button>
        )}
      </div>

      {syncing && (
        <div className="mb-4 rounded-lg border border-white/6 bg-white/2 p-4">
          <div className="mb-2 flex items-center justify-between gap-2">
            <span className="text-[11px] font-bold uppercase tracking-widest text-primary">
              {progress ? stageLabel(progress.stage) : 'Starting…'}
            </span>
            <span className="font-mono text-[11px] text-text-dim">
              {Math.round(percent)}%
            </span>
          </div>
          <div className="h-2 overflow-hidden rounded-full bg-white/6">
            <div
              className={cn(
                'h-full rounded-full bg-primary transition-[width] duration-300',
                'animate-pulse',
              )}
              style={{ width: `${percent}%` }}
            />
          </div>
          {progress?.message && (
            <p className="mt-2 text-xs text-text-dim">{progress.message}</p>
          )}
        </div>
      )}

      {error && (
        <p className="mb-4 rounded-md border border-red/30 bg-red/10 px-3 py-2 text-sm text-red">
          {error}
        </p>
      )}

      {detail && (
        <>
          <h3 className="mb-2 text-sm font-bold text-text">Token</h3>
          <div className="mb-6 rounded-lg border border-white/6 bg-white/2 p-3">
            <TokenDetailPanel detail={detail} loading={false} error={null} />
          </div>

          <div className="mb-6">
            <TokenPriceChart
              symbol={detail.symbol || detail.name || detail.mint_address}
              id={detail.mint_address}
              trades={trades}
              toValue={toChartValue}
              priceLabel={chartMetric === 'mc' ? `MC (${unit})` : unit}
              priceUnit={unit}
              metric={chartMetric}
              onMetricChange={setChartMetric}
              athPriceInSol={detail.ath_price}
              isMigrated={detail.is_migrated}
              isMayhemMode={detail.is_mayhem_mode}
              isCashbackEnabled={detail.is_cashback_enabled}
            />
          </div>

          <div className="mb-2 flex items-center gap-2">
            <h3 className="text-sm font-bold text-text">Trades</h3>
            <span className="rounded-md border border-primary bg-primary/15 px-2 py-0.5 font-mono text-[11px] font-bold text-primary">
              {trades.length}
            </span>
          </div>
          <DataTable
            columns={tradeColumns}
            rows={trades}
            rowKey={(t) => `${t.tx_signature}-${t.leg_index}`}
            defaultPageSize={25}
            searchable
            colFilters
            hoverable
            emptyMessage="No trades"
          />
        </>
      )}
    </div>
  );
}
