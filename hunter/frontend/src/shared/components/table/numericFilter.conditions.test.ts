import { describe, expect, it } from 'vitest';

import {
  parseConditionList,
  formatConditionList,
  conditionListPredicate,
  type ComparisonExpr,
} from './numericFilter';

/**
 * The compound `,` AND / `|` OR grammar (plan §2) is the shared SSOT the strategy
 * rule inputs and DataTable client filtering both use. Strict: a malformed
 * fragment fails the whole parse (no `contains` fallback). Pure — runs on every
 * `npm test`.
 */
describe('parseConditionList', () => {
  it('parses a comma-AND list as a single OR arm', () => {
    expect(parseConditionList('>10, <=30')).toEqual<ComparisonExpr>([
      [
        { op: '>', value: 10 },
        { op: '<=', value: 30 },
      ],
    ]);
  });

  it('parses pipe-OR of AND arms', () => {
    expect(parseConditionList('<30 | >=70')).toEqual<ComparisonExpr>([
      [{ op: '<', value: 30 }],
      [{ op: '>=', value: 70 }],
    ]);
    expect(parseConditionList('>10, <=30 | >=70')).toEqual<ComparisonExpr>([
      [
        { op: '>', value: 10 },
        { op: '<=', value: 30 },
      ],
      [{ op: '>=', value: 70 }],
    ]);
  });

  it('expands a range to >= .. <= inside one arm', () => {
    expect(parseConditionList('1..10')).toEqual<ComparisonExpr>([
      [
        { op: '>=', value: 1 },
        { op: '<=', value: 10 },
      ],
    ]);
    expect(parseConditionList('10..1')).toEqual<ComparisonExpr>([
      [
        { op: '>=', value: 1 },
        { op: '<=', value: 10 },
      ],
    ]);
  });

  it('normalizes == to = and keeps !=', () => {
    expect(parseConditionList('==5')).toEqual<ComparisonExpr>([[{ op: '=', value: 5 }]]);
    expect(parseConditionList('!=0')).toEqual<ComparisonExpr>([[{ op: '!=', value: 0 }]]);
  });

  it('treats empty/whitespace as unconstrained ([])', () => {
    expect(parseConditionList('')).toEqual([]);
    expect(parseConditionList('   ')).toEqual([]);
  });

  it('tolerates whitespace and negatives/decimals', () => {
    expect(parseConditionList('  >  -2.5 ,  <= 3 ')).toEqual<ComparisonExpr>([
      [
        { op: '>', value: -2.5 },
        { op: '<=', value: 3 },
      ],
    ]);
  });

  it('returns null on any malformed fragment (strict, no contains fallback)', () => {
    expect(parseConditionList('10')).toBeNull(); // bare number needs an operator
    expect(parseConditionList('>abc')).toBeNull();
    expect(parseConditionList('>10,')).toBeNull(); // trailing empty fragment
    expect(parseConditionList('>10, , <5')).toBeNull();
    expect(parseConditionList('=>5')).toBeNull(); // not a real operator
    expect(parseConditionList('|')).toBeNull();
    expect(parseConditionList('>10 |')).toBeNull();
    expect(parseConditionList('| >=70')).toBeNull();
    expect(parseConditionList('<30 || >=70')).toBeNull();
  });
});

describe('formatConditionList round-trip', () => {
  it('formats AND and OR canonically', () => {
    expect(
      formatConditionList([
        [
          { op: '>', value: 10 },
          { op: '<=', value: 30 },
        ],
      ]),
    ).toBe('> 10, <= 30');
    expect(
      formatConditionList([
        [{ op: '<', value: 30 }],
        [{ op: '>=', value: 70 }],
      ]),
    ).toBe('< 30 | >= 70');
  });

  it('re-parses to the same arms', () => {
    const list = parseConditionList('>10, <=30 | !=20')!;
    expect(parseConditionList(formatConditionList(list))).toEqual(list);
  });
});

describe('conditionListPredicate', () => {
  it('ANDs within an arm', () => {
    const pred = conditionListPredicate(parseConditionList('>10, <=30')!);
    expect(pred(20)).toBe(true);
    expect(pred(10)).toBe(false);
    expect(pred(30)).toBe(true);
    expect(pred(31)).toBe(false);
  });

  it('ORs across arms', () => {
    const pred = conditionListPredicate(parseConditionList('<30 | >=70')!);
    expect(pred(20)).toBe(true);
    expect(pred(50)).toBe(false);
    expect(pred(70)).toBe(true);
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

  it('empty arms are vacuously true', () => {
    expect(conditionListPredicate([])(123)).toBe(true);
  });
});
