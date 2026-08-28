import { describe, expect, it } from 'vitest';

import {
  AXES,
  axisDef,
  compareBounds,
  criteriaProblems,
  exactPredicate,
  notRangePredicate,
  predicateFromSpans,
  predicatesOverlap,
  spanSetFrom,
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
  /** Raw source of the Rust registry, via Vite's glob (vitest runs through Vite).
   *  Keeps the guard dependency-free - no `node:fs`, no `@types/node` - the same way
   *  `DataTable.boundary.test.ts` reads its siblings. */
  const rustSource = (): string => {
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
    return Object.values(sources)[0];
  };

  /** The `AXES` table alone - the file also names the same identifiers in tests. */
  const rustTable = (rust: string): string => {
    const from = rust.indexOf('pub static AXES');
    expect(from, 'the Rust registry declares AXES').toBeGreaterThan(-1);
    return rust.slice(from, rust.indexOf('\n];', from));
  };

  /** One field of every entry, in declaration order. */
  const fieldValues = (table: string, re: RegExp): string[] =>
    [...table.matchAll(re)].map((m) => m[1]);

  /** A Rust string literal, unwrapped: line continuations joined, whitespace
   *  collapsed, and the punctuation each language prefers normalised - what must
   *  agree is the SENTENCE, not how the two files wrap it. */
  const normalise = (v: string): string =>
    v
      .replace(/\\\s*\n\s*/g, '')
      .replace(/\\"/g, '"')
      .replace(/\\'/g, "'")
      .replace(/\s+/g, ' ')
      .replace(/[‘’]/g, "'")
      .replace(/[“”]/g, '"')
      .replace(/[—–]/g, '-')
      .trim();

  /**
   * **The lock.** This table is a mirror of the Rust `AXES`, and a mirror that
   * drifts is worse than no mirror: an axis the backend matches on but the UI does
   * not render is invisible, one the UI offers but the backend rejects fails on
   * save, and a DEFINITION that drifts is the silent one — `prior_launches` read
   * "how many tokens this creator launched before" here while the engine counted
   * them over a trailing 30-day window, so a threshold authored against the tooltip
   * gated on a different number.
   *
   * Every field is checked, not the identifiers alone. Reading the Rust file
   * directly means adding or editing an axis in one place fails here until the other
   * follows.
   */
  it('mirrors the Rust registry field for field, in order', () => {
    const rust = rustSource();
    expect(rust, 'the Rust registry must be readable — this guard is the lock').toBeTruthy();
    const table = rustTable(rust);

    const keys = fieldValues(table, /^\s*key: "([a-z_]+)",$/gm);
    expect(keys.length).toBeGreaterThan(0);
    expect(AXES.map((a) => a.id)).toEqual(keys);
    expect(AXES.map((a) => a.chip)).toEqual(fieldValues(table, /^\s*chip: "([a-z_]+)",$/gm));

    // The label names the axis in the form and in every summary chip.
    expect(AXES.map((a) => a.label)).toEqual(fieldValues(table, /^\s*label: "([^"]+)",$/gm));

    // Kind and unit decide how a bound is parsed and shown; a mismatch shows SOL for
    // a compute-unit count, or offers a numeric box for a label sequence.
    expect(AXES.map((a) => a.kind)).toEqual(
      fieldValues(table, /^\s*kind: AxisKind::(\w+),$/gm).map((k) => k.toLowerCase()),
    );
    expect(AXES.map((a) => a.unit)).toEqual(
      fieldValues(table, /^\s*unit: AxisUnit::(\w+),$/gm).map((u) =>
        u.replace(/(?<!^)([A-Z])/g, '_$1').toLowerCase(),
      ),
    );

    // Deferral drives whether a rule can fire at birth, so a mismatch here shows a
    // gate as available when it cannot be read yet.
    expect(AXES.map((a) => a.phase)).toEqual(
      fieldValues(table, /^\s*phase: AxisPhase::(\w+),$/gm).map((p) =>
        p === 'FirstSlot' ? 'first_slot' : 'instant',
      ),
    );

    // THE definition, which is what a rule author actually reads.
    const rustDefs = fieldValues(table, /definition: "((?:[^"\\]|\\[\s\S])*)"/g).map(normalise);
    expect(rustDefs).toHaveLength(AXES.length);
    expect(AXES.map((a) => normalise(a.definition))).toEqual(rustDefs);
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

describe('the span algebra', () => {
  // The invariant the whole design rests on: two spellings of one token set are ONE
  // stored value, so `find_or_create` cannot fork a second row for a fingerprint
  // that already exists. Mirrors the Rust
  // `two_spellings_of_one_token_set_store_identically`.
  it('stores two spellings of one token set identically', () => {
    expect(notRangePredicate('3', '3')).toEqual(predicateFromSpans([{ max: '2' }, { min: '4' }]));
    // Adjacent and overlapping windows are one window, never two spans.
    expect(predicateFromSpans([{ min: '1', max: '3' }, { min: '4', max: '6' }])).toEqual({
      kind: 'range',
      min: '1',
      max: '6',
    });
    expect(predicateFromSpans([{ min: '7', max: '8' }, { min: '1', max: '2' }])).toEqual({
      kind: 'spans',
      spans: [{ min: '1', max: '2' }, { min: '7', max: '8' }],
    });
  });

  it('compares bounds as decimal strings, so a ceiling stays its own amount', () => {
    const nearCeiling = '18446744073709551614';
    expect(spanSetFrom([{ min: CEILING, max: CEILING }, { min: nearCeiling, max: nearCeiling }])).toEqual([
      { min: nearCeiling, max: CEILING },
    ]);
  });

  it('matches and overlaps across every span of a gap predicate', () => {
    const gate = notRangePredicate('3', '5');
    expect(predicateMatches(gate, '2')).toBe(true);
    expect(predicateMatches(gate, '4')).toBe(false);
    expect(predicateMatches(gate, '6')).toBe(true);
    // A filter box asks "could this row match anything I typed" — containment for a
    // bare value, so this only widens what the box can ask.
    expect(predicatesOverlap(gate, exactPredicate('6'))).toBe(true);
    expect(predicatesOverlap(gate, exactPredicate('4'))).toBe(false);
    expect(predicatesOverlap(gate, { kind: 'range', min: '4', max: '9' })).toBe(true);
  });

  // A multi-span row is only ever written canonical, so a hand-written one that is
  // not must be REFUSED rather than normalised on read — normalising would let two
  // rows describe one token set. Mirrors the Rust
  // `a_non_canonical_span_list_is_refused_at_the_write_edge`.
  it('refuses a non-canonical span list at the write edge', () => {
    const bad = (spans: { min?: string; max?: string }[]) =>
      criteriaProblems({ ix_count: { kind: 'spans', spans } });
    expect(bad([{ min: '1', max: '2' }])).toHaveLength(1); // one span is a plain range
    expect(bad([])).toHaveLength(1); // no span matches nothing
    expect(bad([{ min: '4', max: '9' }, { min: '1', max: '2' }])).toHaveLength(1); // descending
    expect(bad([{ min: '1', max: '4' }, { min: '3', max: '9' }])).toHaveLength(1); // overlapping
    expect(bad([{ min: '1', max: '2' }, { min: '3', max: '9' }])).toHaveLength(1); // touching
    expect(bad([{ min: '1', max: '2' }, { min: '7', max: '8' }])).toHaveLength(0);
  });
});
