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

/** Longest swing chain to highlight as a full-height band. */
export interface ChainHighlightDef {
  /** Resolved chart time of the chain's first bar (left edge). */
  loTime: UTCTimestamp;
  /** Resolved chart time of the chain's last bar (right edge). */
  hiTime: UTCTimestamp;
  /** Pairs linked in the chain — shown in the label chip. */
  pairCount: number;
}

/** Band geometry in media (CSS) pixels, recomputed each frame from the time scale. */
interface RenderedBand {
  left: number;
  right: number;
  label: string;
}

const LABEL_PAD_X = 6; // CSS px horizontal padding inside the label chip
const LABEL_TOP = 6; // CSS px from the top of the pane to the chip
const LABEL_HEIGHT = 16; // CSS px chip height
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
class ChainBandRenderer implements IPrimitivePaneRenderer {
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
      ctx.fillStyle = CHART_COLORS.chainBandFill;
      ctx.fillRect(left, 0, width, bitmapSize.height);

      ctx.strokeStyle = CHART_COLORS.chainBandBorder;
      ctx.lineWidth = Math.max(1, Math.round(Math.min(hr, vr)));
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

/** Label chip naming the chain — drawn above the series so it stays readable. */
class ChainLabelRenderer implements IPrimitivePaneRenderer {
  constructor(private readonly _band: RenderedBand | null) {}

  draw(target: CanvasRenderingTarget2D): void {
    const band = this._band;
    if (!band) return;
    target.useBitmapCoordinateSpace(({ context: ctx, horizontalPixelRatio: hr, verticalPixelRatio: vr }) => {
      const s = Math.min(hr, vr);
      const cx = ((band.left + band.right) / 2) * hr;
      const top = LABEL_TOP * vr;

      ctx.save();
      ctx.font = `bold ${Math.round(10 * s)}px sans-serif`;
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';

      const textWidth = ctx.measureText(band.label).width;
      const chipWidth = textWidth + LABEL_PAD_X * 2 * s;
      const chipHeight = LABEL_HEIGHT * s;
      const chipLeft = cx - chipWidth / 2;

      ctx.fillStyle = CHART_COLORS.chainBandLabelBg;
      roundRectPath(ctx, chipLeft, top, chipWidth, chipHeight, 3 * s);
      ctx.fill();

      ctx.fillStyle = CHART_COLORS.chainBandLabelText;
      ctx.fillText(band.label, cx, top + chipHeight / 2);
      ctx.restore();
    });
  }
}

class ChainBandView implements IPrimitivePaneView {
  constructor(private readonly _band: RenderedBand | null) {}
  zOrder(): PrimitivePaneViewZOrder {
    return 'bottom';
  }
  renderer(): IPrimitivePaneRenderer {
    return new ChainBandRenderer(this._band);
  }
}

class ChainLabelView implements IPrimitivePaneView {
  constructor(private readonly _band: RenderedBand | null) {}
  zOrder(): PrimitivePaneViewZOrder {
    return 'top';
  }
  renderer(): IPrimitivePaneRenderer {
    return new ChainLabelRenderer(this._band);
  }
}

export class ChainHighlightPlugin
  implements ISeriesPrimitiveBase<SeriesAttachedParameter<UTCTimestamp, SeriesType>>
{
  private _chart: IChartApiBase<UTCTimestamp> | null = null;
  private _requestUpdate: (() => void) | null = null;
  private _def: ChainHighlightDef | null = null;
  private _band: RenderedBand | null = null;

  attached({ chart, requestUpdate }: SeriesAttachedParameter<UTCTimestamp>): void {
    this._chart = chart;
    this._requestUpdate = requestUpdate;
  }

  detached(): void {
    this._chart = null;
    this._requestUpdate = null;
  }

  setHighlight(def: ChainHighlightDef | null): void {
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

    this._band = {
      left: Math.min(xLo, xHi) - half,
      right: Math.max(xLo, xHi) + half,
      label: `Longest chain · ${def.pairCount} pairs`,
    };
  }

  paneViews(): readonly IPrimitivePaneView[] {
    return [new ChainBandView(this._band), new ChainLabelView(this._band)];
  }
}

// UTCTimestamp is a subtype of Time, so the cast through unknown is safe.
export function asChainPrimitive(p: ChainHighlightPlugin): ISeriesPrimitive {
  return p as unknown as ISeriesPrimitive;
}
