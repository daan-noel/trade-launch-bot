import { useTimezone } from 'context/TimezoneContext';
import { formatIso } from 'utils/date';
import { formatAge } from 'utils/format';
import { useNow } from 'hooks/useNow';

/**
 * Renders an ISO timestamp as compact relative time ("2h ago"), with the full
 * absolute timestamp shown on hover. Displays "-" when the value is null.
 *
 * Subscribes to the shared {@link useNow} clock so the relative value advances
 * on its own between polls — every second while it's under a minute old, then
 * ~twice a minute past that. Hooks run unconditionally (before the null/NaN
 * early-outs) to satisfy the rules of hooks.
 */
export function RelativeTimeCell({ iso }: { iso: string | null | undefined }) {
  const { timezone } = useTimezone();
  const ms = iso ? Date.parse(iso) : NaN;
  const provisionalAge = Number.isNaN(ms) ? Infinity : (Date.now() - ms) / 1000;
  const now = useNow(provisionalAge < 60 ? 1000 : 30_000);

  if (!iso) return <span className="text-[10px]">-</span>;
  if (Number.isNaN(ms)) return <span className="text-[11px]">{iso}</span>;
  const secs = Math.max(0, Math.floor((now - ms) / 1000));
  const rel = secs < 5 ? 'just now' : `${formatAge(secs)} ago`;
  return (
    <span className="whitespace-nowrap text-[11px]" title={formatIso(iso, timezone)}>
      {rel}
    </span>
  );
}
