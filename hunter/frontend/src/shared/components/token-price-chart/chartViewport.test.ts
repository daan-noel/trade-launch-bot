import { describe, expect, it } from 'vitest';
import type { LogicalRange, UTCTimestamp } from 'lightweight-charts';
import { barsShape, shiftLogicalRange } from './chartViewport';

/**
 * The regression this locks: every live trade re-runs `setData` with a whole new
 * bar array, so the saved viewport has to be translated onto the new indices. A
 * time-based restore snapped its endpoints onto bar boundaries and walked the
 * user's zoom off target one trade at a time; the index shift is exact.
 */

const t = (n: number) => n as UTCTimestamp;
const barsAt = (times: number[]) => times.map((time) => ({ time: t(time) }));
const range = (from: number, to: number) => ({ from, to }) as unknown as LogicalRange;

describe('shiftLogicalRange', () => {
  it('holds a scrolled-back window in place when a trade appends a bar', () => {
    const prev = barsShape(barsAt([10, 11, 12, 13, 14]));
    const next = barsAt([10, 11, 12, 13, 14, 15]);

    expect(shiftLogicalRange(range(0.5, 2.5), prev, next)).toEqual({ from: 0.5, to: 2.5 });
  });

  it('follows the new bar when the user was parked at the live edge', () => {
    const prev = barsShape(barsAt([10, 11, 12, 13, 14]));
    const next = barsAt([10, 11, 12, 13, 14, 15]);

    // to = 4 is the last bar's index, i.e. the right edge.
    expect(shiftLogicalRange(range(2, 4), prev, next)).toEqual({ from: 3, to: 5 });
  });

  it('compensates when bars are trimmed off the FRONT', () => {
    const prev = barsShape(barsAt([10, 11, 12, 13, 14, 15, 16, 17]));
    const next = barsAt([12, 13, 14, 15, 16, 17]);

    // Bars 10 and 11 are gone, so old indices 3..5 (times 13..15) are now 1..3.
    // A first-bar anchor cannot see this and would leave the window two bars off.
    expect(shiftLogicalRange(range(3, 5), prev, next)).toEqual({ from: 1, to: 3 });
  });

  it('compensates when older bars are prepended', () => {
    const prev = barsShape(barsAt([12, 13, 14]));
    const next = barsAt([10, 11, 12, 13, 14]);

    expect(shiftLogicalRange(range(0, 1), prev, next)).toEqual({ from: 2, to: 3 });
  });

  it('leaves the range alone with no baseline or no bars', () => {
    expect(shiftLogicalRange(range(1, 2), null, barsAt([10, 11]))).toEqual({ from: 1, to: 2 });
    expect(shiftLogicalRange(range(1, 2), barsShape(barsAt([10])), [])).toEqual({
      from: 1,
      to: 2,
    });
  });
});
