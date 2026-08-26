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
import type { LensBarTint } from './lensTint';

/**
 * Full-height vertical washes behind the candles — "the thing you picked happened
 * HERE, and this is how much of the candle it was".
 *
 * Two layers, drawn side by side rather than stacked: a candlestick carries exactly
 * one `borderColor`, so two lenses cannot both express themselves through the
 * series. Splitting the column instead keeps an overlap readable — a bar where both
 * lenses hit shows both halves, which is the cell a reader is actually hunting for.
 *
 * Vocabulary-free like the rest of this folder: a layer is a color and a list of
 * (bar, share) pairs. The chart never learns that one of them means "a wallet" and
 * the other "an ix structure".
 */
export interface BarTintLayers {
  /** Drawn on the LEFT half of each bar slot when both layers are non-empty. */
  primary: { color: string; tints: readonly LensBarTint[] } | null;
  /** Drawn on the RIGHT half of each bar slot when both layers are non-empty. */
  secondary: { color: string; tints: readonly LensBarTint[] } | null;
}

export const EMPTY_BAR_TINTS: BarTintLayers = { primary: null, secondary: null };

/** Fraction of a bar slot one wash spans when it has the slot to itself. */
const FULL_WIDTH_RATIO = 0.9;
/** A wash narrower than this reads as a hairline artifact, so it is widened to it. */
const MIN_WIDTH = 1.5;
/** Alpha at share→0. Above zero on purpose: "one dust leg here" is still a HIT, and
 *  a hit that fades to invisible is indistinguishable from no hit at all. */
const MIN_ALPHA = 0.14;
/** Alpha at share→1 — a bar the target owns outright. Capped below opaque so the
 *  candle and its wick stay readable through the wash. */
const MAX_ALPHA = 0.55;

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
  constructor(private readonly _washes: RenderedWash[]) {}

  draw(target: CanvasRenderingTarget2D): void {
    if (this._washes.length === 0) return;
    target.useMediaCoordinateSpace(({ context: ctx, mediaSize }) => {
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
        ctx.fillRect(w.x, 0, w.width, mediaSize.height);
      }
      ctx.restore();
    });
  }
}

class BarTintPaneView implements IPrimitivePaneView {
  constructor(private readonly _washes: RenderedWash[]) {}

  /** Behind the candles — this is a background wash, not an overlay. */
  zOrder(): PrimitivePaneViewZOrder {
    return 'bottom';
  }

  renderer(): IPrimitivePaneRenderer {
    return new BarTintRenderer(this._washes);
  }
}

export class BarTintPlugin
  implements ISeriesPrimitiveBase<SeriesAttachedParameter<UTCTimestamp>>
{
  private _chart: IChartApiBase<UTCTimestamp> | null = null;
  private _requestUpdate: (() => void) | null = null;
  private _layers: BarTintLayers = EMPTY_BAR_TINTS;
  private _washes: RenderedWash[] = [];

  attached({ chart, requestUpdate }: SeriesAttachedParameter<UTCTimestamp>): void {
    this._chart = chart;
    this._requestUpdate = requestUpdate;
  }

  detached(): void {
    this._chart = null;
    this._requestUpdate = null;
  }

  setTints(layers: BarTintLayers): void {
    this._layers = layers;
    this._requestUpdate?.();
  }

  updateAllViews(): void {
    const chart = this._chart;
    const { primary, secondary } = this._layers;
    if (!chart || (!primary?.tints.length && !secondary?.tints.length)) {
      this._washes = [];
      return;
    }

    const ts = chart.timeScale();
    const slot = ts.options().barSpacing ?? DEFAULT_BAR_SPACING;
    // Both layers live ⇒ split the slot so neither can hide the other.
    const split = !!primary?.tints.length && !!secondary?.tints.length;
    const full = Math.max(MIN_WIDTH, slot * FULL_WIDTH_RATIO);
    const width = split ? Math.max(MIN_WIDTH, full / 2) : full;

    const washes: RenderedWash[] = [];
    const push = (
      layer: { color: string; tints: readonly LensBarTint[] } | null,
      side: -1 | 0 | 1,
    ) => {
      if (!layer) return;
      for (const t of layer.tints) {
        const x = ts.timeToCoordinate(t.barTime as UTCTimestamp);
        if (x == null) continue;
        const left = side === 0 ? x - width / 2 : side < 0 ? x - width : x;
        washes.push({
          x: left,
          width,
          alpha: MIN_ALPHA + (MAX_ALPHA - MIN_ALPHA) * Math.min(1, Math.max(0, t.share)),
          color: layer.color,
        });
      }
    };
    push(primary, split ? -1 : 0);
    push(secondary, split ? 1 : 0);
    this._washes = washes;
  }

  paneViews(): readonly IPrimitivePaneView[] {
    return [new BarTintPaneView(this._washes)];
  }
}

// Mirrors `walletMarkersPlugin`'s cast: UTCTimestamp is a subtype of Time, so the
// hop through unknown is what attachPrimitive/detachPrimitive expect.
export function asBarTintPrimitive(p: BarTintPlugin): ISeriesPrimitive {
  return p as unknown as ISeriesPrimitive;
}
