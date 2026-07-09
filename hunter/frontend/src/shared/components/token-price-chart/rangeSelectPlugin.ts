import type { CanvasRenderingTarget2D } from 'fancy-canvas';
import type {
  ISeriesPrimitive,
  ISeriesPrimitiveBase,
  IPrimitivePaneView,
  IPrimitivePaneRenderer,
  SeriesAttachedParameter,
  SeriesType,
  UTCTimestamp,
  IChartApiBase,
  PrimitivePaneViewZOrder,
  Logical,
} from 'lightweight-charts';
import { CHART_COLORS } from './constants';
import { measureLabelWidth } from './labelMetrics';

/**
 * A user-drawn time range to highlight as a full-height band.
 *
 * Mirrors {@link ./chainHighlightPlugin} but is driven by mouse-drag rather
 * than backend data: `dashed` renders the live preview while dragging, and
 * `label` (when set) draws a hoverable chip the host can hit-test.
 */
export interface RangeBandDef {
  /** Chart time of the band's first bar (left edge). */
  loTime: UTCTimestamp;
  /** Chart time of the band's last bar (right edge). */
  hiTime: UTCTimestamp;
  /** Chip text; omit/null while dragging to draw the band without a chip. */
  label?: string | null;
  /** Draft (in-progress drag): dashed borders, no chip. */
  dashed?: boolean;
}

/** Label chip rectangle in media (CSS) pixels — used for drawing and hit-testing. */
interface LabelRect {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** Band geometry in media (CSS) pixels, recomputed each frame from the time scale. */
interface RenderedBand {
  left: number;
  right: number;
  dashed: boolean;
  label: string | null;
  /** Chip rect (CSS px) so the host can hit-test the label; null when no label. */
  labelRect: LabelRect | null;
}

const LABEL_PAD_X = 6; // CSS px horizontal padding inside the label chip
const LABEL_TOP = 6; // CSS px from the top of the pane to the chip
const LABEL_HEIGHT = 16; // CSS px chip height
const LABEL_FONT = 'bold 10px sans-serif'; // chip text font (CSS px)
const MIN_HALF = 3; // CSS px minimum half-bar so narrow bands stay visible

function roundRectPath(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  r: number,
): void {
  const radius = Math.min(r, w / 2, h / 2);
  ctx.beginPath();
  ctx.moveTo(x + radius, y);
  ctx.arcTo(x + w, y, x + w, y + h, radius);
  ctx.arcTo(x + w, y + h, x, y + h, radius);
  ctx.arcTo(x, y + h, x, y, radius);
  ctx.arcTo(x, y, x + w, y, radius);
  ctx.closePath();
}

/** Translucent band fill + boundary lines — drawn behind the price series. */
class RangeBandRenderer implements IPrimitivePaneRenderer {
  constructor(private readonly _band: RenderedBand | null) {}

  draw(target: CanvasRenderingTarget2D): void {
    const band = this._band;
    if (!band) return;
    target.useBitmapCoordinateSpace(({ context: ctx, bitmapSize, horizontalPixelRatio: hr, verticalPixelRatio: vr }) => {
      const left = band.left * hr;
      const right = band.right * hr;
      const width = right - left;
      if (width <= 0) return;

      ctx.save();
      ctx.fillStyle = CHART_COLORS.rangeBandFill;
      ctx.fillRect(left, 0, width, bitmapSize.height);

      ctx.strokeStyle = CHART_COLORS.rangeBandBorder;
      ctx.lineWidth = Math.max(1, Math.round(Math.min(hr, vr)));
      if (band.dashed) ctx.setLineDash([4 * vr, 3 * vr]);
      ctx.beginPath();
      ctx.moveTo(left, 0);
      ctx.lineTo(left, bitmapSize.height);
      ctx.moveTo(right, 0);
      ctx.lineTo(right, bitmapSize.height);
      ctx.stroke();
      ctx.restore();
    });
  }
}

/** Label chip naming the range — drawn above the series so it stays readable. */
class RangeLabelRenderer implements IPrimitivePaneRenderer {
  constructor(private readonly _band: RenderedBand | null) {}

