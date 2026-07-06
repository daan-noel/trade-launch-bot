import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useDispatch, useSelector } from 'react-redux';
import { DataTable } from 'components/table/DataTable';
import { RelativeTimeCell } from 'components/table/RelativeTimeCell';
import type { ColumnDef } from 'components/table/types';
import { TokenTradeChart } from 'components/tokens/TokenTradeChart';
import { InputSyncStatus } from '@live/components/tokens/InputSyncStatus';
import { TokenDetailPanel } from 'components/tokens/TokenDetailPanel';
import { AddressDisplay } from 'components/ui/AddressDisplay';
import { Badge } from 'components/ui/Badge';
import { Button } from 'components/ui/Button';
import { Checkbox } from 'components/ui/Checkbox';
import { Textarea } from 'components/ui/Input';
import { usePriceDisplay } from 'hooks/usePriceDisplay';
import { syncToken } from 'services/api';
import { sharedApi, useGetSettingsQuery } from 'store/apiSlice';
import type { SyncProgressEvent, TokenDetailRecord } from 'types';
import type { AppDispatch, RootState } from '@live/store';
import {
  cacheSyncPreview,
  clearSyncOutput,
  mergeSyncOutput,
  setSelectedMint,
} from '@live/slices/syncTokenSlice';
import type { SyncResultItem } from '@live/slices/syncTokenSlice';
import { cn } from 'lib/cn';

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

/** A synced-tokens table row: a sync result, with its token record (null on failure). */
type SyncedRow = SyncResultItem & { token: TokenDetailRecord | null };

/** Stable row-key ref so a SOL/USD or live-trade tick doesn't break DataTable's
 *  row memo by handing it a fresh function identity each render. */
const syncedRowKey = (r: SyncedRow) => r.mint;

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
    {
      key: 'last_synced',
      label: 'Last synced',
      width: '96px',
      sortable: true,
      render: (r) => <RelativeTimeCell iso={r.token?.last_synced_at} />,
      sortValue: (r) => r.token?.last_synced_at ?? null,
      searchValue: (r) => r.token?.last_synced_at ?? '',
    },
  ];
}

/** A labeled sync progress bar: title on the left, detail (percent/count) on the right. */
function ProgressBar({
  label,
  detail,
  percent,
  message,
}: {
  label: string;
  detail: string;
  percent: number;
  message?: string;
}) {
  return (
    <div>
      <div className="mb-2 flex items-center justify-between gap-2">
        <span className="text-[11px] font-bold uppercase tracking-widest text-primary">
          {label}
        </span>
        <span className="font-mono text-[11px] text-text-dim">{detail}</span>
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
      {message && <p className="mt-2 text-xs text-text-dim">{message}</p>}
    </div>
  );
}

