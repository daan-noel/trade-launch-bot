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
import { lensLaneHeight } from './lensLanePlugin';
import type { LensBarTint } from './lensTint';

/**
 * Faint vertical washes behind the candles — "the thing you picked happened HERE".
 * Every wash draws at one alpha, so a layer is one flat color from bar to bar and
 * several armed layers stay apart by hue alone. The highlight lane below the candles
 * (`lensLanePlugin`) is what finds a hit at a glance; the wash only ties a lane
 * mark to the candle above it, so it stays faint and stops at the lane's top.
 *
 * Layers are drawn side by side rather than stacked: a candlestick carries exactly
 * one `borderColor`, so two layers cannot both express themselves through the
 * series. A bar's slot is split among the layers that hit THAT bar only, so a bar
 * one layer owns keeps the full width however many layers are armed, and a bar
 * where several hit shows each — the cell a reader is actually hunting for.
 *
 * Vocabulary-free like the rest of this folder: a layer is a color and a list of
 * (bar, trades). The chart never learns that one of them means "a wallet"
 * and another "an ix structure".
 */
export interface BarTintLayer {
  color: string;
  tints: readonly LensBarTint[];
}

export interface BarTintState {
  layers: readonly BarTintLayer[];
  /** Rows in the highlight lane under the candles — the washes stop above it. */
  laneRows: number;
}

export const EMPTY_BAR_TINTS: BarTintState = { layers: [], laneRows: 0 };

/** Fraction of a bar slot the washes of one bar span together. */
const FULL_WIDTH_RATIO = 0.9;
/** A wash narrower than this reads as a hairline artifact, so it is widened to it. */
const MIN_WIDTH = 1.5;
/** Every wash's alpha. Faint on purpose: the lane already says "a hit is here", the
 *  wash only has to point at the candle, and the candle and its wick stay readable
 *  through it. */
const WASH_ALPHA = 0.18;

interface RenderedWash {
  /** Media-space left edge. */
  x: number;
  width: number;
  alpha: number;
  color: string;
}

/** `#rrggbb` + alpha → `rgba(...)`. Any other notation is passed through with the
 *  alpha applied via globalAlpha instead, so a themed color can't break the wash. */
function washFill(color: string, alpha: number): string | null {
  const hex = /^#([0-9a-f]{6})$/i.exec(color);
  if (!hex) return null;
  const n = parseInt(hex[1], 16);
  return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${alpha})`;
}

class BarTintRenderer implements IPrimitivePaneRenderer {
  constructor(
    private readonly _washes: RenderedWash[],
    private readonly _laneRows: number,
  ) {}

  draw(target: CanvasRenderingTarget2D): void {
    if (this._washes.length === 0) return;
    target.useMediaCoordinateSpace(({ context: ctx, mediaSize }) => {
      const height = mediaSize.height - lensLaneHeight(this._laneRows, mediaSize.height);
      ctx.save();
      for (const w of this._washes) {
        const fill = washFill(w.color, w.alpha);
        if (fill) {
          ctx.globalAlpha = 1;
          ctx.fillStyle = fill;
        } else {
          ctx.globalAlpha = w.alpha;
          ctx.fillStyle = w.color;
        }
        ctx.fillRect(w.x, 0, w.width, height);
      }
      ctx.restore();
    });
  }
}

class BarTintPaneView implements IPrimitivePaneView {
  constructor(
    private readonly _washes: RenderedWash[],
    private readonly _laneRows: number,
  ) {}

  /** Behind the candles — this is a background wash, not an overlay. */
  zOrder(): PrimitivePaneViewZOrder {
    return 'bottom';
  }

  renderer(): IPrimitivePaneRenderer {
    return new BarTintRenderer(this._washes, this._laneRows);
  }
}

export class BarTintPlugin
  implements ISeriesPrimitiveBase<SeriesAttachedParameter<UTCTimestamp>>
{
  private _chart: IChartApiBase<UTCTimestamp> | null = null;
  private _requestUpdate: (() => void) | null = null;
  private _laneRows = 0;
  /** Bar key → the colors of the layers hitting it, in layer order. Built once per `setTints`, so
   *  a pan or zoom only re-projects coordinates. */
  private _byBar = new Map<number, string[]>();
  private _washes: RenderedWash[] = [];

  attached({ chart, requestUpdate }: SeriesAttachedParameter<UTCTimestamp>): void {
    this._chart = chart;
    this._requestUpdate = requestUpdate;
  }

  detached(): void {
    this._chart = null;
    this._requestUpdate = null;
  }

  setTints(state: BarTintState): void {
    const byBar = new Map<number, string[]>();
    for (const layer of state.layers) {
      for (const tint of layer.tints) {
        const hits = byBar.get(tint.barTime);
        if (hits) hits.push(layer.color);
        else byBar.set(tint.barTime, [layer.color]);
      }
    }
    this._byBar = byBar;
    this._laneRows = state.laneRows;
    this._requestUpdate?.();
  }

  updateAllViews(): void {
    const chart = this._chart;
    if (!chart || this._byBar.size === 0) {
      this._washes = [];
      return;
    }

    const ts = chart.timeScale();
    const slot = ts.options().barSpacing ?? DEFAULT_BAR_SPACING;
    const full = Math.max(MIN_WIDTH, slot * FULL_WIDTH_RATIO);

    const washes: RenderedWash[] = [];
    for (const [barTime, hits] of this._byBar) {
      const x = ts.timeToCoordinate(barTime as UTCTimestamp);
      if (x == null) continue;
      const width = Math.max(MIN_WIDTH, full / hits.length);
      const left = x - (width * hits.length) / 2;
      hits.forEach((color, i) => {
        washes.push({
          x: left + i * width,
          width,
          alpha: WASH_ALPHA,
          color,
        });
      });
    }
    this._washes = washes;
  }

  paneViews(): readonly IPrimitivePaneView[] {
    return [new BarTintPaneView(this._washes, this._laneRows)];
  }
}

// Mirrors `walletMarkersPlugin`'s cast: UTCTimestamp is a subtype of Time, so the
// hop through unknown is what attachPrimitive/detachPrimitive expect.
export function asBarTintPrimitive(p: BarTintPlugin): ISeriesPrimitive {
  return p as unknown as ISeriesPrimitive;
}
