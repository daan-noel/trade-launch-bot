import type { ITimeScaleApi, LogicalRange, Time, UTCTimestamp } from 'lightweight-charts';

/**
 * Saved zoom/pan, expressed the only way that survives a `setData`: as a LOGICAL
 * range plus the bar-array shape it was measured against.
 *
 * Every live trade re-runs `setData` with a whole new bar array, so restoring the
 * viewport means answering "where did the bars the user was looking at move to?".
 * Logical indices answer that exactly once the array shift is known; restoring by
 * TIME does not — `timeScale.setVisibleRange` snaps the endpoints onto bar
 * boundaries, so a tight zoom drifted a little on every single trade until it no
 * longer looked like the window the user set.
 */
export type ChartViewport = {
  logical: LogicalRange;
  shape: BarsShape | null;
};

/** Identity of a rendered bar array — enough to locate it inside the next one. */
export type BarsShape = {
  length: number;
  first: UTCTimestamp;
  last: UTCTimestamp;
};

/** The right edge counts as "following the live bar" within this many bars, so a
 *  user parked at the edge keeps following new trades instead of being pinned. */
const LIVE_EDGE_SLACK_BARS = 1;

export function barsShape(bars: readonly { time: UTCTimestamp }[]): BarsShape | null {
  if (bars.length === 0) return null;
  return {
    length: bars.length,
    first: bars[0].time,
    last: bars[bars.length - 1].time,
  };
}

export function barsShapeEq(a: BarsShape | null, b: BarsShape | null): boolean {
  if (a == null || b == null) return a === b;
  return a.length === b.length && a.first === b.first && a.last === b.last;
}

/** First index whose time is >= `time` (bars are sorted ascending). */
function lowerBound(bars: readonly { time: UTCTimestamp }[], time: number): number {
  let lo = 0;
  let hi = bars.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (Number(bars[mid].time) < time) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

/**
 * Translate a saved logical range onto the new bar array.
 *
 * The shift is anchored on the LAST bar of the old array, not the first: bars are
 * appended on the right by live trades but can also be dropped from the left (a
 * rolling window, `trimEmptyBars`), and only the last-bar anchor gets that second
 * case right — the first-bar anchor cannot tell "two bars trimmed off the front"
 * from "no change" and would silently slide the window by those two bars.
 */
export function shiftLogicalRange(
  logical: LogicalRange,
  prev: BarsShape | null,
  next: readonly { time: UTCTimestamp }[],
): LogicalRange {
  if (prev == null || next.length === 0) return logical;
  const shift = lowerBound(next, Number(prev.last)) - (prev.length - 1);
  const appended = next.length - (prev.length + shift);
  // Only follow the new bars when the user was already watching the live edge.
  const atLiveEdge = Number(logical.to) >= prev.length - 1 - LIVE_EDGE_SLACK_BARS;
  const delta = shift + (atLiveEdge && appended > 0 ? appended : 0);
  if (delta === 0) return logical;
  return {
    from: (Number(logical.from) + delta) as LogicalRange['from'],
    to: (Number(logical.to) + delta) as LogicalRange['to'],
  };
}

export function captureChartViewport(
  logical: LogicalRange,
  shape: BarsShape | null,
): ChartViewport {
  return { logical, shape };
}

export function restoreChartViewport(
  ts: ITimeScaleApi<Time>,
  viewport: ChartViewport | null,
  bars: readonly { time: UTCTimestamp }[],
): void {
  if (!viewport) return;
  ts.setVisibleLogicalRange(shiftLogicalRange(viewport.logical, viewport.shape, bars));
}

/** Re-apply after setData / overlay updates — LWC may adjust the range on the next frame. */
export function reapplyChartViewport(
  ts: ITimeScaleApi<Time>,
  viewport: ChartViewport | null,
  bars: readonly { time: UTCTimestamp }[],
): void {
  if (!viewport) return;
  restoreChartViewport(ts, viewport, bars);
  requestAnimationFrame(() => {
    restoreChartViewport(ts, viewport, bars);
  });
}