export function SyncTokenPage() {
  const price = usePriceDisplay();

  // Synced output lives in Redux so it survives navigation (the route unmounts
  // on leave). The in-flight sync state below stays local — it's tied to a
  // specific request and isn't meaningful to persist.
  const dispatch = useDispatch<AppDispatch>();
  const results = useSelector((s: RootState) => s.syncToken.results);
  const syncedTokens = useSelector((s: RootState) => s.syncToken.syncedTokens);
  const selectedMint = useSelector((s: RootState) => s.syncToken.selectedMint);
  const syncedMints = useSelector((s: RootState) => s.syncToken.syncedMints);

  const [mint, setMint] = useState('');
  const [includePostMigrate, setIncludePostMigrate] = useState(false);
  // True once the user toggles the checkbox, so the async global-default seed
  // below never clobbers an explicit choice.
  const postMigrateTouchedRef = useRef(false);
  const [syncing, setSyncing] = useState(false);
  const [progress, setProgress] = useState<SyncProgressEvent | null>(null);
  const [batch, setBatch] = useState<{ index: number; total: number } | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Bumped after every sync run so the input status panel re-reads freshness.
  const [syncNonce, setSyncNonce] = useState(0);
  const syncAbortRef = useRef<AbortController | null>(null);

  // Default the post-migration checkbox to the global tracking policy, unless
  // the user has already chosen. Overridable per-sync. Settings come from the
  // shared RTK Query cache (deduped with the rest of the app).
  const { data: settings } = useGetSettingsQuery();
  useEffect(() => {
    if (settings && !postMigrateTouchedRef.current) {
      setIncludePostMigrate(settings.track_post_migration);
    }
  }, [settings]);

  const mints = useMemo(() => parseMints(mint), [mint]);
  // How many entries the user typed were duplicates of an earlier one. parseMints
  // already drops them, so this is purely to let the user know they were ignored.
  const duplicateCount = useMemo(() => {
    const all = mint.split(/[\s,]+/).map((m) => m.trim()).filter(Boolean);
    return all.length - mints.length;
  }, [mint, mints]);

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

  // Per-token (inner) progress: how far the current token is through its stages.
  const percent = progress ? stagePercent(progress.stage, progress.current, progress.total) : 0;
  // Overall (outer) batch progress: how many tokens are done. `batch` is set at
  // the start of each token; fall back to the parsed mint count before it lands.
  const tokensTotal = batch?.total ?? mints.length;
  const tokensIndex = batch?.index ?? 0;
  // Fold the in-flight token's own fraction in so the bar advances smoothly
  // between tokens instead of stepping a whole notch at a time.
  const tokensPercent =
    tokensTotal > 0
      ? Math.min(100, ((tokensIndex + percent / 100) / tokensTotal) * 100)
      : 0;

  const handleCancelSync = useCallback(() => {
    syncAbortRef.current?.abort();
  }, []);

  // Refill the input with every mint synced this session so they can be
  // reviewed or re-synced (e.g. "Fetch New" to pull only fresh trades).
  const handleLoadSyncedMints = useCallback(() => {
    setMint(syncedMints.join('\n'));
  }, [syncedMints]);

  const handleClearSynced = useCallback(() => {
    dispatch(clearSyncOutput());
  }, [dispatch]);

  // Stable select handler so the synced-tokens DataTable's row memo holds across
  // SOL/USD + live-trade ticks (an inline arrow would re-create it every render).
  const handleSelectSynced = useCallback(
    (key: string | null) => {
      if (key && syncedTokens.some((t) => t.token.mint_address === key)) {
        dispatch(setSelectedMint(key));
      }
    },
    [syncedTokens, dispatch],
  );

  // Drop a single mint from the textarea (used by the input status table). Rebuilds
  // from the parsed, deduped list so the remaining mints stay one-per-line.
  const handleRemoveMint = useCallback((target: string) => {
    setMint((prev) => parseMints(prev).filter((m) => m !== target).join('\n'));
  }, []);

  // Bulk-remove (e.g. "Remove synced") — drop every listed mint from the textarea.
  const handleRemoveMints = useCallback((targets: string[]) => {
    const drop = new Set(targets);
    setMint((prev) => parseMints(prev).filter((m) => !drop.has(m)).join('\n'));
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
    setBatch(null);

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
          // Commit this token the moment it finishes (don't wait for the rest of
          // the batch) so its rows update live: the output table + chart pick it
          // up, the input status row shows the fresh last-synced time, and its
          // "To fetch" estimate drops to up-to-date (we just pulled everything).
          dispatch(
            mergeSyncOutput({
              results: [{ mint: target, ok: true }],
              syncedTokens: [{ token: result.token, trades: result.trades }],
            }),
          );
          dispatch(
            cacheSyncPreview({
              key: `${target}|${includePostMigrate}`,
              data: { newCount: 0, newCapped: false, totalCount: 0, totalCapped: false },
            }),
          );
          // Seed the shared trades cache with the freshly synced rows so
          // TokenTradeChart's own `useGetTokenTradesQuery` reflects this sync
          // immediately instead of showing a stale pre-sync cache entry.
          dispatch(sharedApi.util.upsertQueryData('getTokenTrades', target, result.trades));
        } catch (e) {
          if (e instanceof DOMException && e.name === 'AbortError') {
            throw e;
          }
          // Record the failure right away too, so its row flips to FAIL live.
          dispatch(
            mergeSyncOutput({
              results: [
                { mint: target, ok: false, error: e instanceof Error ? e.message : 'Sync failed' },
              ],
              syncedTokens: [],
            }),
          );
        }
      }
      setProgress(null);
    } catch (e) {
      if (e instanceof DOMException && e.name === 'AbortError') {
        // Tokens finished before the abort are already committed above.
        setProgress(null);
        return;
      }
      setError(e instanceof Error ? e.message : 'Sync failed');
    } finally {
      setSyncing(false);
      setBatch(null);
      // Refresh the input status table's DB-backed columns one last time.
      setSyncNonce((n) => n + 1);
      if (syncAbortRef.current === controller) {
        syncAbortRef.current = null;
      }
    }
  }, [mint, includePostMigrate, dispatch]);

  // "Fetch All" re-downloads every transaction from Helius even for data already
  // in the DB, so guard it behind a confirm to avoid accidental slow/costly runs.
  const handleFetchAll = useCallback(() => {
    const ok = window.confirm(
      `Fetch ALL transactions for ${mints.length} token${mints.length === 1 ? '' : 's'}?\n\n` +
      'This re-downloads the full history from Helius even for transactions already ' +
      'saved in the database, which can be slow and use significant RPC credits.\n\n' +
      'Use "Fetch New" for routine updates.',
    );
    if (ok) handleSync(false);
  }, [mints.length, handleSync]);

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
          <span className="flex items-center gap-2 text-[10px] font-bold uppercase tracking-widest text-text-dim">
            Mint address{mints.length > 1 ? ` (${mints.length})` : ''}
            {duplicateCount > 0 && (
              <span className="font-bold normal-case tracking-normal text-warning">
                {duplicateCount} duplicate{duplicateCount > 1 ? 's' : ''} ignored
              </span>
            )}
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
            onChange={(e) => {
              postMigrateTouchedRef.current = true;
              setIncludePostMigrate(e.target.checked);
            }}
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
            <Button
              variant="primary"
              onClick={() => handleSync(true)}
              disabled={mints.length === 0}
              title="Quick update: downloads only transactions newer than the last sync. Fast and cheap — use this for routine refreshes."
            >
              Fetch New
            </Button>
            <Button
              variant="ghost"
              onClick={handleFetchAll}
              disabled={mints.length === 0}
              title="Full re-sync: re-downloads the entire transaction history from Helius, even data already in the database. Slow and uses more RPC credits — only needed to rebuild from scratch."
            >
              Fetch All
            </Button>
          </>
        )}
      </div>

      {syncing && (
        <div className="mb-4 space-y-4 rounded-lg border border-white/6 bg-white/2 p-4">
          {/* Outer bar: progress across the whole batch of tokens. */}
          <ProgressBar
            label={`Tokens ${tokensIndex + 1} / ${tokensTotal}`}
            detail={`${Math.round(tokensPercent)}%`}
            percent={tokensPercent}
          />
          {/* Inner bar: the current token's tx-fetch / stage status. */}
          <ProgressBar
            label={progress ? stageLabel(progress.stage) : 'Starting…'}
            detail={
              progress && progress.stage === 'fetching_transactions' && progress.total > 0
                ? `${progress.current} / ${progress.total} tx`
                : `${Math.round(percent)}%`
            }
            percent={percent}
            message={progress?.message}
          />
        </div>
      )}

      <InputSyncStatus
        mints={mints}
        refreshSignal={syncNonce}
        includePostMigrate={includePostMigrate}
        onRemove={handleRemoveMint}
        onRemoveMany={handleRemoveMints}
      />

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
            <div className="ml-auto flex items-center gap-1">
              <Button
                variant="link"
                size="xs"
                onClick={handleLoadSyncedMints}
                disabled={syncing || syncedMints.length === 0}
                title="Put every mint synced this session back into the input"
              >
                Load {syncedMints.length} mint{syncedMints.length === 1 ? '' : 's'}
              </Button>
              <Button variant="link" size="xs" onClick={handleClearSynced} disabled={syncing}>
                Clear
              </Button>
            </div>
          </div>
          <DataTable
            tableId="sync_tokens"
            columns={syncedColumns}
            rows={syncedRows}
            rowKey={syncedRowKey}
            selectedKey={selectedMint}
            onSelect={handleSelectSynced}
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

          <TokenTradeChart key={selected.token.mint_address} tableId="sync_trades" detail={selected.token} />
        </>
      )}
    </div>
  );
}
