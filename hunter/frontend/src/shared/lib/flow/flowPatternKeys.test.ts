import { describe, expect, it } from 'vitest';
import { flowPatternKeysFromTags, flowPatternKeysOf, rowsFromKeys } from './flowPatternKeys';

describe('flowPatternKeysOf', () => {
  it('returns null for empty / missing rows', () => {
    expect(flowPatternKeysOf(null)).toBeNull();
    expect(flowPatternKeysOf(undefined)).toBeNull();
    expect(flowPatternKeysOf([])).toBeNull();
    expect(flowPatternKeysOf([{ labels: [] }])).toBeNull();
  });

  it('keys an unpinned ix structure as its bare labels', () => {
    const keys = flowPatternKeysOf([{ labels: ['buy', 'sell'] }, { labels: ['a'] }]);
    expect(keys).toEqual(new Set([JSON.stringify(['buy', 'sell']), JSON.stringify(['a'])]));
  });
});

describe('flowPatternKeysFromTags', () => {
  it('keeps fee pins and ix core rows on the key', () => {
    const keys = flowPatternKeysFromTags({
      dump: { match: { ix_shape: [['s']] } },
      volume: {
        match: {
          ix_shape: [['x', 'y'], { labels: ['z'], cu_price: 5 }, { labels: ['Pump.Fun: Buy'], level: 'core', side: 'buy' }],
        },
      },
    });
    expect(rowsFromKeys(keys)).toEqual([
      { labels: ['x', 'y'] },
      { labels: ['z'], cu_price: 5 },
      { labels: ['Pump.Fun: Buy'], level: 'core', side: 'buy' },
    ]);
  });

  it('reads a named tag, and null when it has no rows', () => {
    const doc = { dump: { match: { ix_shape: [['s']] } }, volume: { match: { program: ['p'] } } };
    expect(flowPatternKeysFromTags(doc, 'dump')?.has(JSON.stringify(['s']))).toBe(true);
    expect(flowPatternKeysFromTags(doc)).toBeNull();
    expect(flowPatternKeysFromTags(null)).toBeNull();
  });
});

describe('rowsFromKeys', () => {
  it('drops keys that are not stored rows', () => {
    expect(rowsFromKeys(null)).toEqual([]);
    expect(rowsFromKeys(new Set(['not json', '{"a":1}', '[1,2]', 'program|CU', JSON.stringify(['A'])]))).toEqual([
      { labels: ['A'] },
    ]);
  });
});
