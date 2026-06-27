import { memo } from 'react';
import { useTimezone } from 'context/TimezoneContext';
import { formatIsoLines } from 'utils/date';

/**
 * Renders an ISO timestamp as date + time (with seconds and ms) on two lines.
 *
 * `memo`-wrapped: the live-trade tables prepend each flush, which shifts every
 * visible row's `index` and re-renders the memoized `TableRow` even when the
 * underlying trade is unchanged. With only `iso` as input, an unchanged
 * timestamp skips re-formatting entirely on those index-only re-renders.
 */
export const DateCell = memo(function DateCell({ iso }: { iso: string | null | undefined }) {
  const { timezone } = useTimezone();
  if (!iso) return <span className="text-[10px]">-</span>;
  const { date, time } = formatIsoLines(iso, timezone);
  if (!time) {
    return <span className="text-[11px]">{date}</span>;
  }
  return (
    <span className="inline-flex flex-col items-center justify-center gap-0 leading-tight text-[11px]">
      <span className="whitespace-nowrap">{date}</span>
      <span className="whitespace-nowrap tabular-nums">{time}</span>
    </span>
  );
});
