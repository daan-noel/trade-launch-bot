import { CHART_INTERVALS, CHART_INTERVAL_LABELS } from './constants';
import type { ChartInterval } from './types';

/** Narrowest candle (CSS px) the opening view accepts; a finer bucket is a sliver. */
const MIN_CANDLE_PX = 3;

/**
 * The finest interval whose bar count over the token's whole life fits the chart
 * width at {@link MIN_CANDLE_PX} per candle, so `fitContent` frames every trade
 * with readable candles. A life too long for even the coarsest interval gets it.
 * An unknown width or span keeps the finest interval.
 */
export function pickAutoInterval(spanSec: number, widthPx: number): ChartInterval {
  const finest = CHART_INTERVAL_LABELS[0];
  if (!(spanSec > 0) || !(widthPx > 0)) return finest;
  const maxBars = Math.floor(widthPx / MIN_CANDLE_PX);
  const fit = CHART_INTERVAL_LABELS.find((key) => spanSec / CHART_INTERVALS[key] <= maxBars);
  return fit ?? CHART_INTERVAL_LABELS[CHART_INTERVAL_LABELS.length - 1];
}

/** Share of a token's prints (oldest first) the opening view frames; the sparse tail past it stays a scroll away. */
const FRAME_TRADE_SHARE = 0.99;

/**
 * The time the opening view ends at: the print at the {@link FRAME_TRADE_SHARE} rank.
 * A token that trades in a burst and then ticks along for hours would otherwise
 * squeeze the burst into a sliver to make room for a handful of stragglers.
 */
export function framedEndSec(sortedSecs: readonly number[]): number {
  if (sortedSecs.length === 0) return 0;
  return sortedSecs[Math.max(0, Math.ceil(sortedSecs.length * FRAME_TRADE_SHARE) - 1)];
}

/** Index of the last bar at or before `sec` in time-ascending `bars`; -1 when none. */
export function lastBarIndexAtOrBefore(bars: readonly { time: number }[], sec: number): number {
  let lo = 0;
  let hi = bars.length - 1;
  let found = -1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (bars[mid].time <= sec) {
      found = mid;
      lo = mid + 1;
    } else hi = mid - 1;
  }
  return found;
}
