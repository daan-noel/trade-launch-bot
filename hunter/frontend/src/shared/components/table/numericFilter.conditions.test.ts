import { describe, expect, it } from 'vitest';

import {
  parseConditionList,
  formatConditionList,
  conditionListPredicate,
  type Comparison,
} from './numericFilter';

/**
 * The compound comma-AND grammar (plan §2) is the shared SSOT the strategy rule
 * inputs and DataTable client filtering both use. Strict: a malformed fragment
 * fails the whole parse (no `contains` fallback). Pure — runs on every `npm test`.
 */
describe('parseConditionList', () => {
  it('parses a comma-AND list preserving operator tokens', () => {
    expect(parseConditionList('>10, <=30')).toEqual<Comparison[]>([
      { op: '>', value: 10 },
      { op: '<=', value: 30 },
    ]);
  });

  it('expands a range to >= .. <=', () => {
    expect(parseConditionList('1..10')).toEqual<Comparison[]>([
      { op: '>=', value: 1 },
      { op: '<=', value: 10 },
    ]);
    // Reversed bounds normalize.
    expect(parseConditionList('10..1')).toEqual<Comparison[]>([
      { op: '>=', value: 1 },
      { op: '<=', value: 10 },
    ]);
  });

  it('normalizes == to = and keeps !=', () => {
    expect(parseConditionList('==5')).toEqual<Comparison[]>([{ op: '=', value: 5 }]);
    expect(parseConditionList('!=0')).toEqual<Comparison[]>([{ op: '!=', value: 0 }]);
  });

  it('treats empty/whitespace as unconstrained ([])', () => {
    expect(parseConditionList('')).toEqual([]);
    expect(parseConditionList('   ')).toEqual([]);
  });

  it('tolerates whitespace and negatives/decimals', () => {
    expect(parseConditionList('  >  -2.5 ,  <= 3 ')).toEqual<Comparison[]>([
      { op: '>', value: -2.5 },
      { op: '<=', value: 3 },
    ]);
  });

  it('returns null on any malformed fragment (strict, no contains fallback)', () => {
    expect(parseConditionList('10')).toBeNull(); // bare number needs an operator
    expect(parseConditionList('>abc')).toBeNull();
    expect(parseConditionList('>10,')).toBeNull(); // trailing empty fragment
    expect(parseConditionList('>10, , <5')).toBeNull();
    expect(parseConditionList('=>5')).toBeNull(); // not a real operator
  });
});

describe('formatConditionList round-trip', () => {
  it('formats to canonical "op value, op value"', () => {
    expect(formatConditionList([{ op: '>', value: 10 }, { op: '<=', value: 30 }])).toBe(
      '> 10, <= 30',
    );
  });

  it('re-parses to the same list', () => {
    const list = parseConditionList('>10, <=30, !=20')!;
    expect(parseConditionList(formatConditionList(list))).toEqual(list);
  });
});

describe('conditionListPredicate', () => {
  it('ANDs all comparisons', () => {
    const pred = conditionListPredicate(parseConditionList('>10, <=30')!);
    expect(pred(20)).toBe(true);
    expect(pred(10)).toBe(false);
    expect(pred(30)).toBe(true);
    expect(pred(31)).toBe(false);
  });

  it('handles = and != and ranges', () => {
    expect(conditionListPredicate(parseConditionList('=5')!)(5)).toBe(true);
    expect(conditionListPredicate(parseConditionList('!=5')!)(5)).toBe(false);
    const inRange = conditionListPredicate(parseConditionList('1..10')!);
    expect(inRange(0)).toBe(false);
    expect(inRange(1)).toBe(true);
    expect(inRange(10)).toBe(true);
    expect(inRange(11)).toBe(false);
  });
});
