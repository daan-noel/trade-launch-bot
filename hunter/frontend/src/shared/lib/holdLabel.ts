import { formatDurationShort } from 'utils/format';

/**
 * A position's `entry_time` → `exit_time` span as a compact label ("2m 14s").
 * A row with no exit yet measures to now; a missing/!finite/inverted pair reads
 * as no label rather than a bogus duration.
 *
 * ONE definition — the Console History "Held" column and every position modal
 * read it from here, so a closed row's hold can't render two different ways.
 * The live-ticking counterpart is `components/table/ElapsedCell`, which is for
 * spans that are still running; a closed span is static, so it stays a string
 * and costs no clock subscription.
 */
export function holdLabel(
  entryTime: string | null | undefined,
  exitTime: string | null | undefined,
): string | null {
  if (!entryTime) return null;
  const start = Date.parse(entryTime);
  const end = exitTime ? Date.parse(exitTime) : Date.now();
  if (!Number.isFinite(start) || !Number.isFinite(end) || end < start) return null;
  return formatDurationShort((end - start) / 1000);
}
