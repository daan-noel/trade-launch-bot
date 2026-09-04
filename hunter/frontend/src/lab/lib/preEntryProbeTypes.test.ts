import { describe, expect, it } from 'vitest';

import {
  formatLag,
  lagSortValue,
  probeSummary,
  type PreEntryVerdict,
} from './preEntryProbeTypes';

function verdict(p: Partial<PreEntryVerdict> = {}): PreEntryVerdict {
  return {
    mint_address: 'M',
    state: 'matched',
    hits: 1,
    sol: 1,
    nearest_lag_slots: 3,
    nearest_lag_tx: 0,
    control_hits: 0,
    control_sol: 0,
    control_matched: false,
    ...p,
  };
}

describe('formatLag', () => {
  it('spells a same-slot match as such, never as "0 slots"', () => {
    // The whole point of the column: a print in the trader's own slot did not
    // lead his entry, and at a seat that reads the tape a slot late it could not
    // have triggered him. "0 slots" reads like a clean hit.
    expect(formatLag(verdict({ nearest_lag_slots: 0, nearest_lag_tx: 12 }))).toBe(
      'same slot −12 tx',
    );
    expect(formatLag(verdict({ nearest_lag_slots: 0, nearest_lag_tx: null }))).toBe('same slot');
  });

  it('reads slots away from the entry, singular at one', () => {
    expect(formatLag(verdict({ nearest_lag_slots: 1 }))).toBe('1 slot');
    expect(formatLag(verdict({ nearest_lag_slots: 4 }))).toBe('4 slots');
  });

  it('has nothing to say when nothing matched', () => {
    expect(formatLag(verdict({ nearest_lag_slots: null, nearest_lag_tx: null }))).toBe('-');
  });
});

describe('lagSortValue', () => {
  it('orders same-slot rows among themselves and ahead of a slot away', () => {
    const near = lagSortValue(verdict({ nearest_lag_slots: 0, nearest_lag_tx: 3 }))!;
    const far = lagSortValue(verdict({ nearest_lag_slots: 0, nearest_lag_tx: 40 }))!;
    const nextSlot = lagSortValue(verdict({ nearest_lag_slots: 1, nearest_lag_tx: 0 }))!;
    expect(near).toBeLessThan(far);
    expect(far).toBeLessThan(nextSlot);
  });

  it('is null for a row with no match, so it sorts as absent rather than as zero lag', () => {
    expect(lagSortValue(verdict({ nearest_lag_slots: null }))).toBeNull();
    expect(lagSortValue(undefined)).toBeNull();
  });
});

describe('probeSummary', () => {
  it('carries the control count beside the match count', () => {
    // A presence count with no denominator is not evidence: a structure a crowd
    // shares sits before everything, and only the control number says so.
    const rows = [
      verdict({ mint_address: 'A', nearest_lag_slots: 2, control_matched: true }),
      verdict({ mint_address: 'B', state: 'no-match', nearest_lag_slots: null }),
      verdict({ mint_address: 'C', state: 'unknown', unknown_reason: 'no-entry', nearest_lag_slots: null }),
    ];
    const s = probeSummary(rows, 0);
    expect(s).toContain('matched 1/3');
    expect(s).toContain('control 1/3');
    expect(s).toContain('1 unknown');
  });

  it('says when the backend could not reach every row', () => {
    expect(probeSummary([verdict()], 12)).toContain('12 not probed');
  });

  it('reports the median lag over the matched rows only', () => {
    const rows = [
      verdict({ nearest_lag_slots: 1, nearest_lag_tx: 0 }),
      verdict({ nearest_lag_slots: 5, nearest_lag_tx: 0 }),
      verdict({ nearest_lag_slots: 9, nearest_lag_tx: 0 }),
      // An absent row must not drag the median toward zero.
      verdict({ state: 'no-match', nearest_lag_slots: null }),
    ];
    expect(probeSummary(rows, 0)).toContain('median lag 5.0 slots');
  });

  it('says nothing rather than 0/0 when no row was probed', () => {
    expect(probeSummary([], 0)).toBe('no rows probed');
  });
});
