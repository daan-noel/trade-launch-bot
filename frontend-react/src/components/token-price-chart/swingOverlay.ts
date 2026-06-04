import type { SeriesMarker, UTCTimestamp } from 'lightweight-charts';
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
      resolveChartTime(leg.end_at, groupMode, trades),
      swingPriceToChartY(leg.end_price, metric, toValue),
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

/** Start/end markers for a selected swing leg (price-anchored for line + candle charts). */
export function swingLegSelectionMarkers(
  segment: SwingLegLineSegment,
  markerColor: string,
): SeriesMarker<UTCTimestamp>[] {
  const vertices = segment.data.filter(
    (p): p is Extract<SwingColoredLinePoint, { value: number }> => 'value' in p,
  );
  if (!vertices.length) return [];

  const endpoint = (
    vtx: (typeof vertices)[number],
    label: 'S' | 'E',
  ): SeriesMarker<UTCTimestamp> => ({
    time: vtx.time,
    position: 'atPriceMiddle',
    price: vtx.value,
    color: markerColor,
    shape: 'square',
    size: 2,
    text: label,
  });

  if (vertices.length === 1 || vertices[0].time === vertices[vertices.length - 1].time) {
    return [endpoint(vertices[0], 'S')];
  }

  return [
    endpoint(vertices[0], 'S'),
    endpoint(vertices[vertices.length - 1], 'E'),
  ];
}

export function buildLegSegment(
  leg: ChartSwingLeg,
  metric: ChartMetric,
  toValue: (sol: number) => number,
  groupMode: ChartGroupMode,
  trades: ChartTrade[],
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
  prevTime = pushColoredVertex(
    data,
    resolveChartTime(leg.end_at, groupMode, trades),
    swingPriceToChartY(leg.end_price, metric, toValue),
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

/** Connected swing path with per-segment colors (full detection result). */
export function swingsToColoredLineData(
  swings: ChartSwingLeg[],
  metric: ChartMetric,
  toValue: (sol: number) => number,
  groupMode: ChartGroupMode,
  trades: ChartTrade[],
): SwingColoredLinePoint[] {
  const vertices = buildSwingPathVertices(swings, metric, toValue, groupMode, trades);

  return vertices.map((vertex, i) => {
    const point: SwingColoredLinePoint = { time: vertex.time, value: vertex.value };
    if (i < swings.length) {
      point.color = swingLegColor(swings[i].type);
    }
    return point;
  });
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
      const end = resolveChartTime(swings[i].end_at, groupMode, trades);
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
