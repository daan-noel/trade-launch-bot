import { describe, expect, it } from 'vitest';

import {
  notRangePredicate,
  predicateFromSpans,
  exactPredicate,
  type AxisPredicate,
  type AxisUnit,
} from './fingerprintAxes';
import { formatAxisPredicate, parseAxisPredicate } from './fingerprintGrammar';

// Rust SSOT: `hunter_engine::fingerprint::grammar`. These cases mirror that
// module's own tests one for one — the two parsers have to agree character for
// character, or a condition typed in the form and the same condition typed into
// the dashboard filter box select different tokens.

const p = (text: string) => parseAxisPredicate(text, 'count');
const lam = (text: string) => parseAxisPredicate(text, 'lamports');

/** `u64::MAX`, the `max_sol_cost` "fill at any price" sentinel — real launch data
 *  above 2^53, which is why nothing here may touch `Number()`. */
const CEILING = '18446744073709551615';

describe('parseAxisPredicate', () => {
  it('reads exact as the degenerate range', () => {
    expect(p('5')).toEqual(exactPredicate('5'));
    expect(p('=5')).toEqual(exactPredicate('5'));
    expect(p('== 5')).toEqual(exactPredicate('5'));
    expect(p('5..5')).toEqual(exactPredicate('5'));
  });

  it('treats .. as inclusive and - as the half-open chip form', () => {
    expect(p('3..5')).toEqual({ kind: 'range', min: '3', max: '5' });
    // `[3, 5)` is `[3, 4]` on an integer axis.
    expect(p('3-5')).toEqual({ kind: 'range', min: '3', max: '4' });
    expect(p('3–5')).toEqual({ kind: 'range', min: '3', max: '4' });
    expect(p('5-3')).toBeNull();
  });

  it('lands a strict inequality on the next integer', () => {
    expect(p('>3')).toEqual({ kind: 'range', min: '4' });
    expect(p('<3')).toEqual({ kind: 'range', max: '2' });
    expect(p('>=3')).toEqual({ kind: 'range', min: '3' });
    expect(p('<=3')).toEqual({ kind: 'range', max: '3' });
    // Nothing is below zero, so `<0` selects nothing and is refused rather than
    // stored as a gate that can never fire.
    expect(p('<0')).toBeNull();
  });

  it('intersects AND atoms and unites OR arms', () => {
    expect(p('>=1, <=9')).toEqual({ kind: 'range', min: '1', max: '9' });
    expect(p('1..9')).toEqual(p('>=1, <=9'));
    expect(p('<=2, >=7')).toBeNull(); // disjoint AND selects nothing
    expect(p('<=2 | >=7')).toEqual({
      kind: 'spans',
      spans: [{ max: '2' }, { min: '7' }],
    });
  });

  // The whole reason spans are canonical: two spellings of one token set must
  // produce one stored value, or `find_or_create` forks the fingerprint.
  it('gives one token set exactly one stored spelling', () => {
    expect(p('!=3')).toEqual(p('<=2 | >=4'));
    expect(p('!=3')).toEqual(p('>=4 | <=2'));
    expect(p('1..3 | 4..6')).toEqual(p('1..6')); // adjacent spans are one window
    expect(p('1..5 | 3..9')).toEqual(p('1..9')); // overlapping too
    expect(p('5..1')).toBeNull(); // an inverted range selects nothing
  });

  it('drops the lower half of a complement at zero', () => {
    expect(p('!=0')).toEqual({ kind: 'range', min: '1' });
    expect(p('!=1..3')).toEqual({ kind: 'spans', spans: [{ max: '0' }, { min: '4' }] });
  });

  it('refuses an expression that configures nothing', () => {
    expect(p('<=2 | >=3')).toBeNull();
    expect(p('>=0')).toBeNull();
    expect(p('')).toBeNull();
  });

  it('fails the whole parse on a malformed fragment', () => {
    for (const bad of ['abc', '1..', '..1', '>', '>=x', '1,', '|1', '1||2', '-1', '1.5']) {
      expect(p(bad), bad).toBeNull();
    }
  });

  it('parses lamports as decimal text, never as a float', () => {
    expect(lam('1.5')).toEqual(exactPredicate('1500000000'));
    expect(lam('0.000000001')).toEqual(exactPredicate('1'));
    // The ceiling is 18446744073.709551615 SOL — a float round trip loses its low
    // digits and calls it a different amount.
    expect(lam('18446744073.709551615')).toEqual(exactPredicate(CEILING));
    expect(lam('>1.5')).toEqual({ kind: 'range', min: '1500000001' });
    expect(lam('1.5-1.6')).toEqual({ kind: 'range', min: '1500000000', max: '1599999999' });
  });
});

describe('formatAxisPredicate', () => {
  it('round-trips every shape through its own parser', () => {
    const cases: [AxisUnit, string][] = [
      ['count', '5'],
      ['count', '3..5'],
      ['count', '>=3'],
      ['count', '<=3'],
      ['count', '!=3'],
      ['count', '!=3..5'],
      ['count', '1..2 | 7..8'],
      ['lamports', '1.5'],
      ['lamports', '1.5..2'],
      ['lamports', '!=1.5'],
    ];
    for (const [unit, text] of cases) {
      const pred = parseAxisPredicate(text, unit);
      expect(pred, `${text} did not parse`).not.toBeNull();
      const rendered = formatAxisPredicate(pred as AxisPredicate, unit);
      expect(rendered, 'canonical text drifted').toBe(text);
      expect(parseAxisPredicate(rendered, unit)).toEqual(pred);
    }
  });

  it('names a gap for the hole it excludes', () => {
    expect(formatAxisPredicate(notRangePredicate('3', '3'), 'count')).toBe('!=3');
    expect(
      formatAxisPredicate(predicateFromSpans([{ min: '1', max: '2' }, { min: '7', max: '8' }]), 'count'),
    ).toBe('1..2 | 7..8');
  });
});
