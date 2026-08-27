import { describe, expect, it } from 'vitest';

import {
  AXES,
  axisDef,
  compareBounds,
  criteriaProblems,
  exactPredicate,
  formatBound,
  formatPredicate,
  isAxisId,
  isSatisfiable,
  lamportsToSolLabel,
  parseBound,
  predicateMatches,
  solLabelToLamports,
  type Criteria,
} from './fingerprintAxes';

const CEILING = '18446744073709551615';

describe('the registry', () => {
  /**
   * **The lock.** This table is a mirror of the Rust `AXES`, and a mirror that
   * drifts is worse than no mirror: an axis the backend matches on but the UI does
   * not render is invisible, and one the UI offers but the backend rejects fails on
   * save. Reading the Rust file directly means adding an axis in one place fails
   * here until it is added in the other.
   */
  it('carries the same axes, in the same order, as the Rust registry', () => {
    // Raw source of the Rust registry, via Vite's glob (vitest runs through Vite).
    // Keeps the guard dependency-free — no `node:fs`, no `@types/node` — the same
    // way `DataTable.boundary.test.ts` reads its siblings.
    const sources = (
      import.meta as unknown as {
        glob(
          pattern: string,
          opts: { eager: true; query: string; import: string },
        ): Record<string, string>;
      }
    ).glob('../../../../../engine/src/fingerprint/axis.rs', {
      eager: true,
      query: '?raw',
      import: 'default',
    });
    const rust = Object.values(sources)[0];
    expect(rust, 'the Rust registry must be readable — this guard is the lock').toBeTruthy();
    const keys = [...rust.matchAll(/^\s*key: "([a-z_]+)",$/gm)].map((m) => m[1]);
    expect(keys.length).toBeGreaterThan(0);
    expect(AXES.map((a) => a.id)).toEqual(keys);

    const chips = [...rust.matchAll(/^\s*chip: "([a-z_]+)",$/gm)].map((m) => m[1]);
    expect(AXES.map((a) => a.chip)).toEqual(chips);

    // Deferral drives whether a rule can fire at birth, so a mismatch here shows a
    // gate as available when it cannot be read yet.
    const phases = [...rust.matchAll(/^\s*phase: AxisPhase::(\w+),$/gm)].map((m) =>
      m[1] === 'FirstSlot' ? 'first_slot' : 'instant',
    );
    expect(AXES.map((a) => a.phase)).toEqual(phases);
  });

  it('keys every axis uniquely and explains every one', () => {
    expect(new Set(AXES.map((a) => a.id)).size).toBe(AXES.length);
    expect(new Set(AXES.map((a) => a.chip)).size).toBe(AXES.length);
    for (const def of AXES) {
      expect(def.definition, def.id).not.toBe('');
      expect(axisDef(def.id)).toBe(def);
      expect(isAxisId(def.id)).toBe(true);
    }
    expect(isAxisId('not_an_axis')).toBe(false);
  });
});

describe('bounds are integers, carried as strings', () => {
  // `Number()` rounds past 2^53, so two distinct amounts up there compare equal —
  // and `max_sol_cost = u64::MAX` is a real launch setting, not a hypothetical.
  it('compares a ceiling exactly where Number cannot', () => {
    const below = '18446744073709551614';
    expect(Number(CEILING) === Number(below)).toBe(true);
    expect(compareBounds(CEILING, below)).toBe(1);
    expect(compareBounds(below, CEILING)).toBe(-1);
    expect(compareBounds(CEILING, CEILING)).toBe(0);
  });

  it('ignores leading zeros and orders by magnitude, not lexically', () => {
    expect(compareBounds('007', '7')).toBe(0);
    expect(compareBounds('9', '10')).toBe(-1);
  });

  it('round-trips SOL through lamports, including a ceiling', () => {
    for (const sol of ['0', '0.000000001', '0.108', '1.515', '15.15', '1234.5']) {
      expect(lamportsToSolLabel(solLabelToLamports(sol)!)).toBe(sol);
    }
    expect(lamportsToSolLabel(CEILING)).toBe('18446744073.709551615');
    expect(solLabelToLamports('18446744073.709551615')).toBe(CEILING);
  });

  // A dropped bound reads as "unbounded", which WIDENS the match — the silent
  // direction — so junk has to be reported, never coerced.
  it('refuses junk rather than coercing it', () => {
    for (const bad of ['', ' ', 'abc', '-1', '1.2.3', 'NaN', 'Infinity']) {
      expect(parseBound(bad, 'lamports'), bad).toBeNull();
    }
    // A count axis takes an integer, not SOL.
    expect(parseBound('5', 'count')).toBe('5');
    expect(parseBound('0.5', 'count')).toBeNull();
  });

  it('formats a bound in its own display unit', () => {
    expect(formatBound('1515000000', 'lamports')).toBe('1.515');
    expect(formatBound('5', 'count')).toBe('5');
    expect(formatBound('200000', 'compute_units')).toBe('200000');
  });
});

