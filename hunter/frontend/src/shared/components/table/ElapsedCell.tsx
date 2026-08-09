import { memo } from 'react';
import { formatDurationShort } from 'utils/format';
import { useNow } from 'hooks/useNow';

/**
 * Live-ticking elapsed time since an epoch-ms mark — the "Age" / "Held" cell of
 * a still-open position row.
 *
 * The value counts up from the shared {@link useNow} clock, so the age advances
 * on its own instead of waiting for an SSE delta to re-render the page (a quiet
 * token would otherwise freeze its age). Memoized, and it is the ONLY part of the row
 * that subscribes: an open row re-renders just this cell. Cadence is adaptive
 * (per-second under a minute, ~2×/min past that), the same pattern as
 * {@link AgeCell}.
 *
 * A **closed** span is static, so it doesn't belong here — it would pay a clock
 * subscription per row to render a value that can never change. Use
 * `lib/holdLabel` for that.
 */
function ElapsedCellInner({
  startMs,
  className = 'tabular-nums text-text-dim',
  suffix,
  title,
}: {
  startMs: number | null | undefined;
  className?: string;
  /** Appended after the duration (e.g. a stale-feed marker). */
  suffix?: string;
  title?: string;
}) {
  const provisionalAge =
    startMs != null && Number.isFinite(startMs) ? (Date.now() - startMs) / 1000 : Infinity;
  const now = useNow(provisionalAge < 60 ? 1000 : 30_000);

  if (startMs == null || !Number.isFinite(startMs) || now < startMs) {
    return <span className="text-text-dim">—</span>;
  }
  return (
    <span className={className} title={title}>
      {formatDurationShort((now - startMs) / 1000)}
      {suffix}
    </span>
  );
}

export const ElapsedCell = memo(ElapsedCellInner);
