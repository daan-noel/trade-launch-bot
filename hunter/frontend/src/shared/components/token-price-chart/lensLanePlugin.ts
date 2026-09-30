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
import type { ChartLensSizeLabels, ChartTrade } from './types';

/**
 * The highlight lane: one row per armed lens item along the bottom of the price
 * pane, a solid mark on every bar the item appears in — "where in this token's
 * whole history did it show up", readable at a glance without reading candles.
 *
 * A mark's height grows with the SOL it moved in that bar, so the big hits stand
 * out; with sizes on, the number sits beside the mark, horizontal. Row names are
 * drawn in the price-axis gutter, so the plot keeps every pixel for marks.
 *
 * The host reserves the lane's height under the candles (`lensLaneHeight` → the
 * price scale's bottom margin), so no candle is ever drawn under a row.
 *
 * Vocabulary-free like the rest of this folder: a row is a label, a color and
 * (bar, trades) hits.
 */
export interface LensLaneRow {
  label: string;
  color: string;
  /** The row's background — the host tells kinds of rows apart with it. */
  track: string;
  tints: readonly LensBarTint[];
}

export interface LensLaneState {
  rows: readonly LensLaneRow[];
  sizeLabels: ChartLensSizeLabels;
}

export const EMPTY_LENS_LANE: LensLaneState = { rows: [], sizeLabels: 'off' };

/** Row height at full size. Rows thin down to `MIN_ROW_H` before the lane gives up
 *  its numbers, so a long list of items squeezes rather than vanishes. */
const ROW_H = 20;
const MIN_ROW_H = 6;
/** Below this a row has no room for a number beside its marks. */
const TEXT_MIN_ROW_H = 11;
const ROW_GAP = 2;
const BOTTOM_PAD = 3;
/** Space between the lane's top edge and the lowest candle. */
const TOP_GAP = 6;
/** The most of the price pane the lane may take. */
const MAX_LANE_FRACTION = 0.4;
/** The shortest mark — a hit with no buy SOL still has to read as a hit. */
const MIN_MARK_H = 3;
const MIN_MARK_W = 2;
const MARK_WIDTH_RATIO = 0.7;
const FONT = '9px ui-monospace, SFMono-Regular, Menlo, monospace';
const AXIS_FONT = '9px ui-monospace, SFMono-Regular, Menlo, monospace';
const TEXT_PAD = 2;
const SEPARATOR = 'rgba(255, 255, 255, 0.12)';
/** Numbers are a quiet readout beside the marks: dim, unboxed, so the marks stay
 *  what the eye lands on. */
const TEXT_COLOR = 'rgba(255, 255, 255, 0.38)';

/** Row height for `rows` rows in a pane `paneH` tall, or `null` when none fit. */
function fitRowHeight(rows: number, paneH: number): number | null {
  if (rows <= 0) return null;
  const budget = paneH * MAX_LANE_FRACTION - BOTTOM_PAD - TOP_GAP;
  const h = Math.floor((budget + ROW_GAP) / rows) - ROW_GAP;
  if (h < MIN_ROW_H) return null;
  return Math.min(ROW_H, h);
}

/**
 * Pixels the lane takes at the bottom of a pane `paneH` tall — what the host
 * reserves under the candles and what the washes stop above. 0 with no rows.
 */
export function lensLaneHeight(rows: number, paneH: number): number {
  const h = fitRowHeight(rows, paneH);
  return h == null ? 0 : TOP_GAP + rows * (h + ROW_GAP) - ROW_GAP + BOTTOM_PAD;
}

/**
 * The row under pane-y `y`, or `null` when `y` is above the lane or out of every
 * row. A row's hover band takes half the gap on each side, so the pointer never
 * falls between two rows.
 */
