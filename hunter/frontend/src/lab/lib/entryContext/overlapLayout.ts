/**
 * The geometry of the overlap diagram, to one scale: a fixed area per token.
 *
 * - The pool, the passing set and his tokens are three ellipses of one shape on one
 *   axis, their areas in the ratio of their token counts.
 * - His ellipse sits so the share of it outside the pool is the share of his tokens
 *   outside the pool.
 * - The passing ellipse sits inside the pool, so the share of his ellipse inside it
 *   is the share of his tokens that pass.
 * - A group of his tokens can be a circle inside his ellipse, in the part it belongs
 *   to: over the passing ellipse, over the pool only, or outside the pool.
 *
 * A set too small to see keeps a minimum size, and the passing ellipse stays a hair
 * inside the pool's outline, so both outlines stay visible when the counts are equal.
 */

import type { AreaKey, Zone } from './overlap';

export interface Ellipse {
  cx: number;
  cy: number;
  rx: number;
  ry: number;
}

/** One group of his tokens, as a circle. */
export interface Bubble {
  key: AreaKey;
  x: number;
  y: number;
  r: number;
}

/** A part of the drawing, left to right. */
export type Region = 'poolOnly' | 'passOnly' | 'passHis' | 'poolHis' | 'outside';
export const REGIONS: readonly Region[] = ['poolOnly', 'passOnly', 'passHis', 'poolHis', 'outside'];

/** The part of his ellipse a zone's tokens sit in. */
const HIS_REGION: Record<Zone, Region> = { pass: 'passHis', pool: 'poolHis', outside: 'outside' };

/**
 * `fits`: the centre of a free box of the asked size inside the region. Otherwise
 * the region's deepest point, for a label drawn elsewhere to point at.
 */
export interface Spot {
  x: number;
  y: number;
  fits: boolean;
}

/** The box a region's count needs, in drawing units. */
export interface Box {
  w: number;
  h: number;
}

export interface OverlapInput {
  pool: number;
  pass: number;
  his: number;
  /** His tokens that are passing tokens, and his tokens outside the pool. */
  hisInPass: number;
  hisOutside: number;
  /** His areas drawn as circles, with their token counts. */
  groups: readonly { key: AreaKey; n: number }[];
}

export interface OverlapLayout {
  /** `null` for an empty set. */
  pool: Ellipse | null;
  pass: Ellipse | null;
  his: Ellipse | null;
  bubbles: Bubble[];
  /** Where each wanted region's count goes; absent for a region with no free cell. */
  spots: Partial<Record<Region, Spot>>;
}

/** Width over height of the ellipses. */
const ASPECT = 1.8;
/** The smallest ellipse radius drawn, as a share of the largest. */
const MIN_SCALE = 0.12;
/** The passing ellipse's largest radius, as a share of the pool's. */
const INSET = 0.97;
/** The smallest circle radius drawn. */
const MIN_BUBBLE = 5;
const PAD = 8;
/** Room above the shapes for the pool's title. */
const HEAD = 20;
/** Free space kept around a circle. */
const GAP = 3;
/** Grid step, in drawing units, of the searches. */
const STEP = 3;
/** Points checked on a circle's outline. */
const RIM = 16;

const zoneOf = (key: AreaKey) => key.split(':')[0] as Zone;
const inside = (e: Ellipse | null, x: number, y: number, grow = 0) =>
  e != null && ((x - e.cx) / (e.rx + grow)) ** 2 + ((y - e.cy) / (e.ry + grow)) ** 2 <= 1;

/** Area shared by two circles of radii `a` and `b` whose centres are `d` apart. */
function lens(a: number, b: number, d: number): number {
  if (d >= a + b) return 0;
  if (d <= Math.abs(a - b)) return Math.PI * Math.min(a, b) ** 2;
  const x = Math.acos((d * d + a * a - b * b) / (2 * d * a));
  const y = Math.acos((d * d + b * b - a * a) / (2 * d * b));
  return a * a * x + b * b * y - 0.5 * Math.sqrt((-d + a + b) * (d + a - b) * (d - a + b) * (d + a + b));
}

/** The largest centre distance at which at least `share` of circle `a` lies inside circle `b`. */
function distanceFor(a: number, b: number, share: number): number {
  let lo = 0;
  let hi = a + b;
  for (let i = 0; i < 40; i += 1) {
    const mid = (lo + hi) / 2;
    if (lens(a, b, mid) / (Math.PI * a * a) >= share) lo = mid;
    else hi = mid;
  }
  return lo;
}

