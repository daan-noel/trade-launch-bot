import { describe, expect, it } from 'vitest';
import { flowPatternKeysFromTags, flowPatternKeysOf } from './flowPatternKeys';

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

describe('flowPatternKeysFromTags', () => {
  it('reads the volume tag ix_shape labels, pins dropped', () => {
    const keys = flowPatternKeysFromTags({
      dump: { match: { ix_shape: [['s']] } },
      volume: { match: { ix_shape: [['x', 'y'], { labels: ['z'], cu_price: 5 }] } },
    });
    expect(keys).toEqual(new Set([JSON.stringify(['x', 'y']), JSON.stringify(['z'])]));
  });

  it('reads a named tag, and null when it has no shapes', () => {
    const doc = { dump: { match: { ix_shape: [['s']] } }, volume: { match: { program: ['p'] } } };
    expect(flowPatternKeysFromTags(doc, 'dump')?.has(JSON.stringify(['s']))).toBe(true);
    expect(flowPatternKeysFromTags(doc)).toBeNull();
    expect(flowPatternKeysFromTags(null)).toBeNull();
  });
});
