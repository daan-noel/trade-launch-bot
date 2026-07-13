import { memo } from 'react';
import { formatAge } from '@shared/lib/format';
import { useNow } from '@shared/hooks/useNow';

/**
 * Live-ticking relative age for a table cell (ported from hunter's AgeCell).
 *
 * Subscribes to the shared {@link useNow} clock so the age advances on its own
 * between polls — and only these cells re-render each tick; the rest of the
 * (memoized) row is untouched. The cadence is adaptive: a row under a minute
 * old ticks every second, anything older re-renders ~twice a minute (an hours-
 * old row gains nothing from per-second updates). `formatAge` floors to whole
 * minutes past the first, so the coarse cadence is invisible.
 */
function AgeCellInner({ iso }: { iso: string | null | undefined }) {
  const parsedMs = iso ? new Date(iso).getTime() : NaN;
  const young = Number.isFinite(parsedMs) && Date.now() - parsedMs < 60_000;
  const now = useNow(young ? 1000 : 30_000);
  return <>{formatAge(iso, now)}</>;
}

export const AgeCell = memo(AgeCellInner);
