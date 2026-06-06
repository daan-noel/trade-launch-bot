import { useEffect, useState } from 'react';
import { fetchTokenDetail } from '../../services/api';
import { RelativeTimeCell } from '../table/RelativeTimeCell';
import { AddressDisplay } from '../ui/AddressDisplay';
import { Badge } from '../ui/Badge';
import { Button } from '../ui/Button';

type MintStatus =
  | { state: 'loading' }
  /** Mint not present in the database — a "Fetch All" would pull full history. */
  | { state: 'missing' }
  | { state: 'found'; symbol: string; lastSyncedAt: string | null };

/**
 * For every mint currently typed in the sync textarea, look up its database
 * record and show when it was last synced. Lets the user decide between
 * "Fetch New" (resume) and "Fetch All" (full re-pull) before kicking off a sync.
 *
 * `refreshSignal` is bumped by the parent after each sync run so the freshness
 * column updates without the user having to re-type the mints.
 */
export function InputSyncStatus({
  mints,
  refreshSignal,
  onRemove,
  onRemoveMany,
}: {
  mints: string[];
  refreshSignal: number;
  /** Remove a single mint from the textarea. */
  onRemove: (mint: string) => void;
  /** Remove several mints from the textarea at once (bulk actions). */
  onRemoveMany: (mints: string[]) => void;
}) {
  const [statuses, setStatuses] = useState<Record<string, MintStatus>>({});

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
              <th className="w-16 px-2 py-1.5" aria-label="Remove" />
            </tr>
          </thead>
          <tbody>
            {mints.map((mint) => {
              const s = statuses[mint];
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
