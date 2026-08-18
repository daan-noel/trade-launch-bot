import { describe, expect, it } from 'vitest';

import { fitLaneHeight, snapSpanToBars } from './timeBandsPlugin';

/**
 * The band's whole claim is that it lines up with the candles above it, and the
 * only thing standing between a readout instant and a pixel is this snap.
 *
 * The array is each bar's wall-clock **END** (`buildBarWallEndSec`), NOT a bar key —
 * which is exactly what an earlier version of these cases got wrong, and why the end
 * edge silently dropped a bar in production while the suite stayed green. Named here
 * so the fixture cannot be misread again: `ends[i]` closes the bar that opened when
 * `ends[i-1]` closed.
 */
describe('snapSpanToBars', () => {
  //        bar A      B          C          D          E
  //  ends: 100   (100,110]  (110,120]  (120,130]  (130,140]
  const ends = [100, 110, 120, 130, 140];

  it('covers every bar the span overlaps, both edges rounded outward', () => {
    // 105 falls in B, 135 falls in E — so B..E, not B..D.
    expect(snapSpanToBars(ends, 105, 135)).toEqual({ from: 110, to: 140 });
  });

  // The regression this fixture exists for: rounding the end edge INWARD ("last bar
  // to have finished by `to`") loses the bar the span ends inside, understating every
  // span by a whole bar.
  it('does not drop the bar the span ends inside', () => {
    // 111 falls in C, 121 falls in D. The old inward rounding answered C..C.
    expect(snapSpanToBars(ends, 111, 121)).toEqual({ from: 120, to: 130 });
  });

  it('keeps a span that lands exactly on bar ends exact', () => {
    // An instant AT a bar's end still belongs to that bar.
    expect(snapSpanToBars(ends, 110, 130)).toEqual({ from: 110, to: 130 });
  });

  // A condition that holds for a fraction of a bar covers no bar exactly. Dropping
  // it would silently hide every brief satisfaction, which is the one thing the
  // band exists to make visible.
  it('collapses a sub-bar span onto the bar containing it', () => {
    expect(snapSpanToBars(ends, 121, 123)).toEqual({ from: 130, to: 130 });
    expect(snapSpanToBars(ends, 101, 109)).toEqual({ from: 110, to: 110 });
  });

  it('clamps a span that runs off either end', () => {
    expect(snapSpanToBars(ends, 0, 115)).toEqual({ from: 100, to: 120 });
    expect(snapSpanToBars(ends, 135, 9_999)).toEqual({ from: 140, to: 140 });
    expect(snapSpanToBars(ends, 0, 9_999)).toEqual({ from: 100, to: 140 });
  });

  it('collapses a span entirely outside the bars onto the nearest end', () => {
    expect(snapSpanToBars(ends, 10, 20)).toEqual({ from: 100, to: 100 });
    expect(snapSpanToBars(ends, 500, 600)).toEqual({ from: 140, to: 140 });
  });

  it('has nothing to snap to without bars', () => {
    expect(snapSpanToBars([], 1, 2)).toBeNull();
  });
});

describe('fitLaneHeight', () => {
  // A modal chart, the case that actually ships.
  it('keeps full-height lanes while they fit', () => {
    expect(fitLaneHeight(4, 220)).toBe(9);
  });

  // The point of thinning: adding a condition must never delete the whole band.
  it('thins instead of disappearing as lanes are added', () => {
    const heights = [6, 8, 10, 12].map((n) => fitLaneHeight(n, 220));
    expect(heights.every((h) => h != null)).toBe(true);
    for (let i = 1; i < heights.length; i++) {
      expect(heights[i]!).toBeLessThanOrEqual(heights[i - 1]!);
    }
  });

  it('gives up rather than drawing lanes too thin to see', () => {
    expect(fitLaneHeight(40, 220)).toBeNull();
    expect(fitLaneHeight(4, 40)).toBeNull();
    expect(fitLaneHeight(0, 220)).toBeNull();
  });
});