export function laneRowAt(y: number, rows: number, paneH: number): number | null {
  const rowH = fitRowHeight(rows, paneH);
  if (rowH == null) return null;
  const first = paneH - lensLaneHeight(rows, paneH) + TOP_GAP;
  const i = Math.floor((y - first + ROW_GAP / 2) / (rowH + ROW_GAP));
  return i >= 0 && i < rows ? i : null;
}

/** One lane row's trades in one bar — what hovering its mark lists. */
export interface LensLaneHit {
  label: string;
  color: string;
  trades: readonly ChartTrade[];
}

/** SOL a mark stands for: buys, or buys and sells when every side is printed. */
function markSol(trades: readonly ChartTrade[], mode: ChartLensSizeLabels): number {
  let sol = 0;
  for (const t of trades) {
    if (t.amount_sol == null) continue;
    if (t.trade_type === 'buy' || mode === 'all') sol += t.amount_sol;
  }
  return sol;
}

/**
 * The number beside a mark. One buy prints its size; several print their sum and
 * count, `0.70 (2)`, so a bar of three small buys never reads as one big one.
 * `all` signs each side, `+0.70 −0.30`, so a sell never reads as a buy.
 */
export function laneText(trades: readonly ChartTrade[], mode: ChartLensSizeLabels): string | null {
  if (mode === 'off') return null;
  let buySol = 0;
  let buys = 0;
  let sellSol = 0;
  let sells = 0;
  for (const t of trades) {
    if (t.amount_sol == null) continue;
    if (t.trade_type === 'buy') {
      buySol += t.amount_sol;
      buys += 1;
    } else {
      sellSol += t.amount_sol;
      sells += 1;
    }
  }
  const part = (sol: number, n: number, sign: string) =>
    `${sign}${formatLensSol(sol)}${n > 1 ? ` (${n})` : ''}`;
  if (mode === 'buys') return buys > 0 ? part(buySol, buys, '') : null;
  const parts = [
    ...(buys > 0 ? [part(buySol, buys, '+')] : []),
    ...(sells > 0 ? [part(sellSol, sells, '−')] : []),
  ];
  return parts.length > 0 ? parts.join(' ') : null;
}

interface RenderedMark {
  /** Media-space center x. */
  x: number;
  /** 0..1 of the row height. */
  height: number;
  text: string | null;
}

interface RenderedRow {
  label: string;
  color: string;
  track: string;
  marks: RenderedMark[];
}

class LensLaneRenderer implements IPrimitivePaneRenderer {
  constructor(
    private readonly _rows: RenderedRow[],
    private readonly _markW: number,
  ) {}

  draw(target: CanvasRenderingTarget2D): void {
    if (this._rows.length === 0) return;
    target.useMediaCoordinateSpace(({ context: ctx, mediaSize }) => {
      const rowH = fitRowHeight(this._rows.length, mediaSize.height);
      if (rowH == null) return;
      const width = mediaSize.width;
      const laneTop = mediaSize.height - lensLaneHeight(this._rows.length, mediaSize.height);
      const withText = rowH >= TEXT_MIN_ROW_H;

      ctx.save();
      ctx.fillStyle = SEPARATOR;
      ctx.fillRect(0, laneTop + TOP_GAP / 2, width, 1);
      ctx.font = FONT;
      ctx.textBaseline = 'middle';

      let hidden = 0;
      let top = laneTop + TOP_GAP;
      for (const row of this._rows) {
        ctx.fillStyle = row.track;
        ctx.fillRect(0, top, width, rowH);

        const inView = row.marks.filter((m) => m.x >= -this._markW && m.x <= width + this._markW);
        ctx.fillStyle = row.color;
        for (const m of inView) {
          const h = Math.max(MIN_MARK_H, rowH * m.height);
          ctx.fillRect(m.x - this._markW / 2, top + rowH - h, this._markW, h);
        }

        if (withText) {
          // Numbers go right of their mark, in the gap before the next mark: a
          // number drawn across another mark cannot be read, so one that does not
          // fit its gap is dropped and counted into the hint. The gaps never
          // overlap, so neither can two numbers.
          for (let i = 0; i < inView.length; i++) {
            const m = inView[i];
            if (m.text == null) continue;
            const nextLeft = i + 1 < inView.length ? inView[i + 1].x - this._markW / 2 : width;
            const x1 = m.x + this._markW / 2 + 1;
            const x2 = x1 + ctx.measureText(m.text).width + TEXT_PAD * 2;
            if (x2 > nextLeft) {
              hidden += 1;
              continue;
            }
            ctx.fillStyle = TEXT_COLOR;
            ctx.fillText(m.text, x1 + TEXT_PAD, top + rowH / 2);
          }
        }
        top += rowH + ROW_GAP;
      }

      if (hidden > 0) {
        const hint = `${hidden} size${hidden === 1 ? '' : 's'} hidden · zoom in`;
        ctx.fillStyle = TEXT_COLOR;
        ctx.fillText(hint, 4 + TEXT_PAD, laneTop - 6);
      }
      ctx.restore();
    });
  }
}

