import { tradeBarSlot, tradeBarTime, tradeTimestampSec } from './chartBars';
import type {
  ChartBarSelection,
  ChartGroupMode,
  ChartRangeSelection,
  ChartRangeSelectionDetail,
  ChartTimeSpan,
  ChartTrade,
} from './types';

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

/**
 * The bar range a wall-clock span covers, keyed the way the chart buckets: bucket
 * starts in `time` mode, the slots of the trades inside the span in `slot` mode
 * (null when the span holds none). The inverse of {@link rangeSpan}.
 */
export function rangeForSpan(
  trades: readonly BucketableTrade[],
  span: ChartTimeSpan,
  groupMode: ChartGroupMode,
  intervalSec: number,
): ChartRangeSelection | null {
  if (groupMode === 'time') {
    const at = (sec: number) => Math.floor(sec / intervalSec) * intervalSec;
    return { lo: at(span.from), hi: at(span.to) };
  }
  let lo: number | null = null;
  let hi: number | null = null;
  for (const t of trades) {
    const sec = tradeTimestampSec(t.block_time);
    if (sec == null || t.slot == null || sec < span.from || sec > span.to) continue;
    lo = lo == null ? t.slot : Math.min(lo, t.slot);
    hi = hi == null ? t.slot : Math.max(hi, t.slot);
  }
  return lo == null || hi == null ? null : { lo, hi };
}

/** A committed range as a wall-clock span plus the last slot it holds, for a read
 *  of that stretch of tape. */
export interface RangeTapeSpan extends ChartTimeSpan {
  endSlot: number;
}

/**
 * The wall-clock span a drag-selected range covers: `[first bucket start, last
 * bucket end)` in `time` mode, the first to last trade time in `slot` mode; plus
 * the last slot inside it. Null when no trade sits at or before its end (nothing to
 * read). The inverse of {@link rangeForSpan}.
 */
export function rangeSpan(
  trades: readonly BucketableTrade[],
  range: ChartRangeSelectionDetail,
): RangeTapeSpan | null {
  const inRange = tradesInRange(trades, range);
  const secs = (ts: readonly BucketableTrade[]) =>
    ts.map((t) => tradeTimestampSec(t.block_time)).filter((n): n is number => n != null);
  let from: number;
  let to: number;
  if (range.groupMode === 'time') {
    const interval = range.intervalSec || DEFAULT_INTERVAL_SEC;
    from = Math.min(range.lo, range.hi);
    // The last bucket's end, as a closed bound: 1 ms short of the next bucket.
    to = Math.max(range.lo, range.hi) + interval - 0.001;
  } else {
    const s = secs(inRange);
    if (s.length === 0) return null;
    from = Math.min(...s);
    to = Math.max(...s);
  }
  let endSlot: number | null = null;
  for (const t of trades) {
    const sec = tradeTimestampSec(t.block_time);
    if (sec == null || t.slot == null || sec > to) continue;
    if (range.groupMode === 'slot' && t.slot > Math.max(range.lo, range.hi)) continue;
    endSlot = endSlot == null ? t.slot : Math.max(endSlot, t.slot);
  }
  return endSlot == null ? null : { from, to, endSlot };
}
