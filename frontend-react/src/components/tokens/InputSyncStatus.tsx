import { useEffect, useMemo, useRef, useState } from 'react';
import { useDispatch, useSelector } from 'react-redux';
import { fetchSyncPreview } from 'services/api';
import { RelativeTimeCell } from 'components/table/RelativeTimeCell';
import { AddressDisplay } from 'components/ui/AddressDisplay';
import { Badge } from 'components/ui/Badge';
import { Button } from 'components/ui/Button';
import type { AppDispatch, RootState } from '../../store';
import { apiSlice } from 'store/apiSlice';
import { cacheSyncPreview } from 'store/syncTokenSlice';

/**
 * Max DB-freshness lookups in flight at once. The input can hold hundreds of
 * mints; firing one fetch each (the old `Promise.all`) hammered the backend
 * with an unbounded burst. We drain the list through a small fixed pool of
 * workers instead.
 */
const FRESHNESS_CONCURRENCY = 5;

/**
 * Max "to fetch" preview counts in flight at once. These hit Helius (slow /
 * credit-costly), so a long mint list is drained through a small pool rather
 * than fired all at once.
 */
const PREVIEW_CONCURRENCY = 4;

type MintStatus =
  | { state: 'loading' }
  /** Mint not present in the database — a "Fetch All" would pull full history. */
  | { state: 'missing' }
  | { state: 'found'; symbol: string; lastSyncedAt: string | null };

/** Estimated transactions a sync would download — see {@link fetchSyncPreview}. */
type PreviewState =
  | { state: 'loading' }
  | { state: 'error' }
  | {
      state: 'loaded';
      newCount: number;
      newCapped: boolean;
      totalCount: number;
      totalCapped: boolean;
    };

/** Format a tx count, suffixing "+" when the backend hit its page cap. */
function fmtCount(n: number, capped: boolean): string {
  return capped ? `${n.toLocaleString()}+` : n.toLocaleString();
}

/**
 * For every mint currently typed in the sync textarea, look up its database
 * record (when it was last synced) and estimate how many transactions a sync
 * would download — "new" since the last sync vs. the "total" history. Lets the
 * user decide between "Fetch New" (resume) and "Fetch All" (full re-pull) before
 * kicking off a sync.
 *
 * `includePostMigrate` mirrors the page checkbox so the estimate counts AMM pool
 * transactions only when a real sync would. `refreshSignal` is bumped by the
 * parent after each sync run so both columns refresh without re-typing the mints.
 */