/** `text` cut in the middle to fit `maxW` — both ends of an address or an ix
 *  sequence are what tell two of them apart. */
function fitMiddle(ctx: CanvasRenderingContext2D, text: string, maxW: number): string {
  if (ctx.measureText(text).width <= maxW) return text;
  let keep = text.length - 1;
  while (keep > 2) {
    const head = Math.ceil(keep / 2);
    const cut = `${text.slice(0, head)}…${text.slice(text.length - (keep - head))}`;
    if (ctx.measureText(cut).width <= maxW) return cut;
    keep -= 1;
  }
  return text.slice(0, 1) + '…';
}

/** Row names in the price-axis gutter, level with their rows. */
class LensLaneAxisRenderer implements IPrimitivePaneRenderer {
  constructor(private readonly _rows: RenderedRow[]) {}

  draw(target: CanvasRenderingTarget2D): void {
    if (this._rows.length === 0) return;
    target.useMediaCoordinateSpace(({ context: ctx, mediaSize }) => {
      const rowH = fitRowHeight(this._rows.length, mediaSize.height);
      if (rowH == null) return;
      let top = mediaSize.height - lensLaneHeight(this._rows.length, mediaSize.height) + TOP_GAP;
      ctx.save();
      ctx.font = AXIS_FONT;
      ctx.textBaseline = 'middle';
      for (const row of this._rows) {
        ctx.fillStyle = CHART_COLORS.background;
        ctx.fillRect(0, top - 1, mediaSize.width, rowH + 2);
        ctx.fillStyle = row.track;
        ctx.fillRect(0, top, mediaSize.width, rowH);
        ctx.fillStyle = row.color;
        ctx.fillRect(0, top, 3, rowH);
        ctx.fillText(fitMiddle(ctx, row.label, mediaSize.width - 8), 6, top + rowH / 2);
        top += rowH + ROW_GAP;
      }
      ctx.restore();
    });
  }
}

class LensLanePaneView implements IPrimitivePaneView {
  constructor(private readonly _renderer: IPrimitivePaneRenderer) {}

  /** Over everything in the pane — the lane owns its strip, nothing draws there. */
  zOrder(): PrimitivePaneViewZOrder {
    return 'top';
  }

  renderer(): IPrimitivePaneRenderer {
    return this._renderer;
  }
}

