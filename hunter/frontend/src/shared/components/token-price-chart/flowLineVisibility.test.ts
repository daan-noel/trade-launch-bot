import { describe, expect, it } from 'vitest';
import {
  anyFlowLineVisible,
  flowLineVisibilityFromPrefs,
  flowLineVisibilityKey,
} from './flowLineVisibility';

describe('flowLineVisibilityFromPrefs', () => {
  it('seeds both curves from a legacy single showFlowLines flag', () => {
    expect(flowLineVisibilityFromPrefs({ showFlowLines: false })).toEqual({
      vol: false,
      nonVol: false,
    });
    expect(flowLineVisibilityFromPrefs({ showFlowLines: true })).toEqual({
      vol: true,
      nonVol: true,
    });
  });

  it('prefers the split keys over the legacy flag', () => {
    expect(
      flowLineVisibilityFromPrefs({ showFlowLines: false, showFlowVol: true, showFlowNonVol: false }),
    ).toEqual({ vol: true, nonVol: false });
  });

  it('defaults both on for a blob written before either key existed', () => {
    expect(flowLineVisibilityFromPrefs({})).toEqual({ vol: true, nonVol: true });
  });

  it('fills only the missing half from the legacy flag', () => {
    expect(flowLineVisibilityFromPrefs({ showFlowLines: false, showFlowNonVol: true })).toEqual({
      vol: false,
      nonVol: true,
    });
  });
});

describe('anyFlowLineVisible', () => {
  it('is the left price scale visibility — false only when both curves are off', () => {
    expect(anyFlowLineVisible({ vol: false, nonVol: false })).toBe(false);
    expect(anyFlowLineVisible({ vol: true, nonVol: false })).toBe(true);
    expect(anyFlowLineVisible({ vol: false, nonVol: true })).toBe(true);
  });
});

describe('flowLineVisibilityKey', () => {
  it('distinguishes which curve is hidden — the shared axis rescales either way', () => {
    const keys = new Set([
      flowLineVisibilityKey({ vol: true, nonVol: false }),
      flowLineVisibilityKey({ vol: false, nonVol: true }),
      flowLineVisibilityKey({ vol: true, nonVol: true }),
      flowLineVisibilityKey({ vol: false, nonVol: false }),
    ]);
    expect(keys.size).toBe(4);
  });
});
