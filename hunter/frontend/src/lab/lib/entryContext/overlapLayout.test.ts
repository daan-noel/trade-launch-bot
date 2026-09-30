import { describe, expect, it } from 'vitest';
import { overlapLayout, type Ellipse, type OverlapCounts } from './overlapLayout';

const W = 780;
const H = 250;
const area = (e: Ellipse) => Math.PI * e.rx * e.ry;
const inside = (e: Ellipse, x: number, y: number) => ((x - e.cx) / e.rx) ** 2 + ((y - e.cy) / e.ry) ** 2 <= 1;

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

const counts: OverlapCounts = { pool: 2353, pass: 536, his: 184, hisInPass: 125, hisOutside: 4 };

describe('overlapLayout', () => {
  it('draws each area in the ratio of its token count', () => {
    const l = overlapLayout(counts, W, H, {});
    expect(area(l.pass!) / area(l.pool!)).toBeCloseTo(536 / 2353, 2);
    expect(area(l.his!) / area(l.pool!)).toBeCloseTo(184 / 2353, 2);
  });

  it('splits his ellipse as his tokens split', () => {
    const l = overlapLayout(counts, W, H, {});
    expect(share(l.his!, l.pool!)).toBeCloseTo(180 / 184, 1);
    expect(share(l.his!, l.pass!)).toBeCloseTo(125 / 184, 1);
  });

  it('draws the passing set almost as large as an equal pool', () => {
    const l = overlapLayout({ pool: 2000, pass: 2000, his: 100, hisInPass: 100, hisOutside: 0 }, W, H, {});
    expect(l.pass!.rx / l.pool!.rx).toBeGreaterThan(0.95);
    expect(share(l.his!, l.pool!)).toBe(1);
  });

  it('leaves out an empty set, and tells a region with room from one without', () => {
    const box = { w: 120, h: 24 };
    const l = overlapLayout({ pool: 1000, pass: 0, his: 40, hisInPass: 0, hisOutside: 2 }, W, H, {
      poolOnly: box,
      poolHis: box,
      outside: box,
      passHis: box,
    });
    expect(l.pass).toBeNull();
    expect(l.spots.passHis).toBeUndefined();
    const pool = l.spots.poolOnly!;
    expect(pool.fits).toBe(true);
    expect(inside(l.pool!, pool.x, pool.y) && !inside(l.his!, pool.x, pool.y)).toBe(true);
    const out = l.spots.outside!;
    expect(out.fits).toBe(false);
    expect(inside(l.his!, out.x, out.y) && !inside(l.pool!, out.x, out.y)).toBe(true);
  });
});
