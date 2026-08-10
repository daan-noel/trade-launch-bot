import { describe, expect, it } from 'vitest';

import { fitLaneHeight, snapSpanToBars } from './timeBandsPlugin';

/**
 * The band's whole claim is that it lines up with the candles above it, and the
 * only thing standing between a readout instant and a pixel is this snap.
 */
describe('snapSpanToBars', () => {
  const bars = [100, 110, 120, 130, 140];

  it('covers the bars inside the span', () => {
    expect(snapSpanToBars(bars, 105, 135)).toEqual({ from: 110, to: 130 });
  });

  it('keeps an exactly-aligned span exact', () => {
    expect(snapSpanToBars(bars, 110, 130)).toEqual({ from: 110, to: 130 });
  });

  // A condition that holds for a fraction of a bar covers no bar at all. Dropping
  // it would silently hide every brief satisfaction, which is the one thing the
  // band exists to make visible.
  it('collapses a sub-bar span onto its nearest bar', () => {
    expect(snapSpanToBars(bars, 121, 123)).toEqual({ from: 120, to: 120 });
    expect(snapSpanToBars(bars, 127, 129)).toEqual({ from: 130, to: 130 });
  });

  it('clamps a span that runs off either end', () => {
    expect(snapSpanToBars(bars, 0, 115)).toEqual({ from: 100, to: 110 });
    expect(snapSpanToBars(bars, 135, 9_999)).toEqual({ from: 140, to: 140 });
    expect(snapSpanToBars(bars, 0, 9_999)).toEqual({ from: 100, to: 140 });
  });

  it('collapses a span entirely outside the bars onto the nearest end', () => {
    expect(snapSpanToBars(bars, 10, 20)).toEqual({ from: 100, to: 100 });
    expect(snapSpanToBars(bars, 500, 600)).toEqual({ from: 140, to: 140 });
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
