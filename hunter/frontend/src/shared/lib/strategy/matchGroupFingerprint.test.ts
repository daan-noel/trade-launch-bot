import { describe, expect, it } from 'vitest';
import type { Fingerprint } from './types';
import {
  findFingerprintForGroupKey,
  fingerprintCompatibleWithGroupKey,
  fingerprintIdentityFromGroupKey,
  fingerprintIdentityKey,
  fingerprintToIdentity,
  identityHasCriterion,
  identityLamportsAreStorable,
  indexFingerprintsByIdentity,
  matchFingerprintsForGroups,
  parseLoLamports,
  withIxLabelsFilter,
} from './matchGroupFingerprint';

function fp(partial: Partial<Fingerprint> & Pick<Fingerprint, 'id'>): Fingerprint {
  return {
    name: '',
    cu_limit: null,
    cu_price: null,
    init_buy_lamports: null,
    max_cost_lamports: null,
    spendable_lamports_in: null,
    first_slot_buy_lamports: null,
    first_slot_sell_lamports: null,
    bucket_size_amount: 0.1,
    ix_labels: null,
    wildcard: false,
    metric_config: {},
    created_at: '',
    updated_at: '',
    ...partial,
  };
}

describe('identityHasCriterion — must agree with backend has_any_criterion', () => {
  const bare = {
    cu_limit: null,
    cu_price: null,
    init_buy_lamports: null,
    max_cost_lamports: null,
    spendable_lamports_in: null,
    first_slot_buy_lamports: null,
    first_slot_sell_lamports: null,
    bucket_size_amount: 0.1,
    ix_labels: null,
    wildcard: false,
  };

  it('counts a wildcard — the explicit "every token" criterion', () => {
    // Mirrors the Rust `a_wildcard_is_a_criterion_and_saves` guard. Reading a
    // wildcard as criterion-less here offers Create on a row the server accepts
    // (and worse, describes the one match-everything row as unconfigured).
    expect(identityHasCriterion({ ...bare, wildcard: true })).toBe(true);
  });

  it('does not count an empty label list', () => {
    // `[]` is a second spelling of "not set" (Rust `configured_labels`). Counting
    // it offered Create on a group the server rejects as criterion-less.
    expect(identityHasCriterion({ ...bare, ix_labels: [] })).toBe(false);
    expect(identityHasCriterion({ ...bare, ix_labels: ['A'] })).toBe(true);
  });

  it('counts a zero SOL axis — 0 is a real bucket, not "unset"', () => {
    // Mirrors the Rust `a_zero_sol_axis_is_a_criterion_not_an_unset_field` guard:
    // 0 lamports means the bucket [0, width), so it IS a configured criterion.
    expect(identityHasCriterion({ ...bare, spendable_lamports_in: 0 })).toBe(true);
    expect(identityHasCriterion({ ...bare, init_buy_lamports: 0 })).toBe(true);
    expect(identityHasCriterion({ ...bare, cu_limit: 0 })).toBe(true);
  });

  it('an axis-free identity has no criterion (bucket width alone never counts)', () => {
    expect(identityHasCriterion(bare)).toBe(false);
    expect(identityHasCriterion({ ...bare, bucket_size_amount: 5 })).toBe(false);
  });
});

describe('wildcard is part of identity', () => {
  it('keys apart from an otherwise identical axis-free row', () => {
    // The backend `IDENTITY_WHERE` compares `wildcard = $10`, so these are two
    // different fingerprints — one matches every token, the other matches none.
    const anyToken = fingerprintToIdentity(fp({ id: 'w', wildcard: true }));
    const axisFree = fingerprintToIdentity(fp({ id: 'n' }));
    expect(fingerprintIdentityKey(anyToken)).not.toBe(fingerprintIdentityKey(axisFree));
  });

  it('never badges a group card', () => {
    // A group key always names axis VALUES, so a wildcard can never be the card's
    // identity — and it is not a refinement of the card either: it DROPS the axes
    // the card is made of, so badging it would claim the rule arms on that group.
    const wildcard = fp({ id: 'w', wildcard: true });
    expect(fingerprintCompatibleWithGroupKey(wildcard, { cu_limit: '200000' }, 0.1)).toBe(false);
    expect(findFingerprintForGroupKey({ cu_limit: '200000' }, [wildcard], 0.1)).toBeNull();
  });
});

describe('parseLoLamports', () => {
  it('takes the lower edge of an en-dash bucket label', () => {
    expect(parseLoLamports('1.0–1.1')).toBe(1_000_000_000);
  });
  it('parses a plain numeric label', () => {
    expect(parseLoLamports('0.5')).toBe(500_000_000);
  });
});

