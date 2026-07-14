import { describe, expect, it } from 'vitest';
import { swing1AxesShapeWarning } from './swing1Axes';

/**
 * `swing1AxesShapeWarning` mirrors the backend `swing1_shape_sane` guard (and the
 * lab sweep's combo prune): it flags a grid whose kill/volume depth or duration
 * bands overlap, so the user sees up front that part of the grid will be dropped.
 * Pure, no-DB — runs on every `npm test`.
 */
describe('swing1AxesShapeWarning', () => {
  it('passes when every kill_depth_min ≥ every vol_depth_max', () => {
    expect(
      swing1AxesShapeWarning({
        kill_depth_min_pct: [0.6],
        vol_depth_max_pct: [0.4],
      }),
    ).toBeNull();
    // Equality is allowed (bands touch but don't overlap).
    expect(
      swing1AxesShapeWarning({
        kill_depth_min_pct: [0.5],
        vol_depth_max_pct: [0.5],
      }),
    ).toBeNull();
  });

  it('warns when some (kill, vol) depth pair overlaps (min kill < max vol)', () => {
    // The default-grid case the user hit: kill ∈ {0.4,0.5,0.6}, vol ∈ {0.4,0.6}
    // → (0.4, 0.6) is invalid.
    const w = swing1AxesShapeWarning({
      kill_depth_min_pct: [0.4, 0.5, 0.6],
      vol_depth_max_pct: [0.4, 0.6],
    });
    expect(w).toContain('Kill depth min < Vol depth max');
  });

  it('warns when a volume low can be shorter than the kill cap', () => {
    const w = swing1AxesShapeWarning({
      kill_max_duration_ms: [8000, 10000],
      vol_min_duration_ms: [5000],
    });
    expect(w).toContain('Vol min duration < Kill max duration');
  });

  it('ignores null / 0 candidates (an unbounded axis never trips)', () => {
    // vol_depth_max is "off" (null) → no volume ceiling → nothing to overlap.
    expect(
      swing1AxesShapeWarning({
        kill_depth_min_pct: [0.4],
        vol_depth_max_pct: [null],
      }),
    ).toBeNull();
    // A 0 duration is the "no bound" sentinel, same as absent.
    expect(
      swing1AxesShapeWarning({
        kill_max_duration_ms: [8000],
        vol_min_duration_ms: [0],
      }),
    ).toBeNull();
  });
});
