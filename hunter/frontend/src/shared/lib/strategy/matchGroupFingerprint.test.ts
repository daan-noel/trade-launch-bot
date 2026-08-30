import { describe, expect, it } from 'vitest';

import {
  fingerprintCompatibleWithGroupKey,
  fingerprintIdentityFromGroupKey,
  fingerprintIdentityKey,
  fingerprintMatchesIdentity,
  fingerprintToIdentity,
  findFingerprintForGroupKey,
  identityHasCriterion,
  indexFingerprintsByIdentity,
  matchFingerprintsForGroups,
  predicatesEqual,
  groupValueLabels,
  groupValueText,
  renderGroupKey,
  withIxLabelsFilter,
} from './matchGroupFingerprint';
import { exactPredicate, type Criteria } from './fingerprintAxes';
import type { Fingerprint } from './types';

const SOL = 1_000_000_000n;
const CEILING = '18446744073709551615';

function fp(id: string, criteria: Criteria, wildcard = false): Fingerprint {
  return {
    id,
    name: id,
    wildcard,
    criteria,
    metric_config: {},
    created_at: '',
    updated_at: '',
  };
}

function window(min?: string, max?: string) {
  return { kind: 'window', ...(min != null && { min }), ...(max != null && { max }) };
}

describe('fingerprintIdentityFromGroupKey', () => {
  // The headline property: a card's window IS the predicate a fingerprint stores,
  // so identity is a copy — no string round-trip for a second reader to disagree with.
  it('copies each key window into the matching axis predicate', () => {
    const id = fingerprintIdentityFromGroupKey({
      max_cost_lamports: window(String(SOL), String(2n * SOL - 1n)),
      cu_limit: window('200000', '200000'),
      ix_labels: { kind: 'labels', labels: ['A', 'B'] },
    });
    expect(id.criteria.max_cost_lamports).toEqual({
      kind: 'range',
      min: '1000000000',
      max: '1999999999',
    });
    expect(id.criteria.cu_limit).toEqual({ kind: 'range', min: '200000', max: '200000' });
    expect(id.criteria.ix_labels).toEqual({ kind: 'sequence', labels: ['A', 'B'] });
    // A group names axis VALUES, so it can never describe "every token".
    expect(id.wildcard).toBe(false);
  });

  it('skips values that name nothing a rule can match on', () => {
    const id = fingerprintIdentityFromGroupKey({
      max_cost_lamports: { kind: 'missing' },
      token_program_id: { kind: 'text', value: 'Tokenkeg' },
      is_cashback_enabled: { kind: 'flag', value: true },
      ix_labels: { kind: 'labels', labels: [] },
    });
    expect(id.criteria).toEqual({});
    expect(identityHasCriterion(id)).toBe(false);
  });

  // The value that could not be represented at all under the retired model: it is
  // past 2^53, so a `Number()` round-trip silently dropped its low digits.
  it('carries a u64::MAX ceiling through without losing a digit', () => {
    const id = fingerprintIdentityFromGroupKey({ max_cost_lamports: window(CEILING, CEILING) });
    expect(id.criteria.max_cost_lamports).toEqual({ kind: 'range', min: CEILING, max: CEILING });
    expect(identityHasCriterion(id)).toBe(true);
  });

  it('reads the two new axes like any other', () => {
    const id = fingerprintIdentityFromGroupKey({
      ix_count: window('3', '5'),
      prior_launches: window('0', '0'),
    });
    expect(id.criteria.ix_count).toEqual({ kind: 'range', min: '3', max: '5' });
    expect(id.criteria.prior_launches).toEqual({ kind: 'range', min: '0', max: '0' });
  });
});

describe('predicatesEqual', () => {
  it('compares bounds as decimal strings, never as numbers', () => {
    // Two amounts one lamport apart, both past 2^53: `Number()` calls them equal.
    const a = { kind: 'range' as const, min: CEILING, max: CEILING };
    const b = { kind: 'range' as const, min: '18446744073709551614', max: '18446744073709551614' };
    expect(Number(CEILING) === Number('18446744073709551614')).toBe(true);
    expect(predicatesEqual(a, b)).toBe(false);
    expect(predicatesEqual(a, { ...a })).toBe(true);
  });

  it('normalises leading zeros and treats an empty label list as unset', () => {
    expect(predicatesEqual(exactPredicate('007'), exactPredicate('7'))).toBe(true);
    expect(
      predicatesEqual({ kind: 'sequence', labels: [] }, { kind: 'sequence', labels: [] }),
    ).toBe(true);
    expect(
      predicatesEqual({ kind: 'sequence', labels: [] }, { kind: 'sequence', labels: ['A'] }),
    ).toBe(false);
  });

  it('never equates an open bound with a closed one', () => {
    expect(predicatesEqual({ kind: 'range', min: '1' }, { kind: 'range', min: '1', max: '1' })).toBe(
      false,
    );
  });
});