describe('fingerprintIdentityFromGroupKey', () => {
  it('maps identity axes and skips ∅ / grouping-only fields', () => {
    const id = fingerprintIdentityFromGroupKey(
      {
        cu_limit: '200000',
        initial_buy_sol: '1.0–1.1',
        ix_labels: 'buy | sell',
        is_cashback_enabled: 'true',
        max_cost_lamports: '∅',
      },
      0.1,
    );
    expect(id.cu_limit).toBe(200000);
    expect(id.init_buy_lamports).toBe(1_000_000_000);
    expect(id.ix_labels).toEqual(['buy', 'sell']);
    expect(id.max_cost_lamports).toBeNull();
    expect(id.bucket_size_amount).toBe(0.1);
  });
});

describe('identityLamportsAreStorable — the BIGINT-axis limit', () => {
  const bare = {
    cu_limit: null,
    cu_price: null,
    init_buy_lamports: null,
    max_cost_lamports: null,
    spendable_lamports_in: null,
    first_slot_buy_lamports: null,
    first_slot_sell_lamports: null,
    bucket_size_amount: null,
    ix_labels: null,
  };

  it('rejects a u64 ceiling reconstructed from an exact label', () => {
    // pump.fun's "fill at any price" sentinel: max_sol_cost = u64::MAX. Past both
    // 2^53 and i64::MAX, so it can never be a stored criterion.
    const id = fingerprintIdentityFromGroupKey(
      { max_cost_lamports: '18446744073.709551615' },
      null,
    );
    expect(identityLamportsAreStorable(id)).toBe(false);
  });

  it('accepts real amounts, including 0 and an exact-mode identity', () => {
    expect(identityLamportsAreStorable({ ...bare, init_buy_lamports: 0 })).toBe(true);
    expect(identityLamportsAreStorable({ ...bare, init_buy_lamports: 1_515_000_000 })).toBe(true);
    expect(identityLamportsAreStorable(bare)).toBe(true);
  });
});

describe('fingerprintCompatibleWithGroupKey', () => {
  it('allows a fingerprint refined with extra ix_labels when the card omitted that axis', () => {
    const refined = fp({
      id: 'refined',
      first_slot_buy_lamports: 19_500_000_000,
      first_slot_sell_lamports: 0,
      max_cost_lamports: 0,
      bucket_size_amount: 0.5,
      ix_labels: ['Pump.Fun: Create_v2', 'Associated Token: CreateIdempotent', 'Pump.Fun: Buy'],
    });
    const gk = {
      cu_limit: '∅',
      cu_price: '∅',
      first_slot_buy_sol: '19.5–20.0',
      first_slot_sell_sol: '0.0–0.5',
      max_cost_lamports: '0.0–0.5',
      spendable_lamports_in: '∅',
    };
    expect(fingerprintCompatibleWithGroupKey(refined, gk, 0.5)).toBe(true);
  });

  it('rejects when a present ∅ axis is set on the fingerprint', () => {
    const refined = fp({
      id: 'x',
      cu_limit: 200000,
      first_slot_buy_lamports: 19_500_000_000,
      bucket_size_amount: 0.5,
    });
    expect(
      fingerprintCompatibleWithGroupKey(
        refined,
        { cu_limit: '∅', first_slot_buy_sol: '19.5–20.0' },
        0.5,
      ),
    ).toBe(false);
  });
});

