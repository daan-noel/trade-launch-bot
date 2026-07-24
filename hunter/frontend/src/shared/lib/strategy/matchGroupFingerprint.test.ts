import { describe, expect, it } from 'vitest';
import type { Fingerprint } from './types';
import {
  findFingerprintForGroupKey,
  fingerprintCompatibleWithGroupKey,
  fingerprintIdentityFromGroupKey,
  parseLoLamports,
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
    metric_config: {},
    created_at: '',
    updated_at: '',
    ...partial,
  };
}

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

  it('prefers a refined fingerprint (extra ix_labels) over a sparse duplicate', () => {
    const sparse = fp({
      id: 'sparse',
      first_slot_buy_lamports: 19_500_000_000,
      first_slot_sell_lamports: 0,
      max_cost_lamports: 0,
      bucket_size_amount: 0.5,
    });
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
    const hit = findFingerprintForGroupKey(gk, [sparse, refined], 0.5);
    expect(hit?.id).toBe('refined');
  });

  it('prefers the more-specific fingerprint when a sparse group key fits several', () => {
    const hit = findFingerprintForGroupKey({ cu_limit: '200000' }, library, 0.1);
    expect(hit?.id).toBe('a');
  });
});
