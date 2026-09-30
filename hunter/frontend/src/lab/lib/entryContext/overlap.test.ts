import { describe, expect, it } from 'vitest';
import { type HisToken, overlap } from './overlap';

const his = (cls: HisToken['cls']): HisToken => ({ cls, buys: 1, atPoint: cls === 'point' ? 1 : 0 });

describe('overlap', () => {
  it('puts each token in one area: its market zone by his class', () => {
    const o = overlap(
      new Map([
        ['A', his('point')],
        ['B', his('signal')],
        ['C', his('bought')],
        ['D', his('bought')],
      ]),
      ['A', 'B', 'C', 'X', 'Y'],
      ['A', 'C', 'X'],
    );
    expect(o['pass:point']).toEqual(['A']);
    expect(o['pass:bought']).toEqual(['C']);
    expect(o['pass:none']).toEqual(['X']);
    expect(o['pool:signal']).toEqual(['B']);
    expect(o['pool:none']).toEqual(['Y']);
    expect(o['outside:bought']).toEqual(['D']);
    expect(Object.values(o).flat().sort()).toEqual(['A', 'B', 'C', 'D', 'X', 'Y']);
  });
});
