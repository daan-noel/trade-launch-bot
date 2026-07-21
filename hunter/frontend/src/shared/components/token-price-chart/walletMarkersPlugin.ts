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

/** Silhouette encodes wallet CLASS (orthogonal to color=identity, border=direction):
 *  diamond = the user's own (`mine`) wallet, triangle = the token's dev/creator
 *  wallet, hexagon = the focused/input wallet, circle = every other tracked wallet. */
export type MarkerShape = 'circle' | 'diamond' | 'triangle' | 'hexagon';

export interface WalletMarkerDef {
  barTime: UTCTimestamp;
  /** bar.low for buy (drawn below the bar), bar.high for sell (drawn above the bar). */
  barEdgePrice: number;
  letter: string;
  color: string;
  borderColor: string;
  type: 'buy' | 'sell';
  /** vertical stack index for multiple wallets on same bar+type */
  stackIndex: number;
  /** Wallet-class silhouette (see {@link MarkerShape}). */
  shape: MarkerShape;
  /** Position-lifecycle boundary: `first_buy` = wallet's entry, `sell_all` = full
   *  exit. Drawn heavier (larger + a direction-colored double ring) so entries and
   *  full dumps stand out from quiet mid-position adds/trims. */
  role?: 'first_buy' | 'sell_all';
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
  shape: MarkerShape;
  role?: 'first_buy' | 'sell_all';
  highlighted?: boolean;
  ringColor?: string;
}

// Marker radius tracks candle width so markers shrink/grow as you scroll-zoom.
// Base radius ≈ half a candle (diameter ≈ one candle), clamped so it never gets
// illegibly small or oversized; role/focus tiers are multiples of that base.
const CANDLE_RADIUS_RATIO = 0.5; // base radius as a fraction of barSpacing (px/candle)
const MIN_RADIUS = 2.5;  // CSS px — floor when zoomed out
const MAX_RADIUS = 7;    // CSS px — cap when zoomed in (regular tier)
const LIFECYCLE_MULT = 1.25; // first_buy / sell_all, relative to base
const HIGHLIGHT_MULT = 1.5;  // focused wallet, relative to base
const DEFAULT_BAR_SPACING = 6; // lightweight-charts default; fallback if unavailable
const GLYPH_MIN_RADIUS = 3.5; // below this the disc is too small for a legible letter
const GAP = 5;      // CSS px gap between bar edge and nearest marker center
const SPACING = 2;  // CSS px between stacked markers
/** Diamond/hexagon need a slightly larger bound than a circle to fit the glyph. */
const POLY_SCALE = 1.15;

/** Trace a marker silhouette centered at (cx, cy). Radius is in device px. */
function traceShape(
  ctx: CanvasRenderingContext2D,
  cx: number,
  cy: number,
  r: number,
  shape: MarkerShape,
): void {
  ctx.beginPath();
  if (shape === 'circle') {
    ctx.arc(cx, cy, r, 0, Math.PI * 2);
    return;
  }
  const rr = r * POLY_SCALE;
  if (shape === 'diamond') {
    ctx.moveTo(cx, cy - rr);
    ctx.lineTo(cx + rr, cy);
    ctx.lineTo(cx, cy + rr);
    ctx.lineTo(cx - rr, cy);
    ctx.closePath();
    return;
  }
  if (shape === 'triangle') {
    // Upward equilateral triangle, centered on (cx, cy) — the dev/creator wallet.
    ctx.moveTo(cx, cy - rr);
    ctx.lineTo(cx + rr * 0.866, cy + rr * 0.5);
    ctx.lineTo(cx - rr * 0.866, cy + rr * 0.5);
    ctx.closePath();
    return;
  }
  // Flat-top hexagon (points left/right, flat top & bottom) — widest across the glyph.
  for (let i = 0; i < 6; i++) {
    const a = (Math.PI / 3) * i;
    const px = cx + rr * Math.cos(a);
    const py = cy + rr * Math.sin(a);
    if (i === 0) ctx.moveTo(px, py);
    else ctx.lineTo(px, py);
  }
  ctx.closePath();
}

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
        // Outer extent of the silhouette; concentric rings clear this so a
        // diamond/hexagon's corners never poke through them.
        const ringBase = p.shape === 'circle' ? r : r * POLY_SCALE;

        // Filled silhouette — glow only for the focused wallet, isolated in its own
        // save/restore so the shadow never bleeds onto neighbouring markers.
        ctx.save();
        if (p.highlighted) {
          ctx.shadowColor = p.ringColor ?? p.color;
          ctx.shadowBlur = 9 * s;
        }
        traceShape(ctx, cx, cy, r, p.shape);
        ctx.fillStyle = p.color;
        ctx.fill();
        ctx.restore();

        // Buy/sell border stays (identity + direction), regardless of highlight.
        ctx.lineWidth = 2 * s;
        ctx.strokeStyle = p.borderColor;
        traceShape(ctx, cx, cy, r, p.shape);
        ctx.stroke();

        // Concentric rings stack outward: the direction-colored lifecycle ring
        // (green double-ring = entry, red = full exit) first, then the gold focus
        // ring on the very outside so focus always wins the edge.
        let ringR = ringBase;
        if (p.role) {
          ringR += 3 * s;
          ctx.lineWidth = 2 * s;
          ctx.strokeStyle = p.borderColor;
          ctx.beginPath();
          ctx.arc(cx, cy, ringR, 0, Math.PI * 2);
          ctx.stroke();
        }
        if (p.highlighted) {
          ringR += 3 * s;
          ctx.lineWidth = 2 * s;
          ctx.strokeStyle = p.ringColor ?? '#fff';
          ctx.beginPath();
          ctx.arc(cx, cy, ringR, 0, Math.PI * 2);
          ctx.stroke();
        }

        // Glyph scales with the disc; skip it once the disc is too small to read.
        if (p.radius >= GLYPH_MIN_RADIUS) {
          ctx.font = `bold ${Math.round(p.radius * 1.15 * s)}px sans-serif`;
          ctx.fillStyle = '#fff';
          ctx.fillText(p.letter, cx, cy);
        }
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

    // Candle-proportional base radius — recomputed here so it tracks the current
    // zoom (updateAllViews re-runs on every pan/zoom). barSpacing = px per candle.
    const barSpacing = chart.timeScale().options().barSpacing ?? DEFAULT_BAR_SPACING;
    const unit = Math.max(
      MIN_RADIUS,
      Math.min(MAX_RADIUS, barSpacing * CANDLE_RADIUS_RATIO),
    );

    const pts: RenderedPoint[] = [];
    for (const d of this._defs) {
      const x = chart.timeScale().timeToCoordinate(d.barTime);
      const baseY = series.priceToCoordinate(d.barEdgePrice);
      if (x == null || baseY == null) continue;

      const radius = d.highlighted
        ? unit * HIGHLIGHT_MULT
        : d.role
          ? unit * LIFECYCLE_MULT
          : unit;
      const dir = d.type === 'sell' ? -1 : 1;
      // Stack offset uses the shared base radius (regular-tier diameter) so rows
      // stay evenly spaced regardless of any one marker's tier.
      const y =
        baseY +
        dir * (GAP + radius + d.stackIndex * (unit * 2 + SPACING));

      pts.push({
        x,
        y,
        radius,
        letter: d.letter,
        color: d.color,
        borderColor: d.borderColor,
        shape: d.shape,
        role: d.role,
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
