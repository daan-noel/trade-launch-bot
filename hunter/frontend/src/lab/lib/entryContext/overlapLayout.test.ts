import { describe, expect, it } from 'vitest';
import { overlapLayout, type Bubble, type Ellipse, type OverlapInput } from './overlapLayout';

const W = 680;
const H = 400;
const area = (e: Ellipse) => Math.PI * e.rx * e.ry;
const disc = (b: Bubble) => Math.PI * b.r * b.r;
const inside = (e: Ellipse, x: number, y: number) => ((x - e.cx) / e.rx) ** 2 + ((y - e.cy) / e.ry) ** 2 <= 1;
/** Whether the whole circle lies inside the ellipse. */
const holds = (e: Ellipse, b: Bubble) =>
  [0, 1, 2, 3, 4, 5, 6, 7].every((i) =>
    inside(e, b.x + b.r * Math.cos((Math.PI * i) / 4), b.y + b.r * Math.sin((Math.PI * i) / 4)),
  );

/** Share of ellipse `a` that lies inside `b`, by sampling. */
function share(a: Ellipse, b: Ellipse): number {
  let all = 0;
  let hit = 0;
  for (let x = a.cx - a.rx; x <= a.cx + a.rx; x += 0.5) {
    for (let y = a.cy - a.ry; y <= a.cy + a.ry; y += 0.5) {
      if (!inside(a, x, y)) continue;
      all += 1;
      if (inside(b, x, y)) hit += 1;
    }
  }
  return hit / all;
}

const input: OverlapInput = {
  pool: 2353,
  pass: 536,
  his: 184,
  hisInPass: 125,
  hisOutside: 4,
  groups: [
    { key: 'pass:point', n: 27 },
    { key: 'pass:signal', n: 13 },
    { key: 'pool:signal', n: 5 },
  ],
};

describe('overlapLayout', () => {
  const l = overlapLayout(input, W, H, {});
  const bubble = (key: string) => l.bubbles.find((b) => b.key === key)!;

  it('draws every shape at one area per token', () => {
    expect(area(l.pass!) / area(l.pool!)).toBeCloseTo(536 / 2353, 2);
    expect(area(l.his!) / area(l.pool!)).toBeCloseTo(184 / 2353, 2);
    expect(disc(bubble('pass:point')) / area(l.his!)).toBeCloseTo(27 / 184, 2);
  });

  it('splits his ellipse as his tokens split', () => {
    expect(share(l.his!, l.pool!)).toBeCloseTo(180 / 184, 1);
    expect(share(l.his!, l.pass!)).toBeCloseTo(125 / 184, 1);
  });

  it('keeps each of his circles in its part of his ellipse, apart from the others', () => {
    for (const key of ['pass:point', 'pass:signal']) {
      expect(holds(l.his!, bubble(key))).toBe(true);
      expect(holds(l.pass!, bubble(key))).toBe(true);
    }
    const b = bubble('pool:signal');
    expect(holds(l.his!, b) && holds(l.pool!, b)).toBe(true);
    expect(inside(l.pass!, b.x, b.y)).toBe(false);
    for (const x of l.bubbles) {
      for (const y of l.bubbles) if (x !== y) expect(Math.hypot(x.x - y.x, x.y - y.y)).toBeGreaterThan(x.r + y.r);
    }
  });

  it('draws the passing set almost as large as an equal pool', () => {
    const same = overlapLayout({ pool: 2000, pass: 2000, his: 100, hisInPass: 100, hisOutside: 0, groups: [] }, W, H, {});
    expect(same.pass!.rx / same.pool!.rx).toBeGreaterThan(0.95);
    expect(share(same.his!, same.pool!)).toBe(1);
  });

  it('leaves out an empty set, and tells a region with room from one without', () => {
    const box = { w: 120, h: 24 };
    const none = overlapLayout({ pool: 1000, pass: 0, his: 40, hisInPass: 0, hisOutside: 2, groups: [] }, W, H, {
      poolOnly: box,
      outside: box,
      passHis: box,
    });
    expect(none.pass).toBeNull();
    expect(none.spots.passHis).toBeUndefined();
    const pool = none.spots.poolOnly!;
    expect(pool.fits).toBe(true);
    expect(inside(none.pool!, pool.x, pool.y) && !inside(none.his!, pool.x, pool.y)).toBe(true);
    const out = none.spots.outside!;
    expect(out.fits).toBe(false);
    expect(inside(none.his!, out.x, out.y) && !inside(none.pool!, out.x, out.y)).toBe(true);
  });
});
