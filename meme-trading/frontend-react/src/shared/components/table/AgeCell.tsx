import { memo } from 'react';
import { ageClass, formatAge } from 'utils/format';
import { useNow } from 'hooks/useNow';

/**
 * Live-ticking token age, derived from a pre-parsed creation epoch-ms.
 *
 * Subscribes to the shared {@link useNow} clock so the age advances on its own
 * between polls — and only these cells re-render each tick; the rest of the
 * (memoized) row is untouched. The cadence is adaptive: a token under a minute
 * old ticks every second, anything older re-renders ~twice a minute (a 2-hour
 * token gains nothing from per-second updates). `formatAge` floors to whole
 * seconds, so the coarse cadence is invisible.
 */
function AgeCellInner({ createdMs }: { createdMs: number }) {
  const provisionalAge = (Date.now() - createdMs) / 1000;
  const now = useNow(provisionalAge < 60 ? 1000 : 30_000);
  const secs = Math.max(0, Math.floor((now - createdMs) / 1000));
  return <span className={ageClass(secs)}>{formatAge(secs)}</span>;
}

export const AgeCell = memo(AgeCellInner);
