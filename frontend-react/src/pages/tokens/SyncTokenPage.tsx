import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { DataTable } from '../../components/table/DataTable';
import type { ColumnDef } from '../../components/table/types';
import { tokenTradeColumns } from '../../components/transactions/tokenTradeColumns';
import { TokenPriceChart, WALLET_MARKER_COLORS, type ChartMetric, type ProfileWalletInfo } from '../../components/token-price-chart';
import { TokenDetailPanel } from '../../components/tokens/TokenDetailPanel';
import { AddressDisplay } from '../../components/ui/AddressDisplay';
import { Badge } from '../../components/ui/Badge';
import { Button } from '../../components/ui/Button';
import { Checkbox } from '../../components/ui/Checkbox';
import { Textarea } from '../../components/ui/Input';
import { usePriceUnit } from '../../context/PriceUnitContext';
import { usePriceDisplay } from '../../hooks/usePriceDisplay';
import { fetchProfiles, syncToken } from '../../services/api';
import type { SyncProgressEvent, TokenDetailRecord, TradeRecord, WalletProfile } from '../../types';
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

function buildProfileWallets(profiles: WalletProfile[]): ProfileWalletInfo[] {
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
}

/** Split a comma/newline/whitespace separated blob into unique, trimmed mint addresses. */
function parseMints(raw: string): string[] {
  return Array.from(
    new Set(
      raw
        .split(/[\s,]+/)
        .map((m) => m.trim())
        .filter(Boolean),
    ),
  );
}

type SyncResultItem = { mint: string; ok: boolean; error?: string };

/** A token that synced successfully, paired with its trades. */
type SyncedToken = { token: TokenDetailRecord; trades: TradeRecord[] };

/** A synced-tokens table row: a sync result, with its token record (null on failure). */
type SyncedRow = SyncResultItem & { token: TokenDetailRecord | null };

