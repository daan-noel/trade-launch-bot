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
  /** Focused wallet (Trader Analysis input) — draws larger with a glow + ring. */
  highlighted?: boolean;
  /** Glow/outer-ring color for a highlighted marker. */
  ringColor?: string;
}

interface RenderedPoint {
  x: number;
  y: number;
  radius: number;
  letter: string;
  color: string;
  borderColor: string;
  highlighted?: boolean;
  ringColor?: string;
}

const RADIUS = 7;          // CSS px
const HIGHLIGHT_RADIUS = 11; // CSS px — focused wallet marker
const GAP = 5;      // CSS px gap between bar edge and nearest marker center
const SPACING = 2;  // CSS px between stacked markers

class WalletMarkersRenderer implements IPrimitivePaneRenderer {
  constructor(private readonly _pts: RenderedPoint[]) {}

  draw(target: CanvasRenderingTarget2D): void {
    target.useBitmapCoordinateSpace(({ context: ctx, horizontalPixelRatio, verticalPixelRatio }) => {
      const s = Math.min(horizontalPixelRatio, verticalPixelRatio);

      ctx.save();
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';

      for (const p of this._pts) {
        const cx = p.x * horizontalPixelRatio;
        const cy = p.y * verticalPixelRatio;
        const r = p.radius * s;

        // Filled disc — glow only for the focused wallet, isolated in its own
        // save/restore so the shadow never bleeds onto neighbouring markers.
        ctx.save();
        if (p.highlighted) {
          ctx.shadowColor = p.ringColor ?? p.color;
          ctx.shadowBlur = 9 * s;
        }
        ctx.beginPath();
        ctx.arc(cx, cy, r, 0, Math.PI * 2);
        ctx.fillStyle = p.color;
        ctx.fill();
        ctx.restore();

        // Buy/sell border stays (identity + direction), regardless of highlight.
        ctx.lineWidth = 2 * s;
        ctx.strokeStyle = p.borderColor;
        ctx.beginPath();
        ctx.arc(cx, cy, r, 0, Math.PI * 2);
        ctx.stroke();

        // Highlighted wallet gets an extra gold outer ring so it reads as "the
        // one you're focused on" without losing its buy/sell border.
        if (p.highlighted) {
          ctx.lineWidth = 2 * s;
          ctx.strokeStyle = p.ringColor ?? '#fff';
          ctx.beginPath();
          ctx.arc(cx, cy, r + 3 * s, 0, Math.PI * 2);
          ctx.stroke();
        }

        ctx.font = `bold ${Math.round((p.highlighted ? 10 : 8) * s)}px sans-serif`;
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

      const radius = d.highlighted ? HIGHLIGHT_RADIUS : RADIUS;
      const dir = d.type === 'sell' ? -1 : 1;
      // Stack offset uses each marker's own radius so a larger highlighted disc
      // pushes its neighbours clear instead of overlapping them.
      const y =
        baseY +
        dir * (GAP + radius + d.stackIndex * (RADIUS * 2 + SPACING));

      pts.push({
        x,
        y,
        radius,
        letter: d.letter,
        color: d.color,
        borderColor: d.borderColor,
        highlighted: d.highlighted,
        ringColor: d.ringColor,
      });
    }
    this._pts = pts;
  }

  containsPoint(x: number, y: number): boolean {
    for (const p of this._pts) {
      const dx = p.x - x;
      const dy = p.y - y;
      if (dx * dx + dy * dy <= p.radius * p.radius) return true;
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