describe('findFingerprintForGroupKey', () => {
  const library = [
    fp({
      id: 'a',
      cu_limit: 200000,
      init_buy_lamports: 1_000_000_000,
      ix_labels: ['buy', 'sell'],
      bucket_size_amount: 0.1,
      used_by: 2,
    }),
    fp({
      id: 'b',
      cu_limit: 200000,
      bucket_size_amount: 0.1,
      used_by: 0,
    }),
  ];

  it('matches full identity including null axes', () => {
    const hit = findFingerprintForGroupKey(
      { cu_limit: '200000', initial_buy_sol: '1.0–1.1', ix_labels: 'buy | sell' },
      library,
      0.1,
    );
    expect(hit?.id).toBe('a');
    expect(hit?.used_by).toBe(2);
  });

  it('does not match when an axis differs', () => {
    const hit = findFingerprintForGroupKey(
      { cu_limit: '200000', initial_buy_sol: '2.0–2.1', ix_labels: 'buy | sell' },
      library,
      0.1,
    );
    expect(hit).toBeNull();
  });

  it('matches a sparse group key to a sparse fingerprint when that is the only fit', () => {
    const hit = findFingerprintForGroupKey(
      { cu_limit: '200000' },
      [library[1]!],
      0.1,
    );
    expect(hit?.id).toBe('b');
  });

  const gkNoLabels = {
    cu_limit: '∅',
    cu_price: '∅',
    first_slot_buy_sol: '19.5–20.0',
    first_slot_sell_sol: '0.0–0.5',
    max_cost_lamports: '0.0–0.5',
    spendable_lamports_in: '∅',
  };
  const sparse = fp({
    id: 'sparse',
    first_slot_buy_lamports: 19_500_000_000,
    first_slot_sell_lamports: 0,
    max_cost_lamports: 0,
    bucket_size_amount: 0.5,
  });
  const refinedA = fp({
    id: 'refined-a',
    first_slot_buy_lamports: 19_500_000_000,
    first_slot_sell_lamports: 0,
    max_cost_lamports: 0,
    bucket_size_amount: 0.5,
    ix_labels: ['Pump.Fun: Create_v2', 'Associated Token: CreateIdempotent', 'Pump.Fun: Buy'],
  });
  const refinedB = fp({
    id: 'refined-b',
    first_slot_buy_lamports: 19_500_000_000,
    first_slot_sell_lamports: 0,
    max_cost_lamports: 0,
    bucket_size_amount: 0.5,
    ix_labels: ['Pump.Fun: Create', 'Pump.Fun: Buy'],
  });

  it('exact identity beats a refined sibling (extra ix_labels)', () => {
    // The card IS `sparse`'s identity — badging the labeled sibling instead
    // would conflate two saved fingerprints that differ only in ix_labels.
    const hit = findFingerprintForGroupKey(gkNoLabels, [refinedA, sparse], 0.5);
    expect(hit?.id).toBe('sparse');
  });

  it('exact identity beats a refined sibling in the sparse-key case too', () => {
    // `b` is the exact identity of the card; `a` only refines it.
    const hit = findFingerprintForGroupKey({ cu_limit: '200000' }, library, 0.1);
    expect(hit?.id).toBe('b');
  });

  it('badges a unique refinement when the exact identity is not saved', () => {
    // The one deliberate superset case: the fingerprint this card was created
    // from was later refined with manual ix_labels — keep its badge.
    const hit = findFingerprintForGroupKey(gkNoLabels, [refinedA], 0.5);
    expect(hit?.id).toBe('refined-a');
  });

  it('returns null when several refinements are compatible (ambiguous)', () => {
    // Two fingerprints differ only in ix_labels the card did not group by —
    // picking either would treat different fingerprints as the same one.
    const hit = findFingerprintForGroupKey(gkNoLabels, [refinedA, refinedB], 0.5);
    expect(hit).toBeNull();
  });

  it('a label-filtered run resolves by EXACT identity once the filter is re-attached', () => {
    // The regression this pairing exists for. The run filtered the corpus to a
    // label set but did not group by it, so `promote_group` copied the filter
    // into the saved fingerprint while the group key omits it. Matching the raw
    // key can only reach the ambiguous superset path; `withIxLabelsFilter`
    // rebuilds the key the backend actually promoted from.
    const labels = refinedA.ix_labels as string[];
    const rawKeyHit = findFingerprintForGroupKey(gkNoLabels, [refinedA, refinedB], 0.5);
    expect(rawKeyHit).toBeNull(); // ambiguous — two label refinements compatible

    const resolved = withIxLabelsFilter(gkNoLabels, labels);
    const hit = findFingerprintForGroupKey(resolved, [refinedA, refinedB], 0.5);
    expect(hit?.id).toBe('refined-a');
    // And it is the identity branch, not the single-compatible fallback: adding
    // more unrelated fingerprints must not change the answer.
    expect(findFingerprintForGroupKey(resolved, [sparse, refinedB, refinedA], 0.5)?.id).toBe(
      'refined-a',
    );
  });

  it('identity index agrees with the linear scan (exact + compatible paths)', () => {
    const byIdentity = indexFingerprintsByIdentity([sparse, refinedA, refinedB, ...library]);
    const cases: { gk: Record<string, string>; width: number | null }[] = [
      { gk: { cu_limit: '200000', initial_buy_sol: '1.0–1.1', ix_labels: 'buy | sell' }, width: 0.1 },
      { gk: { cu_limit: '200000' }, width: 0.1 },
      { gk: gkNoLabels, width: 0.5 },
      { gk: withIxLabelsFilter(gkNoLabels, refinedA.ix_labels as string[]), width: 0.5 },
    ];
    const fps = [sparse, refinedA, refinedB, ...library];
    for (const { gk, width } of cases) {
      expect(findFingerprintForGroupKey(gk, fps, width, { byIdentity })?.id).toBe(
        findFingerprintForGroupKey(gk, fps, width)?.id,
      );
    }
  });

  it('matchFingerprintsForGroups rebuilds identity once and scopes win', () => {
    const groups: { g: number; group_key: Record<string, string> }[] = [
      { g: 0, group_key: { cu_limit: '200000' } },
      { g: 1, group_key: { cu_limit: '200000', initial_buy_sol: '1.0–1.1', ix_labels: 'buy | sell' } },
    ];
    const map = matchFingerprintsForGroups(groups, library, 0.1, null, null);
    expect(map.get(0)?.matched?.id).toBe('b');
    expect(map.get(0)?.identity.cu_limit).toBe(200000);
    expect(map.get(1)?.matched?.id).toBe('a');

    const scoped = library[0]!;
    const scopedMap = matchFingerprintsForGroups(groups, library, 0.1, null, scoped);
    expect(scopedMap.get(0)?.matched?.id).toBe('a');
    expect(scopedMap.get(1)?.matched?.id).toBe('a');
  });

  it('fingerprintIdentityKey is stable for row ↔ rebuilt identity', () => {
    const row = library[0]!;
    const fromRow = fingerprintIdentityKey(fingerprintToIdentity(row));
    const fromGk = fingerprintIdentityKey(
      fingerprintIdentityFromGroupKey(
        { cu_limit: '200000', initial_buy_sol: '1.0–1.1', ix_labels: 'buy | sell' },
        0.1,
      ),
    );
    expect(fromRow).toBe(fromGk);
  });
});

