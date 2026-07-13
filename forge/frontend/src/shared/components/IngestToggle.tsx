import clsx from 'clsx';
import { useIngestStatusQuery, useSetIngestMutation } from '@shared/store/endpoints';

/**
 * Runtime pause/resume for the LIVE box's Helius ingest stream (GET/PUT
 * /api/ingest). Polls every 5s so it reflects a toggle from another tab or a
 * watchdog auto-pause. Hidden when the box booted without Helius creds.
 */
export function IngestToggle() {
  const { data } = useIngestStatusQuery(undefined, {
    pollingInterval: 5000,
    skipPollingIfUnfocused: true,
  });
  const [setIngest, { isLoading }] = useSetIngestMutation();

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
