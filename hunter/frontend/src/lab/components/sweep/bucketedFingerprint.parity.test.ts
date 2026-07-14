import { describe, expect, it } from 'vitest';
import { BUCKETED_FINGERPRINT_COLUMNS } from 'lib/params';
import { BUCKETED_GROUP_FIELDS, GROUP_FIELD_TO_COLUMN } from './groupedTypes';

/**
 * The set of SOL fingerprint fields matched by `[lo, hi)` bucket (not exact) is
 * one fact expressed in two vocabularies: the rule form's column-keyed
 * `BUCKETED_FINGERPRINT_COLUMNS` (`@shared`) and the grouped sweep's
 * GroupField-keyed `BUCKETED_GROUP_FIELDS` (`@lab`). If they drift, the rule
 * form's "bucket" chip and the sweep's range chips disagree about which inputs
 * are bucketed. This guard translates the lab set through `GROUP_FIELD_TO_COLUMN`
 * and asserts equality — pure, no-DB, runs on every `npm test`.
 */
describe('bucketed fingerprint field parity (rule form ↔ grouped sweep)', () => {
  it('translates the grouped-sweep bucketed set to the rule-form column set', () => {
    const fromLab = new Set<string>();
    for (const field of BUCKETED_GROUP_FIELDS) {
      const col = GROUP_FIELD_TO_COLUMN[field];
      // Every bucketed group field must pin a rule fingerprint column.
      expect(col, `${field} has no rule column`).toBeDefined();
      if (col) fromLab.add(col);
    }
    expect([...fromLab].sort()).toEqual([...BUCKETED_FINGERPRINT_COLUMNS].sort());
  });
});
