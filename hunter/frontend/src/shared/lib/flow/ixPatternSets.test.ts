import { describe, expect, it } from 'vitest';

import {
  parsePastedPatterns,
  patternGroups,
  patternKeysForGroups,
  toggleIxPattern,
  UNGROUPED,
  type IxPattern,
} from './ixPatternSets';

const p = (group: string | null, ...labels: string[]): IxPattern => ({ group, ix_labels: labels });

describe('parsePastedPatterns', () => {
  it('takes a derived study file: { patterns: [{ tool, ix_labels }] }', () => {
    const r = parsePastedPatterns(
      JSON.stringify({
        note: 'target structures',
        patterns: [
          { tool: 'Axiom Trade', n_buys: 393343, ix_labels: ['A', 'B'] },
          { tool: 'GMGN Bot', ix_labels: ['C'] },
        ],
      }),
    );
    expect(r.error).toBeNull();
    expect(r.patterns).toEqual([p('Axiom Trade', 'A', 'B'), p('GMGN Bot', 'C')]);
  });

  it('takes bare label arrays and one bare sequence', () => {
    expect(parsePastedPatterns('[["A","B"],["C"]]').patterns).toEqual([
      p(null, 'A', 'B'),
      p(null, 'C'),
    ]);
    expect(parsePastedPatterns('["A","B"]').patterns).toEqual([p(null, 'A', 'B')]);
  });

  it('takes one JSON array per line', () => {
    const r = parsePastedPatterns('["A","B"]\n["C"],\n');
    expect(r.error).toBeNull();
    expect(r.patterns).toEqual([p(null, 'A', 'B'), p(null, 'C')]);
  });

  it('counts duplicates and unusable entries instead of silently dropping them', () => {
    const r = parsePastedPatterns('[["A"],["A"],[],{"tool":"x"}]');
    expect(r.accepted).toBe(1);
    expect(r.duplicates).toBe(1);
    expect(r.skipped).toBe(2);
  });

  it('keeps label ORDER as identity — a reordered sequence is a second pattern', () => {
    expect(parsePastedPatterns('[["A","B"],["B","A"]]').accepted).toBe(2);
  });

  it('rejects the lossy "A > B" display form rather than staging patterns that match nothing', () => {
    const r = parsePastedPatterns('Create > Buy > Transfer');
    expect(r.patterns).toEqual([]);
    expect(r.error).toMatch(/Not JSON/);
  });
});

describe('group narrowing', () => {
  const set = [p('Axiom', 'A'), p('GMGN', 'B'), p(null, 'C')];

  it('lists groups in first-seen order with a bucket for ungrouped', () => {
    expect(patternGroups(set)).toEqual(['Axiom', 'GMGN', UNGROUPED]);
  });

  it('classifies with the enabled groups only; null ⇒ every group', () => {
    expect(patternKeysForGroups(set, null)?.size).toBe(3);
    const axiom = patternKeysForGroups(set, new Set(['Axiom']));
    expect([...(axiom ?? [])]).toEqual([JSON.stringify(['A'])]);
  });

  it('returns null when nothing is left to classify with', () => {
    expect(patternKeysForGroups([], null)).toBeNull();
    expect(patternKeysForGroups(set, new Set(['nope']))).toBeNull();
  });
});

describe('toggleIxPattern', () => {
  it('removes an existing sequence whatever group it is filed under', () => {
    expect(toggleIxPattern([p('Axiom', 'A'), p(null, 'B')], ['A'], 'other')).toEqual([p(null, 'B')]);
  });

  it('appends a new sequence under the active group', () => {
    expect(toggleIxPattern([p(null, 'A')], ['B'], 'Axiom')).toEqual([
      p(null, 'A'),
      p('Axiom', 'B'),
    ]);
  });
});
