import { useEffect, useState } from 'react';
import { fetchSyncPreview, fetchTokenDetail } from '../../services/api';
import { RelativeTimeCell } from '../table/RelativeTimeCell';
import { AddressDisplay } from '../ui/AddressDisplay';
import { Badge } from '../ui/Badge';
import { Button } from '../ui/Button';

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
  const [statuses, setStatuses] = useState<Record<string, MintStatus>>({});
  const [previews, setPreviews] = useState<Record<string, PreviewState>>({});

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
      void Promise.all(
        mints.map(async (mint): Promise<readonly [string, MintStatus]> => {
          try {
            const d = await fetchTokenDetail(mint);
            return [mint, { state: 'found', symbol: d.symbol, lastSyncedAt: d.last_synced_at }];
          } catch {
            return [mint, { state: 'missing' }];
          }
        }),
      ).then((entries) => {
        if (!cancelled) setStatuses(Object.fromEntries(entries));
      });
    }, 400);
    return () => {
      cancelled = true;
      clearTimeout(handle);
    };
  }, [mints, refreshSignal]);

  // "To fetch" estimate (hits Helius to count signatures). Re-runs when the
  // post-migrate toggle changes, since that decides whether AMM txs are counted.
  useEffect(() => {
    if (mints.length === 0) {
      setPreviews({});
      return;
    }
    let cancelled = false;
    const handle = setTimeout(() => {
      // Seed loading state, keeping any already-loaded row so it doesn't flicker
      // back to "…" while re-counting.
      setPreviews((prev) => {
        const next: Record<string, PreviewState> = {};
        for (const mint of mints) next[mint] = prev[mint] ?? { state: 'loading' };
        return next;
      });
      // Fire each request independently and commit its result the moment it
      // resolves, so a row's count shows as soon as it's fetched instead of
      // waiting for the whole batch to finish.
      for (const mint of mints) {
        void fetchSyncPreview(mint, includePostMigrate)
          .then(
            (p): PreviewState => ({
              state: 'loaded',
              newCount: p.new_count,
              newCapped: p.new_capped,
              totalCount: p.total_count,
              totalCapped: p.total_capped,
            }),
          )
          .catch((): PreviewState => ({ state: 'error' }))
          .then((result) => {
            if (!cancelled) setPreviews((prev) => ({ ...prev, [mint]: result }));
          });
      }
    }, 500);
    return () => {
      cancelled = true;
      clearTimeout(handle);
    };
  }, [mints, refreshSignal, includePostMigrate]);

  if (mints.length === 0) return null;

  // Mints that already have a recorded sync — the "Remove synced" bulk action
  // strips these so only tokens still needing a sync are left in the input.
  const syncedMints = mints.filter((m) => {
    const s = statuses[m];
    return s?.state === 'found' && s.lastSyncedAt != null;
  });

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
          onClick={() => onRemoveMany(syncedMints)}
          disabled={syncedMints.length === 0}
          title="Remove every already-synced token from the input, leaving only ones still needing a sync"
        >
          Remove {syncedMints.length} synced
        </Button>
      </div>
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
              const s = statuses[mint];
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
                    ) : (
                      <span className="inline-flex items-center gap-1.5 whitespace-nowrap">
                        {p.newCount > 0 ? (
                          <Badge variant="primary" size="sm" title="Transactions Fetch New would download">
                            +{fmtCount(p.newCount, p.newCapped)} new
                          </Badge>
                        ) : (
                          <Badge variant="neutral" size="sm">
                            up to date
                          </Badge>
                        )}
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
