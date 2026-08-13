import { describe, expect, it } from 'vitest';

import { nearestSeriesIndex, seriesIndexAsOf } from './metricPanes';

/**
 * A hovered candle asks "what had the rule seen by here". `nearestSeriesIndex` is
 * free to answer with a row from AFTER that instant — reporting trades that had not
 * happened — which on a launch, where a whole position lives inside one candle, is
 * the difference between seeing an exit's crossing and not.
 */
describe('seriesIndexAsOf', () => {
  // A launch second: rows land on trades, sub-second apart.
  const at = [0.108, 0.147, 0.159, 0.205, 0.261, 1.108];

  it('never returns a row later than the instant asked for', () => {
    for (const t of [0.1, 0.15, 0.2, 0.26, 0.3, 5]) {
      const i = seriesIndexAsOf(at, t);
      if (i != null) expect(at[i], `as-of ${t}`).toBeLessThanOrEqual(t);
    }
  });

  it('picks the last row at or before the instant', () => {
    expect(seriesIndexAsOf(at, 0.204)).toBe(2); // 0.159, not the 0.205 crossing
    expect(seriesIndexAsOf(at, 0.205)).toBe(3); // inclusive
    expect(seriesIndexAsOf(at, 0.26)).toBe(3);
    expect(seriesIndexAsOf(at, 99)).toBe(at.length - 1);
  });

  it('returns null left of the recorded span instead of clamping to row 0', () => {
    // Clamping is what let an unresolvable hover render row 0's values as though
    // they were the crosshair's.
    expect(seriesIndexAsOf(at, 0.0)).toBeNull();
    expect(seriesIndexAsOf([], 1)).toBeNull();
  });

  it('differs from nearest exactly where it matters', () => {
    // 0.19 is closer to the 0.205 row, but 0.205 had not happened yet.
    expect(nearestSeriesIndex(at, 0.19)).toBe(3);
    expect(seriesIndexAsOf(at, 0.19)).toBe(2);
  });
});
