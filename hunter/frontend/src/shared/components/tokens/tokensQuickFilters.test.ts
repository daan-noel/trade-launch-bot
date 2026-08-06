import { describe, expect, it } from 'vitest';
import {
  activeQuickFilterCount,
  defaultQuickFilters,
  quickFiltersEmpty,
  quickFiltersToSpecs,
} from './tokensQuickFilters';

describe('tokensQuickFilters', () => {
  it('counts only active groups', () => {
    expect(activeQuickFilterCount(defaultQuickFilters())).toBe(0);
    expect(
      activeQuickFilterCount({
        created_from: '2026-01-01T00:00',
        created_to: '',
        dead: 'yes',
        migrated: '',
      }),
    ).toBe(2);
  });

  it('serializes created + flags into FilterSpecs', () => {
    const specs = quickFiltersToSpecs(
      {
        created_from: '2026-01-01T00:00',
        created_to: '2026-01-02T00:00',
        dead: 'yes',
        migrated: 'no',
      },
      'UTC',
    );
    expect(specs.created).toEqual({
      op: 'between',
      min: '2026-01-01T00:00:00',
      max: '2026-01-02T00:00:00',
    });
    expect(specs.dead).toEqual({ op: 'eq', val: 'yes' });
    expect(specs.migrated).toEqual({ op: 'eq', val: 'no' });
    expect(quickFiltersEmpty(defaultQuickFilters())).toBe(true);
  });
});