describe('predicates', () => {
  it('treats equal bounds as exact and an open bound as one-sided', () => {
    const exact = exactPredicate('1515000000');
    expect(predicateMatches(exact, '1515000000')).toBe(true);
    expect(predicateMatches(exact, '1515000001')).toBe(false);

    const window = { kind: 'range' as const, min: '10', max: '20' };
    expect(predicateMatches(window, '10')).toBe(true);
    expect(predicateMatches(window, '20')).toBe(true);
    expect(predicateMatches(window, '9')).toBe(false);
    expect(predicateMatches(window, '21')).toBe(false);

    expect(predicateMatches({ kind: 'range', min: '10' }, CEILING)).toBe(true);
    expect(predicateMatches({ kind: 'range', max: '10' }, '0')).toBe(true);
  });

  it('names an unsatisfiable predicate rather than storing it', () => {
    expect(isSatisfiable({ kind: 'range', min: '20', max: '10' })).toBe(false);
    expect(isSatisfiable({ kind: 'sequence', labels: [] })).toBe(false);
    expect(isSatisfiable({ kind: 'range' })).toBe(true);
  });

  it('renders each shape readably in the axis unit', () => {
    expect(formatPredicate('max_cost_lamports', exactPredicate('1515000000'))).toBe('1.515');
    expect(
      formatPredicate('max_cost_lamports', { kind: 'range', min: '1000000000', max: '2000000000' }),
    ).toBe('1–2');
    expect(formatPredicate('ix_count', { kind: 'range', min: '3' })).toBe('≥3');
    expect(formatPredicate('ix_count', { kind: 'range', max: '3' })).toBe('≤3');
    expect(formatPredicate('ix_labels', { kind: 'sequence', labels: ['A', 'B'] })).toBe('A | B');
  });
});

describe('criteriaProblems', () => {
  it('accepts a usable map', () => {
    const ok: Criteria = {
      cu_limit: exactPredicate('200000'),
      ix_labels: { kind: 'sequence', labels: ['A'] },
    };
    expect(criteriaProblems(ok)).toEqual([]);
  });

  it('reports an unsatisfiable window and a predicate on the wrong kind of axis', () => {
    expect(criteriaProblems({ cu_limit: { kind: 'range', min: '9', max: '1' } })).toHaveLength(1);
    expect(criteriaProblems({ ix_labels: { kind: 'sequence', labels: [] } })).toHaveLength(1);
    expect(
      criteriaProblems({ cu_limit: { kind: 'sequence', labels: ['A'] } }),
    ).toHaveLength(1);
  });

  // The two label axes describe the same transaction, so a row setting both must
  // agree with itself — otherwise it reads as fully configured and matches nothing.
  it('rejects an ix_count that excludes its own ix_labels length', () => {
    const labels = { kind: 'sequence' as const, labels: ['A', 'B', 'C'] };
    expect(criteriaProblems({ ix_labels: labels, ix_count: { kind: 'range', min: '2', max: '4' } })).toEqual(
      [],
    );
    expect(criteriaProblems({ ix_labels: labels, ix_count: exactPredicate('5') })).toHaveLength(1);
  });
});
