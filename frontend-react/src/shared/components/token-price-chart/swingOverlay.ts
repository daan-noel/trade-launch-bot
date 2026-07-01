import type { SeriesMarker, UTCTimestamp } from 'lightweight-charts';
import { bucketStart } from './chartBars';
import { CHART_COLORS, TOKEN_TOTAL_SUPPLY } from './constants';
import type { ChartGroupMode, ChartMetric, ChartSwingLeg, ChartTrade } from './types';

export type SwingOverlaySegmentMode = 'connected' | 'perLeg' | 'connectedSequential';

export function swingLegKey(leg: ChartSwingLeg): string {
  return `${leg.type}-${leg.start_at}-${leg.end_at}`;
}

/** Split a filtered leg list into chains that are consecutive in `allLegs`. */
export function groupSequentialLegChains(
  legs: ChartSwingLeg[],
  allLegs: ChartSwingLeg[],
): ChartSwingLeg[][] {
  if (!legs.length) return [];
  const indexOf = new Map<string, number>();
  for (let i = 0; i < allLegs.length; i++) {
    indexOf.set(swingLegKey(allLegs[i]), i);
  }

  const chains: ChartSwingLeg[][] = [];
  let current: ChartSwingLeg[] = [];
  let prevIdx: number | null = null;

  for (const leg of legs) {
    const idx = indexOf.get(swingLegKey(leg));
    if (idx == null) continue;
    if (current.length === 0 || (prevIdx != null && idx === prevIdx + 1)) {
      current.push(leg);
    } else {
      chains.push(current);
      current = [leg];
    }
    prevIdx = idx;
  }
  if (current.length) chains.push(current);
  return chains;
}

export type SwingColoredLinePoint =
  | {
      time: UTCTimestamp;
      value: number;
      /** Styles the segment from this point to the next (lightweight-charts forward color). */
      color?: string;
    }
  | { time: UTCTimestamp };

type SwingVertex = { time: UTCTimestamp; value: number };

function swingLegColor(type: ChartSwingLeg['type']): string {
  return type === 'swing_high' ? CHART_COLORS.swingHigh : CHART_COLORS.swingLow;
}

/** Leg end point used for the overlay line: the terminal pivot (last big tx /
 *  price extreme) when present, else the full-leg end. `end_at`/`end_price` stay
 *  the leg's full span and still drive candle highlighting + bar ranges. */
function legEndMs(leg: ChartSwingLeg): number {
  return leg.pivot_end_at ?? leg.end_at;
}
function legEndPrice(leg: ChartSwingLeg): number {
  return leg.pivot_end_price ?? leg.end_price;
}

function swingPriceToChartY(
  priceInSol: number,
  metric: ChartMetric,
  toValue: (sol: number) => number,
): number {
  const solValue = metric === 'price' ? priceInSol : TOKEN_TOTAL_SUPPLY * priceInSol;
  return toValue(solValue);
}

function nearestTradeForMs(trades: ChartTrade[], ms: number): ChartTrade | null {
  let best: ChartTrade | null = null;
  let bestDiff = Infinity;
  for (const trade of trades) {
    const tms = Date.parse(trade.block_time);
    if (Number.isNaN(tms)) continue;
    const diff = Math.abs(tms - ms);
    if (diff < bestDiff) {
      bestDiff = diff;
      best = trade;
    }
  }
  return best;
}

function resolveChartTime(
  ms: number,
  groupMode: ChartGroupMode,
  trades: ChartTrade[],
): UTCTimestamp | null {
  if (groupMode === 'time') {
    return Math.floor(ms / 1000) as UTCTimestamp;
  }
  const trade = nearestTradeForMs(trades, ms);
  if (trade?.slot == null) return null;
  return trade.slot as UTCTimestamp;
}

function pushVertex(
  vertices: SwingVertex[],
  time: UTCTimestamp | null,
  value: number,
  prevTime: number,
): number {
  if (time == null) return prevTime;
  let t = time as number;
  if (t <= prevTime) t = prevTime + 1;
  vertices.push({ time: t as UTCTimestamp, value });
  return t;
}

/**
 * Is the idle gap between leg `i`'s end and leg `i+1`'s start a "breakage" that
 * should split the line? `gapBreakMs <= 0`/`undefined` disables (legacy bridging).
 * The gap is measured end→start in real ms (the same units the dust trades live in),
 * so a dust-only or trade-less stretch longer than the threshold breaks the path.
 */
function isLegBreakage(
  prev: ChartSwingLeg,
  next: ChartSwingLeg,
  gapBreakMs: number | undefined,
): boolean {
  if (!gapBreakMs || gapBreakMs <= 0) return false;
  return next.start_at - legEndMs(prev) > gapBreakMs;
}

