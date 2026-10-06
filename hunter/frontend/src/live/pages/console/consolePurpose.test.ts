import { describe, expect, it } from 'vitest';
import {
  EMPTY_RULE_SCOPE_ID,
  ruleMatchesPurpose,
  rulePurposeScope,
  withPurposeRuleScope,
} from './consolePurpose';

const COPY = new Set(['copy-1']);

describe('ruleMatchesPurpose', () => {
  it('keeps a manual row on General', () => {
    expect(ruleMatchesPurpose('general', COPY, 'copy-1', 'manual')).toBe(true);
    expect(ruleMatchesPurpose('copy', COPY, 'copy-1', 'manual')).toBe(false);
  });

  it('splits bot rows by the copy-rule set', () => {
    expect(ruleMatchesPurpose('copy', COPY, 'copy-1')).toBe(true);
    expect(ruleMatchesPurpose('general', COPY, 'copy-1')).toBe(false);
    expect(ruleMatchesPurpose('general', COPY, 'metric-1')).toBe(true);
    expect(ruleMatchesPurpose('copy', COPY, 'metric-1')).toBe(false);
  });

  it('keeps a row with no rule on General', () => {
    expect(ruleMatchesPurpose('general', COPY, null)).toBe(true);
    expect(ruleMatchesPurpose('copy', COPY, null)).toBe(false);
  });
});

describe('withPurposeRuleScope', () => {
  it('excludes copy ids on General and keeps a null rule id via nin', () => {
    expect(withPurposeRuleScope({}, rulePurposeScope('general', COPY)).rule_id).toEqual({
      op: 'nin',
      val: ['copy-1'],
    });
  });

  it('adds no predicate when General has no copy rules', () => {
    expect(withPurposeRuleScope({ mode: { op: 'eq', val: 'real' } }, { kind: 'exclude', ids: [] })).toEqual({
      mode: { op: 'eq', val: 'real' },
    });
  });

  it('matches nothing when Copy has no rules', () => {
    expect(withPurposeRuleScope({}, { kind: 'include', ids: [] }).rule_id).toEqual({
      op: 'eq',
      val: EMPTY_RULE_SCOPE_ID,
    });
  });

  it('keeps a selected rule that belongs on the tab', () => {
    const filters = { rule_id: { op: 'eq' as const, val: 'copy-1' } };
    expect(withPurposeRuleScope(filters, { kind: 'include', ids: ['copy-1'] })).toEqual(filters);
  });

  it('replaces a selected rule from the other tab', () => {
    expect(
      withPurposeRuleScope(
        { rule_id: { op: 'eq', val: 'metric-1' } },
        { kind: 'include', ids: ['copy-1'] },
      ).rule_id,
    ).toEqual({ op: 'eq', val: EMPTY_RULE_SCOPE_ID });
  });
});
