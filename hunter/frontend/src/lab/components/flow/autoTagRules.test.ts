import { describe, expect, it } from 'vitest';
import { autoTagState, toggleAutoTag } from './autoTagRules';

describe('auto-tag toggles', () => {
  const marks = { instant_buy: ['a', 'b'], slot_bundle: ['b', 'c'], below_zero: [] };

  it('adds every wallet of a rule that is off or partly listed', () => {
    expect(toggleAutoTag(['x'], marks, 'instant_buy')).toEqual(['x', 'a', 'b']);
    expect(toggleAutoTag(['a'], marks, 'instant_buy')).toEqual(['a', 'b']);
  });

  it('takes a rule out but keeps the wallets another on rule marks', () => {
    const both = ['a', 'b', 'c'];
    expect(toggleAutoTag(both, marks, 'instant_buy')).toEqual(['b', 'c']);
    // Bundle partly listed is not on, so it holds nothing back.
    expect(toggleAutoTag(['a', 'b'], marks, 'instant_buy')).toEqual([]);
  });

  it('reads on, some and off', () => {
    expect(autoTagState(new Set(['a', 'b']), marks.instant_buy)).toBe('on');
    expect(autoTagState(new Set(['b']), marks.instant_buy)).toBe('some');
    expect(autoTagState(new Set(), marks.instant_buy)).toBe('off');
    expect(autoTagState(new Set(['a']), [])).toBe('off');
  });
});
