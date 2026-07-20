import { describe, expect, it } from 'vitest';
import type { Fingerprint } from './types';
import {
  findFingerprintForGroupKey,
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

  it('matches a sparse group key to a sparse fingerprint', () => {
    const hit = findFingerprintForGroupKey({ cu_limit: '200000' }, library, 0.1);
    expect(hit?.id).toBe('b');
  });
});