export class LensLanePlugin
  implements ISeriesPrimitiveBase<SeriesAttachedParameter<UTCTimestamp>>
{
  private _chart: IChartApiBase<UTCTimestamp> | null = null;
  private _requestUpdate: (() => void) | null = null;
  /** Per row: each hit's mark height and text, built once per `setState` — a pan
   *  or zoom only re-projects x. */
  private _prepared: { label: string; color: string; track: string; hits: Omit<RenderedMark, 'x'>[]; times: number[] }[] = [];
  /** Per row: bar key → that bar's matched trades, for the hover. */
  private _tradesByBar: Map<number, readonly ChartTrade[]>[] = [];
  private _rendered: RenderedRow[] = [];
  private _markW = MIN_MARK_W;

  attached({ chart, requestUpdate }: SeriesAttachedParameter<UTCTimestamp>): void {
    this._chart = chart;
    this._requestUpdate = requestUpdate;
  }

  detached(): void {
    this._chart = null;
    this._requestUpdate = null;
  }

  setState(state: LensLaneState): void {
    const sols = state.rows.map((row) =>
      row.tints.map((t) => markSol(t.trades, state.sizeLabels)),
    );
    // One scale for every row: the biggest bar of any row over the whole token. So
    // the same SOL draws the same height in any two rows, and a mark's height does
    // not jump as the view pans. Square root, so a 10x bigger bar reads clearly
    // bigger without flattening every other mark to the floor.
    let max = 0;
    for (const row of sols) for (const sol of row) if (sol > max) max = sol;
    this._prepared = state.rows.map((row, r) => ({
      label: row.label,
      color: row.color,
      track: row.track,
      times: row.tints.map((t) => t.barTime),
      hits: row.tints.map((t, i) => ({
        height: max > 0 ? Math.sqrt(sols[r][i] / max) : 0,
        text: laneText(t.trades, state.sizeLabels),
      })),
    }));
    this._tradesByBar = state.rows.map((row) => new Map(row.tints.map((t) => [t.barTime, t.trades])));
    this._requestUpdate?.();
  }

  /**
   * The row mark under the pointer: the row at pane-y `y` and its trades in the
   * hovered bar, or `null` when the pointer is off the lane or that row has no
   * mark there. The host shows only this row's trades while it is non-null.
   */
  hitAt(y: number, barTime: number): LensLaneHit | null {
    const chart = this._chart;
    if (!chart || this._prepared.length === 0) return null;
    const row = laneRowAt(y, this._prepared.length, chart.paneSize(0).height);
    if (row == null) return null;
    const trades = this._tradesByBar[row]?.get(barTime);
    if (!trades || trades.length === 0) return null;
    const { label, color } = this._prepared[row];
    return { label, color, trades };
  }

  /** True when pane-y `y` is inside the lane strip — the candle tooltip stays off there. */
  containsY(y: number): boolean {
    const chart = this._chart;
    if (!chart || this._prepared.length === 0) return false;
    const paneH = chart.paneSize(0).height;
    const h = lensLaneHeight(this._prepared.length, paneH);
    return h > 0 && y >= paneH - h;
  }

  updateAllViews(): void {
    const chart = this._chart;
    if (!chart || this._prepared.length === 0) {
      this._rendered = [];
      return;
    }
    const ts = chart.timeScale();
    const slot = ts.options().barSpacing ?? DEFAULT_BAR_SPACING;
    this._markW = Math.max(MIN_MARK_W, slot * MARK_WIDTH_RATIO);
    this._rendered = this._prepared.map((row) => {
      const marks: RenderedMark[] = [];
      row.times.forEach((time, i) => {
        const x = ts.timeToCoordinate(time as UTCTimestamp);
        if (x != null) marks.push({ x, ...row.hits[i] });
      });
      return { label: row.label, color: row.color, track: row.track, marks };
    });
  }

  paneViews(): readonly IPrimitivePaneView[] {
    return [new LensLanePaneView(new LensLaneRenderer(this._rendered, this._markW))];
  }

  priceAxisPaneViews(): readonly IPrimitivePaneView[] {
    return [new LensLanePaneView(new LensLaneAxisRenderer(this._rendered))];
  }
}

/** Same widening cast the other primitives use — `UTCTimestamp` is a `Time`. */
export function asLensLanePrimitive(p: LensLanePlugin): ISeriesPrimitive {
  return p as unknown as ISeriesPrimitive;
}
