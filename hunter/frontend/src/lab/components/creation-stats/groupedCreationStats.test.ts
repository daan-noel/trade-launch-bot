import { describe, expect, it } from 'vitest';
import { withIxLabelsFilter } from './groupedCreationStats';

describe('withIxLabelsFilter', () => {
  it('attaches the applied filter when group_key omitted ix_labels', () => {
    expect(
      withIxLabelsFilter({ cu_limit: '300000' }, ['Pump.Fun: Create', 'Pump.Fun: Buy']),
    ).toEqual({
      cu_limit: '300000',
      ix_labels: 'Pump.Fun: Create | Pump.Fun: Buy',
    });
  });

  it('never overwrites an existing ix_labels key (grouped path)', () => {
    expect(
      withIxLabelsFilter(
        { cu_limit: '300000', ix_labels: 'A | B' },
        ['Pump.Fun: Create', 'Pump.Fun: Buy'],
      ),
    ).toEqual({ cu_limit: '300000', ix_labels: 'A | B' });
  });

  it('is a no-op when no filter is applied', () => {
    expect(withIxLabelsFilter({ cu_limit: '300000' }, null)).toEqual({ cu_limit: '300000' });
    expect(withIxLabelsFilter({ cu_limit: '300000' }, [])).toEqual({ cu_limit: '300000' });
    expect(withIxLabelsFilter({ cu_limit: '300000' }, undefined)).toEqual({
      cu_limit: '300000',
    });
  });
});