  draw(target: CanvasRenderingTarget2D): void {
    const band = this._band;
    if (!band || !band.label || !band.labelRect) return;
    const label = band.label;
    const rect = band.labelRect;
    target.useBitmapCoordinateSpace(({ context: ctx, horizontalPixelRatio: hr, verticalPixelRatio: vr }) => {
      const s = Math.min(hr, vr);
      const chipLeft = rect.left * hr;
      const chipTop = rect.top * vr;
      const chipWidth = rect.width * hr;
      const chipHeight = rect.height * vr;
      const cx = chipLeft + chipWidth / 2;

      ctx.save();
      ctx.font = `bold ${Math.round(10 * s)}px sans-serif`;
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';

      ctx.fillStyle = CHART_COLORS.rangeBandLabelBg;
      roundRectPath(ctx, chipLeft, chipTop, chipWidth, chipHeight, 3 * s);
      ctx.fill();

      ctx.fillStyle = CHART_COLORS.rangeBandLabelText;
      ctx.fillText(label, cx, chipTop + chipHeight / 2);
      ctx.restore();
    });
  }
}

class RangeBandView implements IPrimitivePaneView {
  constructor(private readonly _band: RenderedBand | null) {}
  zOrder(): PrimitivePaneViewZOrder {
    return 'bottom';
  }
  renderer(): IPrimitivePaneRenderer {
    return new RangeBandRenderer(this._band);
  }
}

class RangeLabelView implements IPrimitivePaneView {
  constructor(private readonly _band: RenderedBand | null) {}
  zOrder(): PrimitivePaneViewZOrder {
    return 'top';
  }
  renderer(): IPrimitivePaneRenderer {
    return new RangeLabelRenderer(this._band);
  }
}

export class RangeSelectPlugin
  implements ISeriesPrimitiveBase<SeriesAttachedParameter<UTCTimestamp, SeriesType>>
{
  private _chart: IChartApiBase<UTCTimestamp> | null = null;
  private _requestUpdate: (() => void) | null = null;
  private _def: RangeBandDef | null = null;
  private _band: RenderedBand | null = null;

  attached({ chart, requestUpdate }: SeriesAttachedParameter<UTCTimestamp>): void {
    this._chart = chart;
    this._requestUpdate = requestUpdate;
  }

  detached(): void {
    this._chart = null;
    this._requestUpdate = null;
  }

  setBand(def: RangeBandDef | null): void {
    this._def = def;
    this._requestUpdate?.();
  }

  updateAllViews(): void {
    const chart = this._chart;
    const def = this._def;
    if (!chart || !def) {
      this._band = null;
      return;
    }

    const ts = chart.timeScale();
    const xLo = ts.timeToCoordinate(def.loTime);
    const xHi = ts.timeToCoordinate(def.hiTime);
    if (xLo == null || xHi == null) {
      this._band = null;
      return;
    }

    // Extend each side by half a bar so the band wraps the full first/last bars.
    const c0 = ts.logicalToCoordinate(0 as Logical);
    const c1 = ts.logicalToCoordinate(1 as Logical);
    const half =
      c0 != null && c1 != null ? Math.max(MIN_HALF, Math.abs(c1 - c0) / 2) : MIN_HALF;

    const left = Math.min(xLo, xHi) - half;
    const right = Math.max(xLo, xHi) + half;
    const label = def.label ?? null;
    const labelRect =
      label != null
        ? (() => {
            const chipWidth = measureLabelWidth(label, LABEL_FONT) + LABEL_PAD_X * 2;
            return {
              left: (left + right) / 2 - chipWidth / 2,
              top: LABEL_TOP,
              width: chipWidth,
              height: LABEL_HEIGHT,
            };
          })()
        : null;

    this._band = { left, right, dashed: def.dashed ?? false, label, labelRect };
  }

  /** True when (x, y) in CSS px lands on the label chip — not the band body. */
  containsLabelPoint(x: number, y: number): boolean {
    const rect = this._band?.labelRect;
    if (!rect) return false;
    return (
      x >= rect.left &&
      x <= rect.left + rect.width &&
      y >= rect.top &&
      y <= rect.top + rect.height
    );
  }

  paneViews(): readonly IPrimitivePaneView[] {
    return [new RangeBandView(this._band), new RangeLabelView(this._band)];
  }
}

// UTCTimestamp is a subtype of Time, so the cast through unknown is safe.
export function asRangePrimitive(p: RangeSelectPlugin): ISeriesPrimitive {
  return p as unknown as ISeriesPrimitive;
}
