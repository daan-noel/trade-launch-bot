import { describe, expect, it } from 'vitest';
import {
  anyFlowLineVisible,
  flowLineVisibilityFromPrefs,
  flowLineVisibilityKey,
} from './flowLineVisibility';

describe('flowLineVisibilityFromPrefs', () => {
  it('seeds both curves from a legacy single showFlowLines flag', () => {
    expect(flowLineVisibilityFromPrefs({ showFlowLines: false })).toEqual({
      tagged: false,
      untagged: false,
    });
    expect(flowLineVisibilityFromPrefs({ showFlowLines: true })).toEqual({
      tagged: true,
      untagged: true,
    });
  });

  it('prefers the split keys over the legacy flag', () => {
    expect(
      flowLineVisibilityFromPrefs({ showFlowLines: false, showFlowTagged: true, showFlowUntagged: false }),
    ).toEqual({ tagged: true, untagged: false });
  });

  it('defaults both on for a blob written before either key existed', () => {
    expect(flowLineVisibilityFromPrefs({})).toEqual({ tagged: true, untagged: true });
  });

  it('fills only the missing half from the legacy flag', () => {
    expect(flowLineVisibilityFromPrefs({ showFlowLines: false, showFlowUntagged: true })).toEqual({
      tagged: false,
      untagged: true,
    });
  });
});

describe('anyFlowLineVisible', () => {
  it('is the left price scale visibility — false only when both curves are off', () => {
    expect(anyFlowLineVisible({ tagged: false, untagged: false })).toBe(false);
    expect(anyFlowLineVisible({ tagged: true, untagged: false })).toBe(true);
    expect(anyFlowLineVisible({ tagged: false, untagged: true })).toBe(true);
  });
});

describe('flowLineVisibilityKey', () => {
  it('distinguishes which curve is hidden — the shared axis rescales either way', () => {
    const keys = new Set([
      flowLineVisibilityKey({ tagged: true, untagged: false }),
      flowLineVisibilityKey({ tagged: false, untagged: true }),
      flowLineVisibilityKey({ tagged: true, untagged: true }),
      flowLineVisibilityKey({ tagged: false, untagged: false }),
    ]);
    expect(keys.size).toBe(4);
  });
});
