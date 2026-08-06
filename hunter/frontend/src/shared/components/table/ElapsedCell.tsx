import { memo } from 'react';
import { formatDurationShort } from 'utils/format';
import { useNow } from 'hooks/useNow';

/**
 * Live-ticking elapsed time between two epoch-ms marks — the "Age" / "Held"
 * cell of a position row.
 *
 * With `endMs` omitted the row is still open and the value counts up from the
 * shared {@link useNow} clock, so the age advances on its own instead of waiting
 * for an SSE delta to re-render the page (a quiet token used to freeze its age).
 * With `endMs` set the span is closed and fixed — the cell subscribes at an
 * hour's granularity, so the shared timer never re-renders it.
 *
 * Memoized, and it is the ONLY part of the row that subscribes: a table full of
 * closed positions costs nothing per tick, and an open one re-renders just this
 * cell. Cadence is adaptive (per-second under a minute, ~2×/min past that), the
 * same pattern as {@link AgeCell}.
 */
function ElapsedCellInner({
  startMs,
  endMs,
  className = 'tabular-nums text-text-dim',
  suffix,
  title,
  fallback = '—',
}: {
  startMs: number | null | undefined;
  /** Close mark; omit while the span is still running. */
  endMs?: number | null;
  className?: string;
  /** Appended after the duration (e.g. a stale-feed marker). */
  suffix?: string;
  title?: string;
  fallback?: string;
}) {
  const open = endMs == null;
  const provisionalAge =
    startMs != null && Number.isFinite(startMs) ? (Date.now() - startMs) / 1000 : Infinity;
  // A closed span never changes — subscribe coarsely so it costs no re-renders.
  const now = useNow(!open ? 3_600_000 : provisionalAge < 60 ? 1000 : 30_000);

  if (startMs == null || !Number.isFinite(startMs)) {
    return <span className="text-text-dim">{fallback}</span>;
  }
  const end = open ? now : endMs;
  if (end == null || !Number.isFinite(end) || end < startMs) {
    return <span className="text-text-dim">{fallback}</span>;
  }
  return (
    <span className={className} title={title}>
      {formatDurationShort((end - startMs) / 1000)}
      {suffix}
    </span>
  );
}

export const ElapsedCell = memo(ElapsedCellInner);
