import { describe, expect, it } from 'vitest';

import { fromFingerprint, shapeProblems, toDraft } from './FingerprintForm';
import { fingerprintUpdateBody, type Fingerprint } from 'lib/strategy/types';
import type { Criteria } from 'lib/strategy/fingerprintAxes';

const range = (n: string) => ({ kind: 'range' as const, min: n, max: n });

const fp = (over: Partial<Fingerprint>): Fingerprint => ({
  id: 'x',
  name: 'x',
  wildcard: false,
  criteria: {},
  tags: {},
  created_at: '',
  updated_at: '',
  ...over,
});

const five: Criteria = { ix_count: range('5'), cu_limit: range('400000') };
const four: Criteria = { ix_count: range('4') };
const seven: Criteria = { ix_count: range('7') };

describe('FingerprintForm shape lists', () => {
  it('round-trips include and exclude shapes through the form', () => {
    const row = fp({ criteria: five, alt_criteria: [four], exclude_criteria: [seven] });
    const draft = toDraft(fromFingerprint(row));
    expect(draft.criteria).toEqual(five);
    expect(draft.alt_criteria).toEqual([four]);
    expect(draft.exclude_criteria).toEqual([seven]);
    expect(draft.wildcard).toBe(false);
  });

  it('a wildcard saves no include shape but keeps its exclusions', () => {
    const row = fp({ wildcard: true, exclude_criteria: [seven] });
    const state = fromFingerprint(row);
    expect(state.mode).toBe('all');
    const draft = toDraft(state);
    expect(draft.wildcard).toBe(true);
    expect(draft.criteria).toEqual({});
    expect(draft.alt_criteria).toEqual([]);
    expect(draft.exclude_criteria).toEqual([seven]);
    expect(shapeProblems(state)).toEqual([]);
  });

  it('a single-shape row saves with both lists empty', () => {
    const draft = toDraft(fromFingerprint(fp({ criteria: four })));
    expect(draft.alt_criteria).toEqual([]);
    expect(draft.exclude_criteria).toEqual([]);
  });

  it('names an empty shape instead of saving it', () => {
    const state = fromFingerprint(fp({ criteria: four }));
    state.exclude.push({ ...state.include[0], key: 99, conditions: {}, ix_labels: '', openAxes: [], ixOpen: false });
    const problems = shapeProblems(state);
    expect(problems.some((p) => p.startsWith('Exclude X1'))).toBe(true);
  });
});

describe('fingerprintUpdateBody', () => {
  it('carries every identity field so a tag save cannot widen the row', () => {
    const row = fp({ criteria: five, alt_criteria: [four], exclude_criteria: [seven], tags: { a: 1 } });
    const body = fingerprintUpdateBody(row, { tags: { b: 2 } });
    expect(body).toEqual({
      name: 'x',
      wildcard: false,
      criteria: five,
      alt_criteria: [four],
      exclude_criteria: [seven],
      tags: { b: 2 },
    });
  });

  it('sends explicit empty lists for a single-shape row', () => {
    const body = fingerprintUpdateBody(fp({ criteria: four }));
    expect(body.alt_criteria).toEqual([]);
    expect(body.exclude_criteria).toEqual([]);
  });
});