/** Connected reversal path — first leg start + each leg end. */
function buildSwingPathVertices(
  swings: ChartSwingLeg[],
  metric: ChartMetric,
  toValue: (sol: number) => number,
  groupMode: ChartGroupMode,
  trades: ChartTrade[],
): SwingVertex[] {
  const vertices: SwingVertex[] = [];
  let prevTime = -1;

  for (let i = 0; i < swings.length; i++) {
    const leg = swings[i];
    if (i === 0) {
      prevTime = pushVertex(
        vertices,
        resolveChartTime(leg.start_at, groupMode, trades),
        swingPriceToChartY(leg.start_price, metric, toValue),
        prevTime,
      );
    }
    prevTime = pushVertex(
      vertices,
      resolveChartTime(legEndMs(leg), groupMode, trades),
      swingPriceToChartY(legEndPrice(leg), metric, toValue),
      prevTime,
    );
  }

  return vertices;
}

function pushColoredVertex(
  out: SwingColoredLinePoint[],
  time: UTCTimestamp | null,
  value: number,
  prevTime: number,
  color?: string,
): number {
  if (time == null) return prevTime;
  let t = time as number;
  if (t <= prevTime) t = prevTime + 1;
  const point: SwingColoredLinePoint = {
    time: t as UTCTimestamp,
    value,
    ...(color ? { color } : {}),
  };
  out.push(point);
  return t;
}

export type SwingLegLineSegment = {
  color: string;
  data: SwingColoredLinePoint[];
};

/**
 * Resolve a [startAtMs, endAtMs] span to the chart-time range of the bars it
 * covers — bucketed bar starts in time mode, nearest-trade slots in slot mode.
 */
export function chartTimeRangeForSpan(
  startAtMs: number,
  endAtMs: number,
  groupMode: ChartGroupMode,
  trades: ChartTrade[],
  intervalSec: number,
): { lo: number; hi: number } | null {
  if (groupMode === 'slot') {
    const start = resolveChartTime(startAtMs, groupMode, trades);
    const end = resolveChartTime(endAtMs, groupMode, trades);
    if (start == null || end == null) return null;
    return {
      lo: Math.min(start as number, end as number),
      hi: Math.max(start as number, end as number),
    };
  }
  const startSec = Math.floor(startAtMs / 1000);
  const endSec = Math.floor(endAtMs / 1000);
  return {
    lo: bucketStart(Math.min(startSec, endSec), intervalSec),
    hi: bucketStart(Math.max(startSec, endSec), intervalSec),
  };
}

function swingLegChartTimeRange(
  leg: ChartSwingLeg,
  groupMode: ChartGroupMode,
  trades: ChartTrade[],
  intervalSec: number,
): { lo: number; hi: number } | null {
  return chartTimeRangeForSpan(leg.start_at, leg.end_at, groupMode, trades, intervalSec);
}

/** Start/end arrow markers for a selected swing leg (same style as bar selection). */
export function swingLegSelectionMarkers(
  leg: ChartSwingLeg,
  bars: { time: UTCTimestamp }[],
  groupMode: ChartGroupMode,
  trades: ChartTrade[],
  intervalSec: number,
  markerColor: string = CHART_COLORS.barSelected,
): SeriesMarker<UTCTimestamp>[] {
  const legBarTimes = swingLegBarTimes(leg, bars, groupMode, trades, intervalSec);
  if (!legBarTimes.length) return [];

  const barTimeSet = new Set(legBarTimes.map((t) => t as number));
  const rising = leg.end_price >= leg.start_price;
  const shape = rising ? 'arrowUp' : 'arrowDown';
  const position = rising ? 'belowBar' : 'aboveBar';

  const markerTimes =
    legBarTimes.length === 1
      ? legBarTimes
      : [legBarTimes[0], legBarTimes[legBarTimes.length - 1]];

  return markerTimes
    .filter((time) => barTimeSet.has(time as number))
    .map((time) => ({
      time,
      position,
      color: markerColor,
      shape,
      size: 2,
    }));
}

/** OHLC bar times covered by a swing leg (for candle border highlight). */
export function swingLegBarTimes(
  leg: ChartSwingLeg,
  bars: { time: UTCTimestamp }[],
  groupMode: ChartGroupMode,
  trades: ChartTrade[],
  intervalSec: number,
): UTCTimestamp[] {
  const range = swingLegChartTimeRange(leg, groupMode, trades, intervalSec);
  if (!range) return [];
  return bars
    .filter((b) => (b.time as number) >= range.lo && (b.time as number) <= range.hi)
    .map((b) => b.time);
}