/** Compact column set for the synced-tokens picker table. */
function syncedTokenColumns(
  price: ReturnType<typeof usePriceDisplay>,
): ColumnDef<SyncedRow>[] {
  return [
    {
      key: 'result',
      label: 'Result',
      width: '120px',
      sortable: true,
      render: (r) =>
        r.ok ? (
          <Badge variant="success" size="sm">OK</Badge>
        ) : (
          <span className="inline-flex max-w-full items-center gap-1.5">
            <Badge variant="danger" size="sm">FAIL</Badge>
            {r.error && (
              <span className="truncate text-[11px] text-red" title={r.error}>
                {r.error}
              </span>
            )}
          </span>
        ),
      sortValue: (r) => (r.ok ? 1 : 0),
      searchValue: (r) => (r.ok ? 'ok' : `fail ${r.error ?? ''}`),
    },
    {
      key: 'symbol',
      label: 'Symbol',
      width: '90px',
      sortable: true,
      render: (r) => r.token?.symbol || '-',
      sortValue: (r) => r.token?.symbol ?? null,
      searchValue: (r) => `${r.token?.symbol ?? ''} ${r.token?.name ?? ''} ${r.mint}`,
    },
    {
      key: 'name',
      label: 'Name',
      width: '130px',
      sortable: true,
      render: (r) => r.token?.name || '-',
      sortValue: (r) => r.token?.name ?? null,
      searchValue: (r) => r.token?.name ?? '',
    },
    {
      key: 'mint',
      label: 'Mint',
      width: '165px',
      render: (r) => <AddressDisplay address={r.mint} kind="token" stopPropagation />,
      sortValue: (r) => r.mint,
      searchValue: (r) => r.mint,
    },
    {
      key: 'trade_count',
      label: 'Trades',
      width: '70px',
      sortable: true,
      render: (r) => r.token?.trade_count ?? '-',
      sortValue: (r) => r.token?.trade_count ?? null,
      searchValue: (r) => String(r.token?.trade_count ?? ''),
    },
    {
      key: 'volume',
      label: 'Volume',
      width: '84px',
      sortable: true,
      render: (r) =>
        r.token?.volume_sol_total != null ? price.displayCompact(r.token.volume_sol_total, 4) : '-',
      sortValue: (r) => r.token?.volume_sol_total ?? null,
      searchValue: (r) => String(r.token?.volume_sol_total ?? ''),
    },
    {
      key: 'market_cap',
      label: 'MCap',
      width: '84px',
      sortable: true,
      render: (r) =>
        r.token?.market_cap != null ? price.displayCompact(r.token.market_cap, 3) : '-',
      sortValue: (r) => r.token?.market_cap ?? null,
      searchValue: (r) => String(r.token?.market_cap ?? ''),
    },
    {
      key: 'current_price',
      label: 'Price',
      width: '88px',
      sortable: true,
      render: (r) =>
        r.token?.current_price != null ? price.displayPrice(r.token.current_price) : '-',
      sortValue: (r) => r.token?.current_price ?? null,
      searchValue: (r) => String(r.token?.current_price ?? ''),
    },
    {
      key: 'migrated',
      label: 'Migrated',
      width: '70px',
      sortable: true,
      render: (r) => (r.token?.is_migrated ? '✓' : ''),
      sortValue: (r) => (r.token?.is_migrated ? 1 : 0),
      searchValue: (r) => String(r.token?.is_migrated ?? ''),
    },
  ];
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
  const [batch, setBatch] = useState<{ index: number; total: number } | null>(null);
  const [results, setResults] = useState<SyncResultItem[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [syncedTokens, setSyncedTokens] = useState<SyncedToken[]>([]);
  const [selectedMint, setSelectedMint] = useState<string | null>(null);
  const syncAbortRef = useRef<AbortController | null>(null);
  const [profiles, setProfiles] = useState<WalletProfile[]>([]);

  useEffect(() => {
    fetchProfiles().then(setProfiles).catch(() => {});
  }, []);

  const profileWallets = useMemo(() => buildProfileWallets(profiles), [profiles]);
  const mints = useMemo(() => parseMints(mint), [mint]);

  const syncedColumns = useMemo(() => syncedTokenColumns(price), [price]);
  const syncedRows = useMemo<SyncedRow[]>(
    () =>
      results.map((r) => ({
        ...r,
        token: syncedTokens.find((t) => t.token.mint_address === r.mint)?.token ?? null,
      })),
    [results, syncedTokens],
  );
  const failedCount = useMemo(() => results.filter((r) => !r.ok).length, [results]);
  const selected = useMemo(
    () => syncedTokens.find((t) => t.token.mint_address === selectedMint) ?? null,
    [syncedTokens, selectedMint],
  );

  const percent = progress ? stagePercent(progress.stage, progress.current, progress.total) : 0;

  const handleCancelSync = useCallback(() => {
    syncAbortRef.current?.abort();
  }, []);

  const handleSync = useCallback(async (incremental = false) => {
    const targets = parseMints(mint);
    if (targets.length === 0) {
      setError('Enter at least one token mint address.');
      return;
    }

    syncAbortRef.current?.abort();
    const controller = new AbortController();
    syncAbortRef.current = controller;

    setSyncing(true);
    setError(null);
    setProgress(null);
    setSyncedTokens([]);
    setSelectedMint(null);
    setResults([]);
    setBatch(null);

    const collected: SyncResultItem[] = [];
    const oks: SyncedToken[] = [];

    try {
      for (let i = 0; i < targets.length; i++) {
        const target = targets[i];
        setBatch({ index: i, total: targets.length });
        setProgress(null);
        try {
          const result = await syncToken(
            target,
            includePostMigrate,
            (ev) => setProgress(ev),
            controller.signal,
            incremental,
          );
          collected.push({ mint: target, ok: true });
          oks.push({ token: result.token, trades: result.trades });
        } catch (e) {
          if (e instanceof DOMException && e.name === 'AbortError') {
            throw e;
          }
          collected.push({
            mint: target,
            ok: false,
            error: e instanceof Error ? e.message : 'Sync failed',
          });
        }
      }
      setResults(collected);
      setSyncedTokens(oks);
      setSelectedMint(oks[0]?.token.mint_address ?? null);
      setProgress(null);
    } catch (e) {
      if (e instanceof DOMException && e.name === 'AbortError') {
        setProgress(null);
        setResults(collected);
        setSyncedTokens(oks);
        setSelectedMint(oks[0]?.token.mint_address ?? null);
        return;
      }
      setError(e instanceof Error ? e.message : 'Sync failed');
    } finally {
      setSyncing(false);
      setBatch(null);
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
            Mint address{mints.length > 1 ? ` (${mints.length})` : ''}
          </span>
          <Textarea
            fieldSize="md"
            variant="card"
            rows={1}
            autoResize
            value={mint}
            onChange={(e) => setMint(e.target.value)}
            placeholder="One or more token mints (base58), separated by comma or newline"
            disabled={syncing}
            className="font-mono placeholder:text-text-dim/60"
          />
        </label>

        <label className="flex cursor-pointer items-center gap-2 pb-2 text-[13px] text-text-mid">
          <Checkbox
            checked={includePostMigrate}
            onChange={(e) => setIncludePostMigrate(e.target.checked)}
            disabled={syncing}
          />
          Include post-migrate trades
        </label>

        {syncing ? (
          <Button variant="ghost" onClick={handleCancelSync}>
            Cancel
          </Button>
        ) : (
          <>
            <Button variant="primary" onClick={() => handleSync(false)} disabled={mints.length === 0}>
              Fetch All
            </Button>
            <Button variant="ghost" onClick={() => handleSync(true)} disabled={mints.length === 0}>
              Fetch New
            </Button>
          </>
        )}
      </div>

      {syncing && (
        <div className="mb-4 rounded-lg border border-white/6 bg-white/2 p-4">
          <div className="mb-2 flex items-center justify-between gap-2">
            <span className="text-[11px] font-bold uppercase tracking-widest text-primary">
              {batch && batch.total > 1 && (
                <span className="mr-2 text-text-dim">
                  Token {batch.index + 1}/{batch.total}
                </span>
              )}
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

      {syncedRows.length > 0 && (
        <div className="mb-6">
          <div className="mb-2 flex items-center gap-2">
            <h3 className="text-sm font-bold text-text">Synced tokens</h3>
            <Badge variant="primary" className="font-mono">
              {syncedRows.length}
            </Badge>
            {failedCount > 0 && <Badge variant="danger">{failedCount} failed</Badge>}
          </div>
          <DataTable
            columns={syncedColumns}
            rows={syncedRows}
            rowKey={(r) => r.mint}
            selectedKey={selectedMint}
            onSelect={(key) => {
              if (key && syncedTokens.some((t) => t.token.mint_address === key)) {
                setSelectedMint(key);
              }
            }}
            defaultPageSize={25}
            searchable
            hoverable
            emptyMessage="No tokens"
          />
        </div>
      )}

      {selected && (
        <>
          <h3 className="mb-2 text-sm font-bold text-text">Token</h3>
          <div className="mb-6 rounded-lg border border-white/6 bg-white/2 p-3">
            <TokenDetailPanel detail={selected.token} loading={false} error={null} />
          </div>

          <div className="mb-6">
            <TokenPriceChart
              key={selected.token.mint_address}
              symbol={selected.token.symbol || selected.token.name || selected.token.mint_address}
              id={selected.token.mint_address}
              trades={selected.trades}
              toValue={toChartValue}
              priceLabel={chartMetric === 'mc' ? `MC (${unit})` : unit}
              priceUnit={unit}
              metric={chartMetric}
              onMetricChange={setChartMetric}
              athPriceInSol={selected.token.ath_price}
              isMigrated={selected.token.is_migrated}
              isMayhemMode={selected.token.is_mayhem_mode}
              isCashbackEnabled={selected.token.is_cashback_enabled}
              profileWallets={profileWallets}
            />
          </div>

          <div className="mb-2 flex items-center gap-2">
            <h3 className="text-sm font-bold text-text">Trades</h3>
            <Badge variant="primary" className="font-mono">
              {selected.trades.length}
            </Badge>
          </div>
          <DataTable
            key={selected.token.mint_address}
            columns={tradeColumns}
            rows={selected.trades}
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
