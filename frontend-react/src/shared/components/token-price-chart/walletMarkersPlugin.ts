import type { CanvasRenderingTarget2D } from 'fancy-canvas';
import type {
  ISeriesPrimitive,
  ISeriesPrimitiveBase,
  IPrimitivePaneView,
  IPrimitivePaneRenderer,
  SeriesAttachedParameter,
  ISeriesApi,
  SeriesType,
  UTCTimestamp,
  IChartApiBase,
  PrimitivePaneViewZOrder,
} from 'lightweight-charts';

export interface WalletMarkerDef {
  barTime: UTCTimestamp;
  /** bar.low for buy (belowBar), bar.high for sell (aboveBar) */
  barEdgePrice: number;
  letter: string;
  color: string;
  borderColor: string;
  type: 'buy' | 'sell';
  /** vertical stack index for multiple wallets on same bar+type */
  stackIndex: number;
}

interface RenderedPoint {
  x: number;
  y: number;
  letter: string;
  color: string;
  borderColor: string;
}

const RADIUS = 7;   // CSS px
const GAP = 5;      // CSS px gap between bar edge and nearest marker center
const SPACING = 2;  // CSS px between stacked markers

class WalletMarkersRenderer implements IPrimitivePaneRenderer {
  constructor(private readonly _pts: RenderedPoint[]) {}

  draw(target: CanvasRenderingTarget2D): void {
    target.useBitmapCoordinateSpace(({ context: ctx, horizontalPixelRatio, verticalPixelRatio }) => {
      const s = Math.min(horizontalPixelRatio, verticalPixelRatio);
      const r = RADIUS * s;

      ctx.save();
      ctx.font = `bold ${Math.round(8 * s)}px sans-serif`;
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';

      for (const p of this._pts) {
        const cx = p.x * horizontalPixelRatio;
        const cy = p.y * verticalPixelRatio;

        ctx.beginPath();
        ctx.arc(cx, cy, r, 0, Math.PI * 2);
        ctx.fillStyle = p.color;
        ctx.fill();
        ctx.lineWidth = 2 * s;
        ctx.strokeStyle = p.borderColor;
        ctx.stroke();

        ctx.fillStyle = '#fff';
        ctx.fillText(p.letter, cx, cy);
      }

      ctx.restore();
    });
  }
}

class WalletMarkersPaneView implements IPrimitivePaneView {
  constructor(private readonly _pts: RenderedPoint[]) {}

  zOrder(): PrimitivePaneViewZOrder {
    return 'top';
  }

  renderer(): IPrimitivePaneRenderer {
    return new WalletMarkersRenderer(this._pts);
  }
}

export class WalletMarkersPlugin
  implements ISeriesPrimitiveBase<SeriesAttachedParameter<UTCTimestamp, SeriesType>>
{
  private _chart: IChartApiBase<UTCTimestamp> | null = null;
  private _series: ISeriesApi<SeriesType, UTCTimestamp> | null = null;
  private _requestUpdate: (() => void) | null = null;
  private _defs: WalletMarkerDef[] = [];
  private _pts: RenderedPoint[] = [];

  attached({ chart, series, requestUpdate }: SeriesAttachedParameter<UTCTimestamp>): void {
    this._chart = chart;
    this._series = series;
    this._requestUpdate = requestUpdate;
  }

  detached(): void {
    this._chart = null;
    this._series = null;
    this._requestUpdate = null;
  }

  setMarkers(defs: WalletMarkerDef[]): void {
    this._defs = defs;
    this._requestUpdate?.();
  }

  updateAllViews(): void {
    const chart = this._chart;
    const series = this._series;
    if (!chart || !series) {
      this._pts = [];
      return;
    }

    const pts: RenderedPoint[] = [];
    for (const d of this._defs) {
      const x = chart.timeScale().timeToCoordinate(d.barTime);
      const baseY = series.priceToCoordinate(d.barEdgePrice);
      if (x == null || baseY == null) continue;

      const dir = d.type === 'sell' ? -1 : 1;
      const y =
        baseY +
        dir * (GAP + RADIUS + d.stackIndex * (RADIUS * 2 + SPACING));

      pts.push({ x, y, letter: d.letter, color: d.color, borderColor: d.borderColor });
    }
    this._pts = pts;
  }

  containsPoint(x: number, y: number): boolean {
    for (const p of this._pts) {
      const dx = p.x - x;
      const dy = p.y - y;
      if (dx * dx + dy * dy <= RADIUS * RADIUS) return true;
    }
    return false;
  }

  paneViews(): readonly IPrimitivePaneView[] {
    return [new WalletMarkersPaneView(this._pts)];
  }
}

// ISeriesPrimitive<Time> is what attachPrimitive/detachPrimitive expect.
// UTCTimestamp is a subtype of Time, so the cast through unknown is safe.
export function asSeriesPrimitive(p: WalletMarkersPlugin): ISeriesPrimitive {
  return p as unknown as ISeriesPrimitive;
}