export function buildLegSegment(
  leg: ChartSwingLeg,
  metric: ChartMetric,
  toValue: (sol: number) => number,
  groupMode: ChartGroupMode,
  trades: ChartTrade[],
  /** End the segment at the leg's full-span `end_at`/`end_price` instead of its
   *  terminal pivot — aligns the line with the full-span candle highlight. */
  useFullSpanEnd = false,
): SwingLegLineSegment | null {
  const color = swingLegColor(leg.type);
  const data: SwingColoredLinePoint[] = [];
  let prevTime = -1;
  prevTime = pushColoredVertex(
    data,
    resolveChartTime(leg.start_at, groupMode, trades),
    swingPriceToChartY(leg.start_price, metric, toValue),
    prevTime,
    color,
  );
  const endMs = useFullSpanEnd ? leg.end_at : legEndMs(leg);
  const endPrice = useFullSpanEnd ? leg.end_price : legEndPrice(leg);
  prevTime = pushColoredVertex(
    data,
    resolveChartTime(endMs, groupMode, trades),
    swingPriceToChartY(endPrice, metric, toValue),
    prevTime,
  );
  if (data.filter((p) => 'value' in p).length < 2) return null;
  return { color, data };
}

/** One isolated line segment per leg (start → end), for filtered swing results. */
export function swingsToLegSegments(
  swings: ChartSwingLeg[],
  metric: ChartMetric,
  toValue: (sol: number) => number,
  groupMode: ChartGroupMode,
  trades: ChartTrade[],
): SwingLegLineSegment[] {
  const segments: SwingLegLineSegment[] = [];
  for (const leg of swings) {
    const segment = buildLegSegment(leg, metric, toValue, groupMode, trades);
    if (segment) segments.push(segment);
  }
  return segments;
}

/** Connected swing path with per-segment colors (full detection result).
 *
 * `gapBreakMs` (when > 0) splits the line at a breakage: if a leg starts more than
 * `gapBreakMs` after the previous leg ended, a whitespace point is emitted at the
 * new leg's start so lightweight-charts draws a visible gap instead of a diagonal
 * bridging the breakage. The low still ends at its last real defining-side trade
 * and the next high still starts at its first real one — the dust gap between them
 * is simply not connected. */
export function swingsToColoredLineData(
  swings: ChartSwingLeg[],
  metric: ChartMetric,
  toValue: (sol: number) => number,
  groupMode: ChartGroupMode,
  trades: ChartTrade[],
  gapBreakMs?: number,
): SwingColoredLinePoint[] {
  const out: SwingColoredLinePoint[] = [];
  let prevTime = -1;

  for (let i = 0; i < swings.length; i++) {
    const leg = swings[i];
    const startTime = resolveChartTime(leg.start_at, groupMode, trades);
    const startVal = swingPriceToChartY(leg.start_price, metric, toValue);
    const endTime = resolveChartTime(legEndMs(leg), groupMode, trades);
    const endVal = swingPriceToChartY(legEndPrice(leg), metric, toValue);
    // Forward-color convention: a point styles the segment to the NEXT point, so
    // the vertex that OPENS a leg's segment carries that leg's color.
    const color = swingLegColor(leg.type);

    if (i === 0) {
      // Open the path at the first leg's start — colors the leg0 segment.
      prevTime = pushColoredVertex(out, startTime, startVal, prevTime, color);
    } else if (startTime != null && isLegBreakage(swings[i - 1], leg, gapBreakMs)) {
      // Breakage: emit a whitespace gap, then re-anchor at THIS leg's own start
      // (its first real defining-side trade) so the segment never bridges the gap.
      // The re-anchor point opens this leg's segment, so it carries this leg's color.
      // Both points must be strictly after `prevTime` (lightweight-charts requires
      // ascending, unique times): the whitespace sits one tick past the prior point,
      // and the value point one tick past the whitespace (or at its real start time
      // if that's already later).
      const breakTime = (prevTime + 1) as UTCTimestamp;
      out.push({ time: breakTime }); // whitespace → line break
      prevTime = breakTime as number;
      prevTime = pushColoredVertex(out, startTime, startVal, prevTime, color);
    } else {
      // No breakage: the previous leg's end vertex IS this leg's opening point;
      // recolor it forward to this leg's color so the upcoming segment is styled
      // correctly (matches the legacy connected path).
      const last = out[out.length - 1];
      if (last && 'value' in last) last.color = color;
    }
    // This leg's end vertex — its forward color is set by the NEXT iteration (or
    // left uncolored for the final leg, as in the legacy path).
    prevTime = pushColoredVertex(out, endTime, endVal, prevTime);
  }

  return out;
}

