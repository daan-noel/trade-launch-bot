import clsx from 'clsx';
import { useEffect } from 'react';
import { useDispatch } from 'react-redux';
import { api, useIngestStatusQuery, useSetIngestMutation } from '@shared/store/endpoints';
import { connectIngestStatusStream } from '@shared/services/sse';
import type { AppDispatch } from '@app/store';

/**
 * Runtime pause/resume for the LIVE box's Helius ingest stream (GET/PUT
 * /api/ingest). The badge flips instantly off the `ingest_status` SSE push
 * (audit §1); the 30s poll is now only a gap-heal fallback for a state change
 * that arrived while disconnected (e.g. a watchdog auto-pause). Hidden when the
 * box booted without Helius creds.
 */
export function IngestToggle() {
  const dispatch = useDispatch<AppDispatch>();
  const { data } = useIngestStatusQuery(undefined, {
    pollingInterval: 30_000,
    skipPollingIfUnfocused: true,
  });
  const [setIngest, { isLoading }] = useSetIngestMutation();

  // Patch the shared ingest cache in place when the server pushes a new state,
  // so every surface reading this query (header badge, dashboard tile) updates
  // at once without a refetch.
  useEffect(() => {
    const handle = connectIngestStatusStream((live) => {
      dispatch(
        api.util.updateQueryData('ingestStatus', undefined, (draft) => {
          draft.live = live;
        }),
      );
    });
    return () => handle.close();
  }, [dispatch]);

  if (!data || !data.configured) return null;

  return (
    <button
      type="button"
      disabled={isLoading}
      onClick={() => setIngest(!data.live)}
      title="Pause/resume the Helius ingest stream"
      className={clsx(
        'badge cursor-pointer',
        data.live ? 'badge-good' : 'badge-bad',
        isLoading && 'opacity-60',
      )}
    >
      <span className={clsx('h-1.5 w-1.5 rounded-full', data.live ? 'bg-[var(--color-good)]' : 'bg-[var(--color-bad)]')} />
      Ingest {data.live ? 'Live' : 'Paused'}
    </button>
  );
}
