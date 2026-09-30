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

import { CHART_COLORS, DEFAULT_BAR_SPACING } from './constants';
import { formatLensSol, type LensBarTint } from './lensTint';
import type { ChartLensSizeLabels } from './types';

/**
 * Full-height vertical washes behind the candles — "the thing you picked happened
 * HERE, and this is how much of the candle it was" — plus the SOL size of each
 * matched trade printed under its wash.
 *
 * Layers are drawn side by side rather than stacked: a candlestick carries exactly
 * one `borderColor`, so two layers cannot both express themselves through the
 * series. A bar's slot is split among the layers that hit THAT bar only, so a bar
 * one layer owns keeps the full width however many layers are armed, and a bar
 * where several hit shows each — the cell a reader is actually hunting for.
 *
 * Vocabulary-free like the rest of this folder: a layer is a color and a list of
 * (bar, share, trades). The chart never learns that one of them means "a wallet"
 * and another "an ix structure".
 */
export interface BarTintLayer {
  color: string;
  tints: readonly LensBarTint[];
}

export interface BarTintState {
  layers: readonly BarTintLayer[];
  /** Which matched trades print their size under the wash. */
  sizeLabels: ChartLensSizeLabels;
}

export const EMPTY_BAR_TINTS: BarTintState = { layers: [], sizeLabels: 'off' };

/** Fraction of a bar slot the washes of one bar span together. */
const FULL_WIDTH_RATIO = 0.9;
/** A wash narrower than this reads as a hairline artifact, so it is widened to it. */
const MIN_WIDTH = 1.5;
/** Alpha at share→0. Above zero on purpose: "one dust leg here" is still a HIT, and
 *  a hit that fades to invisible is indistinguishable from no hit at all. */
const MIN_ALPHA = 0.14;
/** Alpha at share→1 — a bar the target owns outright. Capped below opaque so the
 *  candle and its wick stay readable through the wash. */
const MAX_ALPHA = 0.55;

/** Size labels: rotated text, so a column is one line tall however long the number. */
const LABEL_FONT = '10px ui-monospace, SFMono-Regular, Menlo, monospace';
/** Column thickness of a rotated label (its line height). */
const LABEL_LINE = 12;
/** Sizes printed per (bar, layer) before the rest collapse into `+k`. */
const LABEL_MAX_PER_COLUMN = 3;
const LABEL_PAD = 2;
/** Gap between stacked labels, and between the stack and the pane bottom. */
const LABEL_GAP = 3;
const LABEL_BG = 'rgba(13, 13, 13, 0.82)';

interface RenderedWash {
  /** Media-space left edge. */
  x: number;
  width: number;
  alpha: number;
  color: string;
}

/** One stack of size labels under one layer's share of one bar. */
interface LabelColumn {
  /** Media-space center of the column. */
  cx: number;
  color: string;
  /** Printed bottom-up: the largest trades first, then `+k` for the rest. */
  texts: string[];
  /** Placement priority when columns collide: the column moving the most SOL wins. */
  sol: number;
}

/** `#rrggbb` + alpha → `rgba(...)`. Any other notation is passed through with the
 *  alpha applied via globalAlpha instead, so a themed color can't break the wash. */
