import { describe, expect, it } from 'vitest';

import { groupShortLabel, type GroupedCreationGroup } from './groupedCreationStats';

function group(group_key: Record<string, unknown>): GroupedCreationGroup {
  return { g: 0, group_key, total: 1, trades: 1, trades_avg: 1 };
}

// A group key carries PREDICATES, not rendered text — the fingerprint-scoped card
// builds its key straight off the saved fingerprint's criteria. A reader that took
// a value for a string threw on the first `ix_labels` axis it met.
describe('groupShortLabel', () => {
  it('names a fingerprint-scoped key of window + label predicates', () => {
    expect(
      groupShortLabel(
        group({
          max_cost_lamports: { kind: 'window', min: '1515000000', max: '1515000000' },
          ix_labels: { kind: 'labels', labels: ['Create', 'Buy', 'Sync'] },
        }),
      ),
    ).toBe('Max cost=1.515 · Instruction labels=3ix:Sync');
  });

  it('reads an absent label axis as an empty sequence, not a crash', () => {
    expect(groupShortLabel(group({ ix_labels: { kind: 'missing' } }))).toBe(
      'Instruction labels=0 ix',
    );
  });

  it('names an axis-free key as the whole corpus', () => {
    expect(groupShortLabel(group({}))).toBe('ALL tokens');
  });
});
