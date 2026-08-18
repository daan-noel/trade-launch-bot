import type { CanvasRenderingTarget2D } from 'fancy-canvas';
import type {
  IChartApiBase,
  IPrimitivePaneRenderer,
  IPrimitivePaneView,
  ISeriesPrimitive,
  ISeriesPrimitiveBase,
  PrimitivePaneViewZOrder,
  SeriesAttachedParameter,
  UTCTimestamp,
} from 'lightweight-charts';

import { DEFAULT_BAR_SPACING } from './constants';

/**
 * Horizontal on/off lanes along the bottom of the price pane — "this was true from
 * here to here", stacked one row per thing being tracked.
 *
 * Deliberately vocabulary-free: a lane is a label and a list of spans. The chart is
 * shared by both apps, so it knows nothing about what a lane means; the live app's
 * rule conditions and anything the lab wants to overlay both arrive as the same shape.
 *
 * Drawn as a chart primitive rather than a strip under the canvas because the only
 * thing that makes a band readable is that it lines up with the candles above it.
 * In the pane, panning and zooming carry it for free; beside the pane, every zoom is
 * a chance to be subtly wrong, which is worse than not drawing it.
 */
export interface TimeBandSpan {
  from: UTCTimestamp;
  to: UTCTimestamp;
}

export interface TimeBandLane {
  key: string;
  label: string;
  /** Fill for the satisfied spans. The track behind them is drawn from this too. */
  color: string;
  spans: TimeBandSpan[];
}

const LANE_H = 9; // CSS px per lane at full size, label included
/** Lanes thin down to this before the band gives up — a visible thin lane beats a
 *  band that silently disappears once a rule authors one condition too many. */
const MIN_LANE_H = 3;
/** Below this a lane has no room for its label, so it draws as a bare bar. */
const LABEL_MIN_LANE_H = 8;
const LANE_GAP = 2;
const BOTTOM_PAD = 4;
const LABEL_PAD = 4;
const LABEL_FONT = '8px sans-serif';
/** A span shorter than this reads as nothing at all, so it is widened to it. */
const MIN_SPAN_W = 1.5;
/** The most of the price pane the band may ever take. */
const MAX_BAND_FRACTION = 0.45;

interface RenderedLane {
  label: string;
  color: string;
  /** Media-space x ranges, already clamped and widened. */
  rects: Array<[number, number]>;
}

class TimeBandsRenderer implements IPrimitivePaneRenderer {
  constructor(
    private readonly _lanes: RenderedLane[],
    private readonly _track: [number, number] | null,
  ) {}

  draw(target: CanvasRenderingTarget2D): void {
    if (!this._lanes.length) return;
    target.useMediaCoordinateSpace(({ context: ctx, mediaSize }) => {
      const laneH = fitLaneHeight(this._lanes.length, mediaSize.height);
      if (laneH == null) return;
      const total = this._lanes.length * (laneH + LANE_GAP);

      ctx.save();
      ctx.textBaseline = 'middle';
      ctx.font = LABEL_FONT;

      let top = mediaSize.height - BOTTOM_PAD - total;
      for (const lane of this._lanes) {
        // The track is what separates "never satisfied" from "no data here". Without
        // it an empty lane and an uncovered stretch of chart look identical.
        if (this._track) {
          ctx.fillStyle = 'rgba(255,255,255,0.05)';
          ctx.fillRect(this._track[0], top, this._track[1] - this._track[0], laneH);
        }
        ctx.fillStyle = lane.color;
        for (const [x1, x2] of lane.rects) ctx.fillRect(x1, top, x2 - x1, laneH);

        // Label rides on top of its own lane. Shadowed rather than boxed so it stays
        // legible over a filled span without punching a hole in the data. Dropped
        // once the lane is too thin for it — the chip strip still names them, in
        // this order.
        if (laneH >= LABEL_MIN_LANE_H) {
          ctx.save();
          ctx.shadowColor = 'rgba(0,0,0,0.9)';
          ctx.shadowBlur = 2;
          ctx.fillStyle = 'rgba(255,255,255,0.72)';
          ctx.fillText(lane.label, LABEL_PAD, top + laneH / 2);
          ctx.restore();
        }

        top += laneH + LANE_GAP;
      }
      ctx.restore();
    });
  }
}

class TimeBandsPaneView implements IPrimitivePaneView {
  constructor(
    private readonly _lanes: RenderedLane[],
    private readonly _track: [number, number] | null,
  ) {}

  /** Under the crosshair and markers — the band is context, not a reading. */
  zOrder(): PrimitivePaneViewZOrder {
    return 'normal';
  }

  renderer(): IPrimitivePaneRenderer {
    return new TimeBandsRenderer(this._lanes, this._track);
  }
}