function washFill(color: string, alpha: number): string | null {
  const hex = /^#([0-9a-f]{6})$/i.exec(color);
  if (!hex) return null;
  const n = parseInt(hex[1], 16);
  return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${alpha})`;
}

/** The label column for one layer's hit on one bar, or null when `mode` prints
 *  nothing for its trades. Buys-only prints bare sizes; `all` signs each one, so a
 *  sell can never be read as a buy. */
function labelColumn(
  tint: LensBarTint,
  color: string,
  cx: number,
  mode: ChartLensSizeLabels,
): LabelColumn | null {
  const sized: { sol: number; text: string }[] = [];
  for (const t of tint.trades) {
    if (t.amount_sol == null) continue;
    if (t.trade_type === 'buy') {
      sized.push({ sol: t.amount_sol, text: (mode === 'all' ? '+' : '') + formatLensSol(t.amount_sol) });
    } else if (mode === 'all') {
      sized.push({ sol: t.amount_sol, text: `−${formatLensSol(t.amount_sol)}` });
    }
  }
  if (sized.length === 0) return null;
  sized.sort((a, b) => b.sol - a.sol);
  const texts = sized.slice(0, LABEL_MAX_PER_COLUMN).map((s) => s.text);
  if (sized.length > LABEL_MAX_PER_COLUMN) texts.push(`+${sized.length - LABEL_MAX_PER_COLUMN}`);
  return { cx, color, texts, sol: sized.reduce((n, s) => n + s.sol, 0) };
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

/**
 * Size labels over the candles. Columns that would overlap are resolved by SOL —
 * the biggest column keeps its place — and the ones that lose are counted into a
 * "zoom in" hint, so a hidden size is never mistaken for a bar with no match.
 */
class SizeLabelRenderer implements IPrimitivePaneRenderer {
  constructor(private readonly _columns: LabelColumn[]) {}

  draw(target: CanvasRenderingTarget2D): void {
    if (this._columns.length === 0) return;
    target.useMediaCoordinateSpace(({ context: ctx, mediaSize }) => {
      const width = mediaSize.width;
      const inView = this._columns.filter((c) => c.cx >= 0 && c.cx <= width);
      if (inView.length === 0) return;
      inView.sort((a, b) => b.sol - a.sol);

      const taken = new Uint8Array(Math.ceil(width) + 1);
      const half = LABEL_LINE / 2;
      let hidden = 0;
      ctx.save();
      ctx.font = LABEL_FONT;
      ctx.textBaseline = 'middle';
      for (const col of inView) {
        const lo = Math.max(0, Math.floor(col.cx - half));
        const hi = Math.min(taken.length - 1, Math.ceil(col.cx + half));
        let free = true;
        for (let x = lo; x <= hi && free; x++) free = taken[x] === 0;
        if (!free) {
          hidden += 1;
          continue;
        }
        taken.fill(1, lo, hi + 1);

        // Bottom-up, rotated a quarter turn so the text reads upward.
        let y = mediaSize.height - LABEL_GAP;
        for (const text of col.texts) {
          const len = ctx.measureText(text).width + LABEL_PAD * 2;
          if (y - len < 0) break;
          ctx.save();
          ctx.translate(col.cx, y);
          ctx.rotate(-Math.PI / 2);
          ctx.fillStyle = LABEL_BG;
          ctx.fillRect(0, -half, len, LABEL_LINE);
          ctx.fillStyle = col.color;
          ctx.fillText(text, LABEL_PAD, 0);
          ctx.restore();
          y -= len + LABEL_GAP;
        }
      }

      if (hidden > 0) {
        const hint = `sizes hidden on ${hidden} bar${hidden === 1 ? '' : 's'} · zoom in`;
        const len = ctx.measureText(hint).width + LABEL_PAD * 2;
        ctx.fillStyle = LABEL_BG;
        ctx.fillRect(6, 6, len, LABEL_LINE);
        ctx.fillStyle = CHART_COLORS.panelTextDim;
        ctx.fillText(hint, 6 + LABEL_PAD, 6 + half);
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

class SizeLabelPaneView implements IPrimitivePaneView {
  constructor(private readonly _columns: LabelColumn[]) {}

  /** Over the candles — a number a candle body hides is no number at all. */
  zOrder(): PrimitivePaneViewZOrder {
    return 'top';
  }

  renderer(): IPrimitivePaneRenderer {
    return new SizeLabelRenderer(this._columns);
  }
}

export class BarTintPlugin
  implements ISeriesPrimitiveBase<SeriesAttachedParameter<UTCTimestamp>>
{
  private _chart: IChartApiBase<UTCTimestamp> | null = null;
  private _requestUpdate: (() => void) | null = null;
  private _sizeLabels: ChartLensSizeLabels = 'off';
  /** Bar key → the layers hitting it, in layer order. Built once per `setTints`, so
   *  a pan or zoom only re-projects coordinates. */
  private _byBar = new Map<number, { color: string; tint: LensBarTint }[]>();
  private _washes: RenderedWash[] = [];
  private _columns: LabelColumn[] = [];

  attached({ chart, requestUpdate }: SeriesAttachedParameter<UTCTimestamp>): void {
    this._chart = chart;
    this._requestUpdate = requestUpdate;
  }

  detached(): void {
    this._chart = null;
    this._requestUpdate = null;
  }

  setTints(state: BarTintState): void {
    const byBar = new Map<number, { color: string; tint: LensBarTint }[]>();
    for (const layer of state.layers) {
      for (const tint of layer.tints) {
        const hits = byBar.get(tint.barTime);
        if (hits) hits.push({ color: layer.color, tint });
        else byBar.set(tint.barTime, [{ color: layer.color, tint }]);
      }
    }
    this._byBar = byBar;
    this._sizeLabels = state.sizeLabels;
    this._requestUpdate?.();
  }

  updateAllViews(): void {
    const chart = this._chart;
    if (!chart || this._byBar.size === 0) {
      this._washes = [];
      this._columns = [];
      return;
    }

    const ts = chart.timeScale();
    const slot = ts.options().barSpacing ?? DEFAULT_BAR_SPACING;
    const full = Math.max(MIN_WIDTH, slot * FULL_WIDTH_RATIO);
    const mode = this._sizeLabels;

    const washes: RenderedWash[] = [];
    const columns: LabelColumn[] = [];
    for (const [barTime, hits] of this._byBar) {
      const x = ts.timeToCoordinate(barTime as UTCTimestamp);
      if (x == null) continue;
      const width = Math.max(MIN_WIDTH, full / hits.length);
      const left = x - (width * hits.length) / 2;
      hits.forEach(({ color, tint }, i) => {
        washes.push({
          x: left + i * width,
          width,
          alpha: MIN_ALPHA + (MAX_ALPHA - MIN_ALPHA) * Math.min(1, Math.max(0, tint.share)),
          color,
        });
        if (mode === 'off') return;
        const col = labelColumn(tint, color, left + (i + 0.5) * width, mode);
        if (col) columns.push(col);
      });
    }
    this._washes = washes;
    this._columns = columns;
  }

  paneViews(): readonly IPrimitivePaneView[] {
    return [new BarTintPaneView(this._washes), new SizeLabelPaneView(this._columns)];
  }
}

// Mirrors `walletMarkersPlugin`'s cast: UTCTimestamp is a subtype of Time, so the
// hop through unknown is what attachPrimitive/detachPrimitive expect.
export function asBarTintPrimitive(p: BarTintPlugin): ISeriesPrimitive {
  return p as unknown as ISeriesPrimitive;
}