export function InputSyncStatus({
  mints,
  refreshSignal,
  includePostMigrate,
  onRemove,
  onRemoveMany,
}: {
  mints: string[];
  refreshSignal: number;
  /** Whether a sync would include post-migration (AMM) trades. */
  includePostMigrate: boolean;
  /** Remove a single mint from the textarea. */
  onRemove: (mint: string) => void;
  /** Remove several mints from the textarea at once (bulk actions). */
  onRemoveMany: (mints: string[]) => void;
}) {
  const dispatch = useDispatch<AppDispatch>();
  const [statuses, setStatuses] = useState<Record<string, MintStatus>>({});

  // Tokens synced this session carry their just-updated record, so a mint that
  // finishes in a running batch shows its fresh symbol/last-synced time here
  // immediately — ahead of (and overriding) the slower DB freshness fetch below.
  const syncedTokens = useSelector((s: RootState) => s.syncToken.syncedTokens);
  const syncedByMint = useMemo(() => {
    const out: Record<string, MintStatus> = {};
    for (const t of syncedTokens) {
      out[t.token.mint_address] = {
        state: 'found',
        symbol: t.token.symbol,
        lastSyncedAt: t.token.last_synced_at,
      };
    }
    return out;
  }, [syncedTokens]);
  const statusOf = (mint: string): MintStatus | undefined =>
    syncedByMint[mint] ?? statuses[mint];

  // Cached "to fetch" estimates live in Redux (keyed by `${mint}|${flag}`) so
  // they survive navigation away from the page and aren't re-counted on every
  // re-render. `pending` holds only the transient loading/error states for
  // previews not yet in the cache.
  const previewCache = useSelector((s: RootState) => s.syncToken.syncPreviews);
  const [pending, setPending] = useState<
    Record<string, { state: 'loading' } | { state: 'error' }>
  >({});
  // Latest cache, read inside the (debounced) fetch effect without making the
  // effect re-run on every cache write.
  const cacheRef = useRef(previewCache);
  cacheRef.current = previewCache;
  // Cache keys currently being fetched, so a re-render mid-flight doesn't fire a
  // duplicate request for the same mint.
  const inFlight = useRef<Set<string>>(new Set());

  // Database freshness (cheap, no RPC) — populates the Symbol/Status/Last synced
  // columns. Kept independent of the preview so the panel renders instantly even
  // if Helius is slow.
  useEffect(() => {
    if (mints.length === 0) {
      setStatuses({});
      return;
    }
    let cancelled = false;
    // Debounce so pasting / typing a list doesn't fire a request per keystroke.
    const handle = setTimeout(() => {
      // Keep known rows, mark the rest as loading so the table doesn't flicker.
      setStatuses((prev) => {
        const next: Record<string, MintStatus> = {};
        for (const mint of mints) next[mint] = prev[mint] ?? { state: 'loading' };
        return next;
      });
      // Drain the mint list through a bounded pool of workers (instead of one
      // request per mint at once). Each lookup goes through the shared RTK
      // `getTokenDetail` cache — deduped with the Tokens/TPSL detail views and
      // retained, so re-typing or revisiting a mint resolves without a refetch
      // — and commits its row as it lands so the table fills in progressively.
      const queue = mints.slice();
      const worker = async () => {
        for (;;) {
          const mint = queue.shift();
          if (mint === undefined || cancelled) return;
          let status: MintStatus;
          try {
            const sub = dispatch(apiSlice.endpoints.getTokenDetail.initiate(mint));
            try {
              const d = await sub.unwrap();
              status = { state: 'found', symbol: d.symbol, lastSyncedAt: d.last_synced_at };
            } finally {
              sub.unsubscribe();
            }
          } catch {
            status = { state: 'missing' };
          }
          if (cancelled) return;
          setStatuses((prev) => ({ ...prev, [mint]: status }));
        }
      };
      const pool = Math.min(FRESHNESS_CONCURRENCY, queue.length);
      for (let i = 0; i < pool; i++) void worker();
    }, 400);
    return () => {
      cancelled = true;
      clearTimeout(handle);
    };
  }, [mints, refreshSignal, dispatch]);

  // "To fetch" estimate (hits Helius to count signatures). Only fetches mints
  // whose count isn't already cached for the current post-migrate setting, so
  // adding a mint, toggling the setting back, or revisiting the page reuses
  // prior results instead of re-counting. `refreshSignal` triggers a re-fetch
  // after a sync — the parent invalidates the synced mints' cache entries first,
  // so they read as missing here and get re-counted.
  useEffect(() => {
    if (mints.length === 0) return;
    const handle = setTimeout(() => {
      const missing = mints.filter((m) => {
        const key = `${m}|${includePostMigrate}`;
        return !cacheRef.current[key] && !inFlight.current.has(key);
      });
      if (missing.length === 0) return;
      // Seed loading state for the mints we're about to fetch.
      setPending((prev) => {
        const next = { ...prev };
        for (const m of missing) next[`${m}|${includePostMigrate}`] = { state: 'loading' };
        return next;
      });
      // Drain the missing mints through a bounded pool, committing each result
      // the moment it resolves so a row's count shows as soon as it's fetched
      // instead of waiting for the whole batch — without firing every Helius
      // count request at once.
      const queue = missing.slice();
      const worker = async () => {
        for (;;) {
          const m = queue.shift();
          if (m === undefined) return;
          const key = `${m}|${includePostMigrate}`;
          inFlight.current.add(key);
          try {
            const p = await fetchSyncPreview(m, includePostMigrate);
            dispatch(
              cacheSyncPreview({
                key,
                data: {
                  newCount: p.new_count,
                  newCapped: p.new_capped,
                  totalCount: p.total_count,
                  totalCapped: p.total_capped,
                },
              }),
            );
            // Drop the loading marker — the cached value now drives the row.
            setPending((prev) => {
              const next = { ...prev };
              delete next[key];
              return next;
            });
          } catch {
            setPending((prev) => ({ ...prev, [key]: { state: 'error' } }));
          } finally {
            inFlight.current.delete(key);
          }
        }
      };
      const pool = Math.min(PREVIEW_CONCURRENCY, queue.length);
      for (let i = 0; i < pool; i++) void worker();
    }, 500);
    return () => clearTimeout(handle);
  }, [mints, refreshSignal, includePostMigrate, dispatch]);

  // Per-mint view the table and bulk-action filters read from: the cached count
  // (keyed by the current post-migrate setting) when present, else the transient
  // loading/error state for an in-flight fetch.
  const previews = useMemo<Record<string, PreviewState>>(() => {
    const out: Record<string, PreviewState> = {};
    for (const mint of mints) {
      const key = `${mint}|${includePostMigrate}`;
      const cached = previewCache[key];
      out[mint] = cached
        ? { state: 'loaded', ...cached }
        : pending[key] ?? { state: 'loading' };
    }
    return out;
  }, [mints, includePostMigrate, previewCache, pending]);

  if (mints.length === 0) return null;

  // Mints that already have a recorded sync — the "Remove synced" bulk action
  // strips these so only tokens still needing a sync are left in the input.
  const syncedMints = mints.filter((m) => {
    const s = statusOf(m);
    return s?.state === 'found' && s.lastSyncedAt != null;
  });

  // Mints whose preview reports nothing new to fetch ("up to date"). The
  // "Remove up to date" action strips these so only tokens with pending trades
  // remain — a tighter filter than "synced" (which keeps stale-but-synced ones).
  const upToDateMints = mints.filter((m) => {
    const p = previews[m];
    return p?.state === 'loaded' && p.newCount === 0;
  });

  // Preview fetch progress: how many input mints have a resolved "to fetch"
  // count (loaded from cache or errored) vs. still being counted on Helius.
  const previewDone = mints.filter((m) => previews[m]?.state !== 'loading').length;
  const previewing = previewDone < mints.length;
  const previewPercent = mints.length > 0 ? (previewDone / mints.length) * 100 : 0;

  return (
    <div className="mb-4 rounded-lg border border-white/6 bg-white/2 p-4">
      <div className="mb-2 flex items-center gap-2">
        <h3 className="text-sm font-bold text-text">Last synced</h3>
        <Badge variant="primary" className="font-mono">
          {mints.length}
        </Badge>
        <span className="min-w-0 flex-1 truncate text-[11px] text-text-dim">
          Freshness of each input mint — use Fetch New to resume, Fetch All to re-pull everything
        </span>
        <Button
          variant="link"
          size="xs"
          className="shrink-0"
          onClick={() => onRemoveMany(upToDateMints)}
          disabled={upToDateMints.length === 0}
          title="Remove every up-to-date token (nothing new to fetch) from the input, leaving only ones with pending trades"
        >
          Remove {upToDateMints.length} up to date
        </Button>
        <Button
          variant="link"
          size="xs"
          className="shrink-0"
          onClick={() => onRemoveMany(syncedMints)}
          disabled={syncedMints.length === 0}
          title="Remove every already-synced token from the input, leaving only ones still needing a sync"
        >
          Remove {syncedMints.length} synced
        </Button>
      </div>
      {previewing && (
        <div className="mb-2">
          <div className="mb-1 flex items-center justify-between gap-2">
            <span className="text-[10px] font-bold uppercase tracking-widest text-primary">
              Estimating to-fetch counts
            </span>
            <span className="font-mono text-[11px] text-text-dim">
              {previewDone} / {mints.length}
            </span>
          </div>
          <div className="h-1.5 overflow-hidden rounded-full bg-white/6">
            <div
              className="h-full animate-pulse rounded-full bg-primary transition-[width] duration-300"
              style={{ width: `${previewPercent}%` }}
            />
          </div>
        </div>
      )}
      <div className="overflow-hidden rounded-md border border-white/6">
        <table className="w-full text-[12px]">
          <thead>
            <tr className="bg-white/4 text-left text-[10px] uppercase tracking-widest text-text-dim">
              <th className="px-3 py-1.5 font-bold">Mint</th>
              <th className="px-3 py-1.5 font-bold">Symbol</th>
              <th className="px-3 py-1.5 font-bold">Status</th>
              <th className="px-3 py-1.5 font-bold">Last synced</th>
              <th className="px-3 py-1.5 font-bold" title="New transactions to fetch vs. total history">
                To fetch
              </th>
              <th className="w-16 px-2 py-1.5" aria-label="Remove" />
            </tr>
          </thead>
          <tbody>
            {mints.map((mint) => {
              const s = statusOf(mint);
              const p = previews[mint];
              return (
                <tr key={mint} className="border-t border-white/6">
                  <td className="px-3 py-1.5">
                    <AddressDisplay address={mint} kind="token" stopPropagation />
                  </td>
                  <td className="px-3 py-1.5 text-text-mid">
                    {s?.state === 'found' ? s.symbol || '-' : '-'}
                  </td>
                  <td className="px-3 py-1.5">
                    {!s || s.state === 'loading' ? (
                      <span className="text-[11px] text-text-dim">checking…</span>
                    ) : s.state === 'missing' ? (
                      <Badge variant="neutral" size="sm">
                        Not in DB
                      </Badge>
                    ) : s.lastSyncedAt ? (
                      <Badge variant="success" size="sm">
                        Synced
                      </Badge>
                    ) : (
                      <Badge variant="warning" size="sm">
                        Never
                      </Badge>
                    )}
                  </td>
                  <td className="px-3 py-1.5">
                    {s?.state === 'found' ? (
                      <RelativeTimeCell iso={s.lastSyncedAt} />
                    ) : (
                      <span className="text-[11px] text-text-dim">-</span>
                    )}
                  </td>
                  <td className="px-3 py-1.5">
                    {!p || p.state === 'loading' ? (
                      <span className="text-[11px] text-text-dim">…</span>
                    ) : p.state === 'error' ? (
                      <span
                        className="text-[11px] text-text-dim"
                        title="Couldn't estimate (invalid mint or RPC error)"
                      >
                        —
                      </span>
                    ) : p.newCount === 0 ? (
                      <Badge variant="neutral" size="sm">
                        up to date
                      </Badge>
                    ) : p.newCount >= p.totalCount ? (
                      // Nothing synced yet — the whole history is "new", so a Fetch
                      // would download everything. Flagged distinctly (no "/ total",
                      // since it's redundant) as a full first-time pull.
                      <Badge
                        variant="accent"
                        size="sm"
                        title="Never synced — Fetch All would download the entire history"
                      >
                        ↓ {fmtCount(p.newCount, p.newCapped)} all new
                      </Badge>
                    ) : (
                      // Partial top-up — only the trades since the last sync.
                      <span className="inline-flex items-center gap-1.5 whitespace-nowrap">
                        <Badge variant="primary" size="sm" title="Transactions Fetch New would download">
                          +{fmtCount(p.newCount, p.newCapped)} new
                        </Badge>
                        <span
                          className="text-[11px] text-text-dim"
                          title="Total transactions in history (Fetch All)"
                        >
                          / {fmtCount(p.totalCount, p.totalCapped)}
                        </span>
                      </span>
                    )}
                  </td>
                  <td className="px-2 py-1 text-right">
                    <button
                      type="button"
                      onClick={() => onRemove(mint)}
                      className="inline-flex h-7 w-12 items-center justify-center rounded-md text-[18px] leading-none text-text-dim opacity-70 transition-colors hover:bg-red/10 hover:text-red hover:opacity-100"
                      title="Remove from input"
                      aria-label={`Remove ${mint} from input`}
                    >
                      ✕
                    </button>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}