describe('identity matching', () => {
  it('matches a fingerprint whose criteria equal the card`s', () => {
    const card = fingerprintIdentityFromGroupKey({ cu_limit: window('200000', '200000') });
    const a = fp('a', { cu_limit: exactPredicate('200000') });
    const b = fp('b', { cu_limit: exactPredicate('300000') });
    expect(fingerprintMatchesIdentity(a, card)).toBe(true);
    expect(fingerprintMatchesIdentity(b, card)).toBe(false);
  });

  // A wildcard drops every axis the card is made of, so badging with it would claim
  // the card's group is what the rule arms on while the rule arms on everything.
  it('never matches a wildcard to an axis-bearing card', () => {
    const card = fingerprintIdentityFromGroupKey({ cu_limit: window('1', '1') });
    expect(fingerprintMatchesIdentity(fp('w', {}, true), card)).toBe(false);
    expect(fingerprintCompatibleWithGroupKey(fp('w', {}, true), { cu_limit: window('1', '1') })).toBe(
      false,
    );
  });

  it('keys identity so equal fingerprints collide and different ones do not', () => {
    const a = fp('a', { cu_limit: exactPredicate('1') });
    const b = fp('b', { cu_limit: exactPredicate('1') });
    const c = fp('c', { cu_limit: { kind: 'range', min: '1', max: '2' } });
    const w = fp('w', {}, true);
    expect(fingerprintIdentityKey(fingerprintToIdentity(a))).toBe(
      fingerprintIdentityKey(fingerprintToIdentity(b)),
    );
    expect(fingerprintIdentityKey(fingerprintToIdentity(a))).not.toBe(
      fingerprintIdentityKey(fingerprintToIdentity(c)),
    );
    // An empty row and a wildcard match opposite token sets — never one key.
    expect(fingerprintIdentityKey(fingerprintToIdentity(fp('e', {})))).not.toBe(
      fingerprintIdentityKey(fingerprintToIdentity(w)),
    );
  });
});

describe('findFingerprintForGroupKey', () => {
  const gk = { cu_limit: window('200000', '200000') };

  it('prefers an exact identity over a compatible refinement', () => {
    const exact = fp('exact', { cu_limit: exactPredicate('200000') });
    const refined = fp('refined', {
      cu_limit: exactPredicate('200000'),
      ix_labels: { kind: 'sequence', labels: ['A'] },
    });
    expect(findFingerprintForGroupKey(gk, [refined, exact])?.id).toBe('exact');
    expect(
      findFingerprintForGroupKey(gk, [refined, exact], {
        byIdentity: indexFingerprintsByIdentity([refined, exact]),
      })?.id,
    ).toBe('exact');
  });

  it('badges a single compatible refinement, and refuses when two compete', () => {
    const one = fp('one', {
      cu_limit: exactPredicate('200000'),
      ix_labels: { kind: 'sequence', labels: ['A'] },
    });
    const two = fp('two', {
      cu_limit: exactPredicate('200000'),
      ix_labels: { kind: 'sequence', labels: ['B'] },
    });
    expect(findFingerprintForGroupKey(gk, [one])?.id).toBe('one');
    // Two refinements are genuinely different identities — badging either conflates them.
    expect(findFingerprintForGroupKey(gk, [one, two])).toBeNull();
  });

  // "Absent" is not "unconstrained": a fingerprint that configures the axis matches
  // tokens that HAVE a value, the opposite of what the card selected.
  it('does not badge a fingerprint that configures an axis the card has none of', () => {
    const card = { max_cost_lamports: { kind: 'missing' }, cu_limit: window('1', '1') };
    const configured = fp('x', {
      cu_limit: exactPredicate('1'),
      max_cost_lamports: exactPredicate('5'),
    });
    expect(fingerprintCompatibleWithGroupKey(configured, card)).toBe(false);
    expect(fingerprintCompatibleWithGroupKey(fp('y', { cu_limit: exactPredicate('1') }), card)).toBe(
      true,
    );
  });
});

