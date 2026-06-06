import { useTimezone } from 'context/TimezoneContext';
import { formatIso } from 'utils/date';
import { formatAge } from 'utils/format';

/**
 * Renders an ISO timestamp as compact relative time ("2h ago"), with the full
 * absolute timestamp shown on hover. Displays "-" when the value is null.
 */
export function RelativeTimeCell({ iso }: { iso: string | null | undefined }) {
  const { timezone } = useTimezone();
  if (!iso) return <span className="text-[10px]">-</span>;
  const ms = Date.parse(iso);
  if (Number.isNaN(ms)) return <span className="text-[11px]">{iso}</span>;
  const secs = Math.max(0, Math.floor((Date.now() - ms) / 1000));
  const rel = secs < 5 ? 'just now' : `${formatAge(secs)} ago`;
  return (
    <span className="whitespace-nowrap text-[11px]" title={formatIso(iso, timezone)}>
      {rel}
    </span>
  );
}
