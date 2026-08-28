import { describe, expect, it } from 'vitest';
import { flowPatternKeysFromMetricConfig, flowPatternKeysOf } from './flowPatternKeys';

describe('flowPatternKeysOf', () => {
  it('returns null for empty / missing patterns', () => {
    expect(flowPatternKeysOf(null)).toBeNull();
    expect(flowPatternKeysOf(undefined)).toBeNull();
    expect(flowPatternKeysOf([])).toBeNull();
    expect(flowPatternKeysOf([[]])).toBeNull();
  });

  it('builds JSON keys for non-empty patterns', () => {
    const keys = flowPatternKeysOf([['buy', 'sell'], ['a']]);
    expect(keys).not.toBeNull();
    expect(keys!.size).toBe(2);
    expect(keys!.has(JSON.stringify(['buy', 'sell']))).toBe(true);
    expect(keys!.has(JSON.stringify(['a']))).toBe(true);
  });
});

describe('flowPatternKeysFromMetricConfig', () => {
  it('reads m_flow_ix.ix_patterns', () => {
    const keys = flowPatternKeysFromMetricConfig({
      m_flow_ix: { ix_patterns: [['x', 'y']] },
    });
    expect(keys?.has(JSON.stringify(['x', 'y']))).toBe(true);
  });

  it('returns null when flow config is absent', () => {
    expect(flowPatternKeysFromMetricConfig({})).toBeNull();
    expect(flowPatternKeysFromMetricConfig(null)).toBeNull();
  });
});