export class TimeBandsPlugin
  implements ISeriesPrimitiveBase<SeriesAttachedParameter<UTCTimestamp>>
{
  private _chart: IChartApiBase<UTCTimestamp> | null = null;
  private _requestUpdate: (() => void) | null = null;
  private _lanes: TimeBandLane[] = [];
  private _coverage: TimeBandSpan | null = null;
  private _rendered: RenderedLane[] = [];
  private _track: [number, number] | null = null;

  attached({ chart, requestUpdate }: SeriesAttachedParameter<UTCTimestamp>): void {
    this._chart = chart;
    this._requestUpdate = requestUpdate;
  }

  detached(): void {
    this._chart = null;
    this._requestUpdate = null;
  }

  /**
   * `coverage` is the stretch the lanes actually speak for — usually narrower than
   * the chart, since a reconstruction can start after the first trade or stop before
   * the last one.
   */
  setLanes(lanes: TimeBandLane[], coverage: TimeBandSpan | null): void {
    this._lanes = lanes;
    this._coverage = coverage;
    this._requestUpdate?.();
  }

  updateAllViews(): void {
    const chart = this._chart;
    if (!chart || !this._lanes.length) {
      this._rendered = [];
      this._track = null;
      return;
    }
    const ts = chart.timeScale();
    const x = (t: UTCTimestamp) => ts.timeToCoordinate(t);
    // `timeToCoordinate` answers with a bar's CENTRE, so a span painted straight from
    // one coordinate to the other covers neither of its end bars fully — half a bar
    // is lost at each end. The span owns whole bars, so it is drawn edge to edge.
    const half = (ts.options().barSpacing ?? DEFAULT_BAR_SPACING) / 2;
    /** The x range of the bars `from..to`, edge to edge, never thinner than a tick. */
    const rect = (from: UTCTimestamp, to: UTCTimestamp): [number, number] | null => {
      const a = x(from);
      const b = x(to);
      // `timeToCoordinate` resolves only times the scale knows, which is why the
      // host snaps span ends to bar times before handing them over.
      if (a == null || b == null) return null;
      const x1 = a - half;
      return [x1, Math.max(b + half, x1 + MIN_SPAN_W)];
    };

    this._track = this._coverage
      ? rect(this._coverage.from, this._coverage.to)
      : null;

    this._rendered = this._lanes.map((lane) => {
      const rects: Array<[number, number]> = [];
      for (const s of lane.spans) {
        const r = rect(s.from, s.to);
        if (r) rects.push(r);
      }
      return { label: lane.label, color: lane.color, rects };
    });
  }

  paneViews(): readonly IPrimitivePaneView[] {
    return [new TimeBandsPaneView(this._rendered, this._track)];
  }
}

/** Same widening cast the other primitives use — `UTCTimestamp` is a `Time`. */
export function asTimeBandsPrimitive(p: TimeBandsPlugin): ISeriesPrimitive {
  return p as unknown as ISeriesPrimitive;
}

/**
 * Lane height that fits `count` lanes in the band's share of a pane `paneH` tall,
 * or `null` when even the thinnest lanes do not fit.
 *
 * Thinning rather than hiding: a rule with a long ladder would otherwise cross some
 * invisible threshold and lose its band entirely, which reads as "nothing was ever
 * satisfied" — the exact misreading the coverage track exists to prevent.
 */
export function fitLaneHeight(count: number, paneH: number): number | null {
  if (count <= 0) return null;
  const budget = paneH * MAX_BAND_FRACTION - BOTTOM_PAD;
  const laneH = Math.floor(budget / count) - LANE_GAP;
  if (laneH < MIN_LANE_H) return null;
  return Math.min(LANE_H, laneH);
}

export interface SnappedSpan {
  from: number;
  to: number;
}

/**
 * Move a wall-clock span onto the bars it covers, which are the only times the
 * chart's scale can turn into an x coordinate.
 *
 * `barEnds` holds each bar's wall-clock **end**, ascending (`buildBarWallEndSec`), so
 * the bar at index `i` covers `(barEnds[i-1], barEnds[i]]` and the bar holding an
 * instant `t` is the first one whose end is at or after `t`. A span therefore becomes
 * every bar it **overlaps**, with both edges rounding OUTWARD to the bar the instant
 * falls inside.
 *
 * Rounding the end edge inward instead — the last bar to have *finished* by `to` —
 * drops the bar the span ends in. That is a whole bar of understatement on every
 * span, which turns a two-bar satisfaction into a one-bar one and is invisible
 * precisely where the band matters most: a brief condition on a busy launch.
 *
 * A span shorter than one bar collapses onto the single bar containing it rather than
 * covering none, and the renderer widens that to a visible tick.
 */
export function snapSpanToBars(
  barEnds: number[],
  from: number,
  to: number,
): SnappedSpan | null {
  if (barEnds.length === 0) return null;
  const last = barEnds.length - 1;
  const lo = Math.min(lowerBound(barEnds, from), last);
  // `to >= from` for every span the callers build, but clamping to `lo` keeps a
  // reversed pair a degenerate one-bar span rather than an inverted rect.
  const hi = Math.max(Math.min(lowerBound(barEnds, to), last), lo);
  return { from: barEnds[lo], to: barEnds[hi] };
}

/** First index whose value is >= `t`, or `length` if there is none. */
function lowerBound(xs: number[], t: number): number {
  let lo = 0;
  let hi = xs.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (xs[mid] < t) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}
