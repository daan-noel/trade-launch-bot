/**
 * The geometry of the overlap diagram: three same-shaped ellipses on one axis whose
 * areas, and the areas they share, follow the token counts.
 *
 * - Pool, passing and his areas are in the ratio of their token counts.
 * - His ellipse sits so the share of it outside the pool is the share of his tokens
 *   outside the pool.
 * - The passing ellipse sits inside the pool, so the share of his ellipse inside it
 *   is the share of his tokens that pass.
 *
 * A count too small to see keeps `MIN_SCALE`, and the passing ellipse stays a hair
 * inside the pool's outline, so both outlines stay visible when the counts are equal.
 */

export interface Ellipse {
  cx: number;
  cy: number;
  rx: number;
  ry: number;
}

/** A part of the drawing that holds counts, left to right. */
export type Region = 'poolOnly' | 'passOnly' | 'passHis' | 'poolHis' | 'outside';
export const REGIONS: readonly Region[] = ['poolOnly', 'passOnly', 'passHis', 'poolHis', 'outside'];

export interface OverlapCounts {
  pool: number;
  pass: number;
  his: number;
  /** His tokens that are passing tokens, and his tokens outside the pool. */
  hisInPass: number;
  hisOutside: number;
}

export interface OverlapLayout {
  /** `null` for an empty set. */
  pool: Ellipse | null;
  pass: Ellipse | null;
  his: Ellipse | null;
  /** Where each region's counts go; absent for a region too thin to point at. */
  spots: Partial<Record<Region, Spot>>;
}

/**
 * `fits`: the centre of a free box of the asked size inside the region. Otherwise
 * the region's deepest point, for a label drawn elsewhere to point at.
 */
export interface Spot {
  x: number;
  y: number;
  fits: boolean;
}

/** The box a region's counts need, in drawing units. */
export interface Box {
  w: number;
  h: number;
}

/** Width over height of every ellipse. */
const ASPECT = 1.8;
/** The smallest radius drawn, as a share of the largest. */
const MIN_SCALE = 0.12;
/** The passing ellipse's largest radius, as a share of the pool's. */
const INSET = 0.97;
const PAD = 8;
/** Grid step, in drawing units, of the search for a region's deepest point. */
const STEP = 3;

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

const inside = (e: Ellipse | null, x: number, y: number) =>
  e != null && ((x - e.cx) / e.rx) ** 2 + ((y - e.cy) / e.ry) ** 2 <= 1;

function regionAt(l: Pick<OverlapLayout, 'pool' | 'pass' | 'his'>, x: number, y: number): Region | null {
  const his = inside(l.his, x, y);
  const pass = inside(l.pass, x, y);
  const pool = pass || inside(l.pool, x, y);
  if (his) return pass ? 'passHis' : pool ? 'poolHis' : 'outside';
  return pass ? 'passOnly' : pool ? 'poolOnly' : null;
}

/**
 * The drawing inside a `width` by `height` box. `need` names the regions that show
 * counts, each with the box its counts take.
 */
export function overlapLayout(
  c: OverlapCounts,
  width: number,
  height: number,
  need: Partial<Record<Region, Box>>,
): OverlapLayout {
  const top = Math.max(c.pool, c.his, 1);
  const scale = (n: number) => (n > 0 ? Math.max(Math.sqrt(n / top), MIN_SCALE) : 0);
  // Circles on one axis; the x axis stretches by `ASPECT` when drawn.
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
  const k = Math.min((width - 2 * PAD) / (ASPECT * (right - left)), (height - 2 * PAD) / (2 * tall));
  const ox = width / 2 - (k * ASPECT * (left + right)) / 2;
  const cy = height / 2;
  const ellipse = (x: number, r: number): Ellipse | null =>
    r > 0 ? { cx: ox + k * ASPECT * x, cy, rx: k * ASPECT * r, ry: k * r } : null;
  const shapes = { pool: ellipse(0, rPool), pass: ellipse(xPass, rPass), his: ellipse(xHis, rHis) };
  return { ...shapes, spots: spots(shapes, width, height, need) };
}

/**
 * Each wanted region's spot, on a grid. A cell's depth is its steps to the nearest
 * cell outside the region; the spot is the deepest cell whose box lies wholly in the
 * region, else the deepest cell.
 */
function spots(
  shapes: Pick<OverlapLayout, 'pool' | 'pass' | 'his'>,
  width: number,
  height: number,
  need: Partial<Record<Region, Box>>,
): OverlapLayout['spots'] {
  const cols = Math.floor(width / STEP);
  const rows = Math.floor(height / STEP);
  const cell: (Region | null)[] = [];
  for (let j = 0; j < rows; j += 1) {
    for (let i = 0; i < cols; i += 1) cell.push(regionAt(shapes, (i + 0.5) * STEP, (j + 0.5) * STEP));
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
