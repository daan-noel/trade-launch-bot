import { tradeBarSlot, tradeBarTime } from './chartBars';
import type { ChartBarSelection, ChartRangeSelectionDetail, ChartTrade } from './types';

/** The two fields a bar/range selection buckets a trade by. */
type BucketableTrade = Pick<ChartTrade, 'block_time' | 'slot'>;

/** Bar width assumed when a `time`-mode selection carries no interval. */
const DEFAULT_INTERVAL_SEC = 60;

/**
 * Trades inside a clicked bar, keyed exactly the way the chart buckets them
 * (`tradeBarTime` / slot number). This is the ONE matcher every selection panel
 * uses — a private copy drifts from the candle the user actually clicked and
 * silently lists the wrong trades.
 */
export function tradesInBar<T extends BucketableTrade>(
  trades: readonly T[],
  bar: ChartBarSelection,
): T[] {
  if (bar.groupMode === 'slot') {
    return trades.filter((t) => t.slot === bar.slot);
  }
  const intervalSec = bar.intervalSec ?? DEFAULT_INTERVAL_SEC;
  return trades.filter((t) => tradeBarTime(t.block_time, intervalSec) === bar.barTime);
}

/** Trades whose bar key falls inside the drag-selected range [lo, hi]. */
export function tradesInRange<T extends BucketableTrade>(
  trades: readonly T[],
  range: ChartRangeSelectionDetail,
): T[] {
  const lo = Math.min(range.lo, range.hi);
  const hi = Math.max(range.lo, range.hi);
  return trades.filter((t) => {
    const key =
      range.groupMode === 'slot'
        ? tradeBarSlot(t)
        : tradeBarTime(t.block_time, range.intervalSec);
    if (key == null) return false;
    const k = key as number;
    return k >= lo && k <= hi;
  });
}
