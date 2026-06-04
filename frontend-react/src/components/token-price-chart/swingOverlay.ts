import type { UTCTimestamp } from 'lightweight-charts';
import { CHART_COLORS, TOKEN_TOTAL_SUPPLY } from './constants';
import type { ChartGroupMode, ChartMetric, ChartSwingLeg, ChartTrade } from './types';

export interface SwingColoredLinePoint {
  time: UTCTimestamp;
  value: number;
  /** Styles the segment from this point to the next (lightweight-charts forward color). */
  color?: string;
}

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

/** Shared reversal vertices — one monotonic time axis for the full swing path. */
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

/** One connected line; per-point color alternates sky (swing_high) / magenta (swing_low). */
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
