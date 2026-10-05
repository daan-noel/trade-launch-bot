import { describe, expect, it } from 'vitest';
import {
  activeQuickFilterCount,
  clearedQuickFilters,
  defaultQuickFilters,
  quickFiltersEmpty,
  quickFiltersToSpecs,
  rollingOneDayFromUtc,
} from './tokensQuickFilters';

describe('tokensQuickFilters', () => {
  it('counts the default 1-day window as a reduction', () => {
    expect(activeQuickFilterCount(defaultQuickFilters())).toBe(1);
    expect(activeQuickFilterCount(clearedQuickFilters())).toBe(0);
    expect(
      activeQuickFilterCount({
        created_preset: 'custom',
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
        created_preset: 'custom',
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
    expect(quickFiltersEmpty(defaultQuickFilters())).toBe(false);
    expect(quickFiltersEmpty(clearedQuickFilters())).toBe(true);
  });

  it('defaults created to a rolling lower bound about one day back', () => {
    const now = new Date('2026-08-01T15:40:00Z');
    expect(rollingOneDayFromUtc(now)).toBe(
      new Date(now.getTime() - 24 * 3_600_000).toISOString().slice(0, 13) + ':00:00',
    );
    const specs = quickFiltersToSpecs(defaultQuickFilters(), 'UTC');
    expect(specs.created?.op).toBe('gte');
    if (specs.created?.op !== 'gte') return;
    const ageH = (Date.now() - Date.parse(`${specs.created.val}Z`)) / 3_600_000;
    expect(ageH).toBeGreaterThanOrEqual(24);
    expect(ageH).toBeLessThan(25);
  });
});