type Shapes = Pick<OverlapLayout, 'pool' | 'pass' | 'his'>;

function regionAt(l: Shapes, x: number, y: number): Region | null {
  const pass = inside(l.pass, x, y);
  const pool = pass || inside(l.pool, x, y);
  if (inside(l.his, x, y)) return pass ? 'passHis' : pool ? 'poolHis' : 'outside';
  return pass ? 'passOnly' : pool ? 'poolOnly' : null;
}

/**
 * The drawing inside a `width` by `height` box. `need` names the regions that show
 * a count as text, each with the box the text takes.
 */
export function overlapLayout(
  c: OverlapInput,
  width: number,
  height: number,
  need: Partial<Record<Region, Box>>,
): OverlapLayout {
  const top = Math.max(c.pool, c.his, 1);
  // Circles on one axis, in units of the largest set's radius; x stretches by `ASPECT` when drawn.
  const scale = (n: number) => (n > 0 ? Math.max(Math.sqrt(n / top), MIN_SCALE) : 0);
  const rPool = scale(c.pool);
  const rHis = scale(c.his);
  const rPass = Math.min(scale(c.pass), rPool * INSET);
  const xHis = rPool > 0 && rHis > 0 ? distanceFor(rHis, rPool, 1 - c.hisOutside / c.his) : 0;
  const reach = Math.max(rPool - rPass, 0);
  const xPass =
    rHis > 0 && rPass > 0
      ? Math.min(Math.max(xHis - distanceFor(rHis, rPass, c.hisInPass / c.his), -reach), reach)
      : 0;

  const circles = [
    [0, rPool],
    [xPass, rPass],
    [xHis, rHis],
  ].filter(([, r]) => r > 0);
  const left = Math.min(...circles.map(([x, r]) => x - r), -MIN_SCALE);
  const right = Math.max(...circles.map(([x, r]) => x + r), MIN_SCALE);
  const tall = Math.max(...circles.map(([, r]) => r), MIN_SCALE);
  const k = Math.min((width - 2 * PAD) / (ASPECT * (right - left)), (height - HEAD - 2 * PAD) / (2 * tall));
  const ox = width / 2 - (k * ASPECT * (left + right)) / 2;
  const cy = HEAD + (height - HEAD) / 2;
  const ellipse = (x: number, r: number): Ellipse | null =>
    r > 0 ? { cx: ox + k * ASPECT * x, cy, rx: k * ASPECT * r, ry: k * r } : null;
  const shapes: Shapes = { pool: ellipse(0, rPool), pass: ellipse(xPass, rPass), his: ellipse(xHis, rHis) };
  const bubbles = shapes.his ? place(shapes, shapes.his, c.groups, k * Math.sqrt(ASPECT / top)) : [];
  return { ...shapes, bubbles, spots: spots(shapes, bubbles, width, height, need) };
}

/**
 * His circles, largest first, each at the free place in its part of his ellipse
 * nearest the ellipse's centre. `unit` is the radius of a one-token circle.
 */
function place(l: Shapes, his: Ellipse, groups: OverlapInput['groups'], unit: number): Bubble[] {
  const bubbles: Bubble[] = [];
  const sized = groups
    .filter((g) => g.n > 0)
    .map((g) => ({ key: g.key, r: Math.max(unit * Math.sqrt(g.n), MIN_BUBBLE) }))
    .sort((a, b) => b.r - a.r);
  for (const { key, r } of sized) {
    const region = HIS_REGION[zoneOf(key)];
    const free = (x: number, y: number) => !bubbles.some((b) => Math.hypot(b.x - x, b.y - y) < b.r + r + GAP);
    // The whole circle in its part, clear of the outlines.
    const whole = (x: number, y: number) => {
      if (!free(x, y) || regionAt(l, x, y) !== region) return false;
      for (let i = 0; i < RIM; i += 1) {
        const a = (2 * Math.PI * i) / RIM;
        if (regionAt(l, x + (r + GAP) * Math.cos(a), y + (r + GAP) * Math.sin(a)) !== region) return false;
      }
      return true;
    };
    // With no room for the whole circle, its centre alone stays in the part.
    const centred = (x: number, y: number) => free(x, y) && regionAt(l, x, y) === region;
    const nearest = (allowed: (x: number, y: number) => boolean) => {
      let best: { x: number; y: number } | null = null;
      let bestGap = Infinity;
      for (let x = his.cx - his.rx; x <= his.cx + his.rx; x += STEP) {
        for (let y = his.cy - his.ry; y <= his.cy + his.ry; y += STEP) {
          const gap = Math.hypot(x - his.cx, y - his.cy);
          if (gap < bestGap && allowed(x, y)) [best, bestGap] = [{ x, y }, gap];
        }
      }
      return best;
    };
    bubbles.push({ key, r, ...(nearest(whole) ?? nearest(centred) ?? { x: his.cx, y: his.cy }) });
  }
  return bubbles;
}