describe('exact-mode identity — an exact-grouped card is a saveable fingerprint', () => {
  // Regression: the creation-stats page used to substitute the 0.1 default for the
  // response's `bucket_width: null`, so an exact card compared against every
  // fingerprint at the wrong precision — no card ever badged and Create was hidden.
  const gk = { initial_buy_sol: '1.515' };
  const exact = fp({ id: 'e', init_buy_lamports: 1_515_000_000, bucket_size_amount: null });
  const bucketed = fp({ id: 'b', init_buy_lamports: 1_515_000_000, bucket_size_amount: 0.1 });

  it('badges the exact fingerprint an exact card would resolve to', () => {
    expect(findFingerprintForGroupKey(gk, [exact, bucketed], null)?.id).toBe('e');
  });

  it('never crosses precision — the two arm on different token sets', () => {
    expect(findFingerprintForGroupKey(gk, [bucketed], null)).toBeNull();
    expect(findFingerprintForGroupKey({ initial_buy_sol: '1.5–1.6' }, [exact], 0.1)).toBeNull();
  });

  it('reconstructs a plain exact label whole, and keeps the NULL width', () => {
    const id = fingerprintIdentityFromGroupKey(gk, null);
    expect(id.init_buy_lamports).toBe(1_515_000_000);
    expect(id.bucket_size_amount).toBeNull();
    expect(identityHasCriterion(id)).toBe(true);
    expect(identityLamportsAreStorable(id)).toBe(true);
  });
});

describe('withIxLabelsFilter — the group_key ⋈ run-filter join', () => {
  it('attaches the applied filter when group_key omitted ix_labels', () => {
    expect(
      withIxLabelsFilter({ cu_limit: '300000' }, ['Pump.Fun: Create', 'Pump.Fun: Buy']),
    ).toEqual({
      cu_limit: '300000',
      ix_labels: 'Pump.Fun: Create | Pump.Fun: Buy',
    });
  });

  it('never overwrites an existing ix_labels key (grouped path)', () => {
    expect(
      withIxLabelsFilter({ cu_limit: '300000', ix_labels: 'A | B' }, [
        'Pump.Fun: Create',
        'Pump.Fun: Buy',
      ]),
    ).toEqual({ cu_limit: '300000', ix_labels: 'A | B' });
  });

  it('is a no-op when no filter is applied', () => {
    expect(withIxLabelsFilter({ cu_limit: '300000' }, null)).toEqual({ cu_limit: '300000' });
    expect(withIxLabelsFilter({ cu_limit: '300000' }, [])).toEqual({ cu_limit: '300000' });
    expect(withIxLabelsFilter({ cu_limit: '300000' }, undefined)).toEqual({
      cu_limit: '300000',
    });
  });

  it('joins with the same separator the backend group key uses', () => {
    // `fingerprint_from_group_key` splits on " | " — a different separator here
    // would produce a single bogus label instead of the set.
    const gk = withIxLabelsFilter({}, ['A', 'B', 'C']);
    expect(fingerprintIdentityFromGroupKey(gk, null).ix_labels).toEqual(['A', 'B', 'C']);
  });
});
