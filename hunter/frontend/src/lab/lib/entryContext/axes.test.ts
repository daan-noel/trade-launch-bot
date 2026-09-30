import { describe, expect, it } from 'vitest';
import { AXIS_FAMILIES, ENTRY_AXES, groupsHiddenIn } from './axes';

describe('AXIS_FAMILIES', () => {
  it('puts every axis group but his buy in exactly one family', () => {
    for (const g of new Set(ENTRY_AXES.map((a) => a.group))) {
      const owners = AXIS_FAMILIES.filter((f) => f.groups.includes(g)).length;
      expect([g, owners]).toEqual([g, g === 'buy' ? 0 : 1]);
    }
  });

  it("hides only the other families' groups", () => {
    for (const f of AXIS_FAMILIES) {
      const hidden = groupsHiddenIn(f.key);
      expect(f.groups.some((g) => hidden.includes(g))).toBe(false);
      expect(hidden).not.toContain('buy');
    }
  });
});