describe('withIxLabelsFilter', () => {
  it('attaches the run filter only when the key omits the axis', () => {
    expect(withIxLabelsFilter({}, ['A', 'B'])).toEqual({
      ix_labels: { kind: 'labels', labels: ['A', 'B'] },
    });
    const grouped = { ix_labels: { kind: 'labels', labels: ['X'] } };
    expect(withIxLabelsFilter(grouped, ['A'])).toBe(grouped);
    // An empty collection is the same sentinel as absent.
    expect(withIxLabelsFilter({}, [])).toEqual({});
    expect(withIxLabelsFilter({}, null)).toEqual({});
  });
});

describe('matchFingerprintsForGroups', () => {
  it('resolves badge + create per group, and lets a scope win outright', () => {
    const saved = fp('saved', { cu_limit: exactPredicate('1') });
    const groups = [
      { g: 0, group_key: { cu_limit: window('1', '1') } },
      { g: 1, group_key: { cu_limit: window('2', '2') } },
      { g: 2, group_key: {} },
    ];
    const map = matchFingerprintsForGroups(groups, [saved], null, null);
    expect(map.get(0)?.matched?.id).toBe('saved');
    expect(map.get(1)?.matched).toBeNull();
    expect(map.get(1)?.canCreate).toBe(true);
    // The ALL group names no criterion, so it cannot become a fingerprint.
    expect(map.get(2)?.canCreate).toBe(false);

    const scoped = matchFingerprintsForGroups(groups, [saved], null, saved);
    expect([...scoped.values()].every((v) => v.matched?.id === 'saved')).toBe(true);
  });

  // A ceiling card used to be un-creatable: no `BIGINT` axis could hold the value.
  it('offers Create on a ceiling card', () => {
    const map = matchFingerprintsForGroups(
      [{ g: 0, group_key: { max_cost_lamports: window(CEILING, CEILING) } }],
      [],
      null,
      null,
    );
    expect(map.get(0)?.canCreate).toBe(true);
  });
});

describe('renderGroupKey', () => {
  it('reads each value in its own display unit', () => {
    const rendered = Object.fromEntries(
      renderGroupKey({
        max_cost_lamports: window('1515000000', '1515000000'),
        init_buy_lamports: window(String(SOL), String(2n * SOL - 1n)),
        ix_count: window('3'),
        prior_launches: window(undefined, '0'),
        ix_labels: { kind: 'labels', labels: ['A', 'B'] },
        token_program_id: { kind: 'text', value: 'Tokenkeg' },
        is_cashback_enabled: { kind: 'flag', value: true },
        spendable_lamports_in: { kind: 'missing' },
      }),
    );
    expect(rendered.max_cost_lamports).toBe('1.515');
    expect(rendered.init_buy_lamports).toBe('1–1.999999999');
    expect(rendered.ix_count).toBe('≥3');
    expect(rendered.prior_launches).toBe('≤0');
    expect(rendered.ix_labels).toBe('A | B');
    expect(rendered.token_program_id).toBe('Tokenkeg');
    expect(rendered.is_cashback_enabled).toBe('true');
    expect(rendered.spendable_lamports_in).toBe('∅');
  });
});

describe('single-value readers', () => {
  // The regression: every one of these values is an object, so a reader that
  // treated a key value as pre-rendered text threw `value.split is not a
  // function` the moment a fingerprint-scoped card carried an `ix_labels` axis.
  it('renders one value exactly as renderGroupKey does', () => {
    expect(groupValueText('max_cost_lamports', window('1515000000', '1515000000'))).toBe('1.515');
    expect(groupValueText('ix_labels', { kind: 'labels', labels: ['A', 'B'] })).toBe('A | B');
    expect(groupValueText('spendable_lamports_in', { kind: 'missing' })).toBe('∅');
  });

  it('reads a label sequence only off a labels value', () => {
    expect(groupValueLabels({ kind: 'labels', labels: ['A', 'B'] })).toEqual(['A', 'B']);
    expect(groupValueLabels({ kind: 'labels', labels: [] })).toBeNull();
    expect(groupValueLabels({ kind: 'missing' })).toBeNull();
    expect(groupValueLabels(window('3'))).toBeNull();
    expect(groupValueLabels('A | B')).toBeNull();
  });
});