export type SwingOverlayInteractionMeta = {
  segmentMode: SwingOverlaySegmentMode;
  groupMode: ChartGroupMode;
  legs: ChartSwingLeg[];
  allLegs?: ChartSwingLeg[];
};

export type ResolveSwingLegOptions = {
  /** When true, only match while the pointer is on a swing overlay series (not by time alone). */
  requireSwingSeries?: boolean;
};

/** Resolve swing leg under crosshair or click (per-leg series or connected path + time). */
export function resolveSwingLegAtChartInteraction(
  hoveredSeries: unknown,
  seriesData: ReadonlyMap<unknown, unknown> | undefined,
  chartTime: number | undefined,
  swingSeriesRefs: readonly unknown[],
  swingSeriesLegRef: ReadonlyMap<unknown, ChartSwingLeg>,
  meta: SwingOverlayInteractionMeta | null,
  trades: ChartTrade[],
  options?: ResolveSwingLegOptions,
): ChartSwingLeg | undefined {
  if (!meta?.legs.length || !swingSeriesRefs.length) return undefined;

  const onSwingSeries =
    hoveredSeries != null && swingSeriesRefs.includes(hoveredSeries);
  if (options?.requireSwingSeries) {
    if (!onSwingSeries) return undefined;
  } else if (!onSwingSeries && chartTime == null) {
    return undefined;
  }

  let leg =
    hoveredSeries != null
      ? swingSeriesLegRef.get(hoveredSeries)
      : undefined;

  if (!leg && meta.legs.length) {
    let resolvedTime = chartTime;
    if (resolvedTime == null && hoveredSeries != null && seriesData) {
      const point = seriesData.get(hoveredSeries);
      if (point && typeof point === 'object' && 'time' in point) {
        resolvedTime = point.time as number;
      }
    }
    if (resolvedTime != null) {
      const idx = findSwingLegIndexAtChartTime(
        meta.legs,
        resolvedTime,
        meta.groupMode,
        trades,
        meta.segmentMode,
        meta.allLegs,
      );
      if (idx != null) leg = meta.legs[idx];
    }
  }

  return leg;
}

/** Which swing leg contains this chart time (for crosshair tooltips). */
export function findSwingLegIndexAtChartTime(
  swings: ChartSwingLeg[],
  chartTime: number,
  groupMode: ChartGroupMode,
  trades: ChartTrade[],
  segmentMode: SwingOverlaySegmentMode = 'connected',
  allLegs?: ChartSwingLeg[],
): number | null {
  if (!swings.length) return null;

  if (segmentMode === 'connectedSequential') {
    const chains = groupSequentialLegChains(swings, allLegs ?? swings);
    for (const chain of chains) {
      const idx = findSwingLegIndexAtChartTime(
        chain,
        chartTime,
        groupMode,
        trades,
        'connected',
      );
      if (idx != null) {
        const leg = chain[idx];
        const key = swingLegKey(leg);
        const globalIdx = swings.findIndex((s) => swingLegKey(s) === key);
        if (globalIdx >= 0) return globalIdx;
      }
    }
    return null;
  }

  if (segmentMode === 'perLeg') {
    for (let i = 0; i < swings.length; i++) {
      const start = resolveChartTime(swings[i].start_at, groupMode, trades);
      const end = resolveChartTime(legEndMs(swings[i]), groupMode, trades);
      if (start == null || end == null) continue;
      const lo = Math.min(start as number, end as number);
      const hi = Math.max(start as number, end as number);
      if (chartTime >= lo && chartTime <= hi) return i;
    }
    return null;
  }

  const vertices = buildSwingPathVertices(swings, 'price', (v) => v, groupMode, trades);
  for (let i = 0; i < swings.length; i++) {
    const start = vertices[i]?.time;
    const end = vertices[i + 1]?.time;
    if (start == null || end == null) continue;
    const lo = Math.min(start as number, end as number);
    const hi = Math.max(start as number, end as number);
    if (chartTime >= lo && chartTime <= hi) return i;
  }
  return null;
}

/** Piecewise swing path — one vertex per reversal (first leg start + each leg end). */
export function swingsToLineData(
  swings: ChartSwingLeg[],
  metric: ChartMetric,
  toValue: (sol: number) => number,
  groupMode: ChartGroupMode,
  trades: ChartTrade[],
): SwingVertex[] {
  return buildSwingPathVertices(swings, metric, toValue, groupMode, trades);
}