/**
 * Each wanted region's spot, on a grid. A cell's depth is its steps to the nearest
 * cell outside the region; the spot is the deepest cell whose box lies wholly in the
 * region, else the deepest cell. A cell under a circle is in no region.
 */
function spots(
  l: Shapes,
  bubbles: readonly Bubble[],
  width: number,
  height: number,
  need: Partial<Record<Region, Box>>,
): OverlapLayout['spots'] {
  const cols = Math.floor(width / STEP);
  const rows = Math.floor(height / STEP);
  const cell: (Region | null)[] = [];
  for (let j = 0; j < rows; j += 1) {
    for (let i = 0; i < cols; i += 1) {
      const x = (i + 0.5) * STEP;
      const y = (j + 0.5) * STEP;
      cell.push(bubbles.some((b) => Math.hypot(b.x - x, b.y - y) < b.r + GAP) ? null : regionAt(l, x, y));
    }
  }
  const out: OverlapLayout['spots'] = {};
  for (const region of REGIONS) {
    const box = need[region];
    if (!box) continue;
    const depth = cell.map((r) => (r === region ? cols + rows : 0));
    const relax = (n: number) => {
      if (depth[n] === 0) return;
      const i = n % cols;
      const j = (n - i) / cols;
      const up = j > 0 ? depth[n - cols] : 0;
      const down = j < rows - 1 ? depth[n + cols] : 0;
      const before = i > 0 ? depth[n - 1] : 0;
      const after = i < cols - 1 ? depth[n + 1] : 0;
      depth[n] = Math.min(depth[n], Math.min(up, down, before, after) + 1);
    };
    for (let n = 0; n < depth.length; n += 1) relax(n);
    for (let n = depth.length - 1; n >= 0; n -= 1) relax(n);

    // Region cells above and left of each grid corner, so a box is four lookups.
    const sum = new Array<number>((cols + 1) * (rows + 1)).fill(0);
    const at = (i: number, j: number) => j * (cols + 1) + i;
    for (let j = 0; j < rows; j += 1) {
      for (let i = 0; i < cols; i += 1) {
        sum[at(i + 1, j + 1)] =
          (depth[j * cols + i] > 0 ? 1 : 0) + sum[at(i, j + 1)] + sum[at(i + 1, j)] - sum[at(i, j)];
      }
    }
    const halfW = Math.ceil(box.w / 2 / STEP);
    const halfH = Math.ceil(box.h / 2 / STEP);
    const full = (2 * halfW + 1) * (2 * halfH + 1);

    let deep = -1;
    let fit = -1;
    for (let n = 0; n < depth.length; n += 1) {
      if (depth[n] === 0) continue;
      if (deep < 0 || depth[n] > depth[deep]) deep = n;
      const i = n % cols;
      const j = (n - i) / cols;
      if (i < halfW || j < halfH || i + halfW >= cols || j + halfH >= rows) continue;
      const held =
        sum[at(i + halfW + 1, j + halfH + 1)] -
        sum[at(i - halfW, j + halfH + 1)] -
        sum[at(i + halfW + 1, j - halfH)] +
        sum[at(i - halfW, j - halfH)];
      if (held === full && (fit < 0 || depth[n] > depth[fit])) fit = n;
    }
    const n = fit >= 0 ? fit : deep;
    if (n < 0) continue;
    out[region] = { x: ((n % cols) + 0.5) * STEP, y: (Math.floor(n / cols) + 0.5) * STEP, fits: fit >= 0 };
  }
  return out;
}
