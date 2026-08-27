import { describe, expect, it } from 'vitest';
import {
  fingerprintAutoName,
  fingerprintNameFromGroupKey,
  isLegacyAutoName,
} from './fingerprintNameFromGroupKey';

const buyIx = [
  'Pump.Fun: Create_v2',
  'Associated Token: CreateIdempotent',
  'Pump.Fun: Buy',
];

describe('fingerprintAutoName', () => {
  // Golden strings — keep byte-equal with Rust `auto_name_golden`.
  it('puts ix first and uses chip tokens', () => {
    expect(
      fingerprintAutoName({
        wildcard: false,
        cu_limit: null,
        cu_price: null,
        init_buy_lamports: null,
        max_cost_lamports: 1_000_000_000,
        spendable_lamports_in: null,
        first_slot_buy_lamports: null,
        first_slot_sell_lamports: null,
        bucket_size_amount: 1,
        ix_labels: buyIx,
      }),
    ).toBe('3ix:Buy · max=1 · bkt=1');
  });

  it('keeps axis order after ix and omits default 0.1 bucket', () => {
    expect(
      fingerprintAutoName({
        wildcard: false,
        cu_limit: null,
        cu_price: null,
        init_buy_lamports: null,
        max_cost_lamports: 0,
        spendable_lamports_in: null,
        first_slot_buy_lamports: 19_500_000_000,
        first_slot_sell_lamports: 0,
        bucket_size_amount: 0.5,
        ix_labels: buyIx,
      }),
    ).toBe('3ix:Buy · max=0 · fs_buy=19.5 · fs_sell=0 · bkt=0.5');
  });

  it('separates two label sets that differ only past the count', () => {
    const axes = (last: string) =>
      fingerprintAutoName({
        wildcard: false,
        cu_limit: 80_000,
        cu_price: null,
        init_buy_lamports: null,
        max_cost_lamports: null,
        spendable_lamports_in: null,
        first_slot_buy_lamports: null,
        first_slot_sell_lamports: null,
        bucket_size_amount: 0.1,
        ix_labels: [
          'Pump.Fun: Create_v2',
          'Associated Token: Create',
          `Pump.Fun: ${last}`,
        ],
      });
    expect(axes('Buy')).toBe('3ix:Buy · cu_limit=80K');
    expect(axes('BuyExactSolIn')).toBe('3ix:BuyExactSolIn · cu_limit=80K');
    expect(axes('Buy')).not.toBe(axes('BuyExactSolIn'));
  });

  it('omits bkt when width is the default 0.1', () => {
    expect(
      fingerprintAutoName({
        wildcard: false,
        cu_limit: null,
        cu_price: null,
        init_buy_lamports: null,
        max_cost_lamports: null,
        spendable_lamports_in: null,
        first_slot_buy_lamports: 19_500_000_000,
        first_slot_sell_lamports: null,
        bucket_size_amount: 0.1,
        ix_labels: ['A', 'B'],
      }),
    ).toBe('2ix:B · fs_buy=19.5');
  });

  it('compacts cu and drops grouping-only axes (via group-key wrapper)', () => {
    expect(fingerprintAutoName({
      wildcard: false,
      cu_limit: 200_000,
      cu_price: null,
      init_buy_lamports: null,
      max_cost_lamports: null,
      spendable_lamports_in: null,
      first_slot_buy_lamports: null,
      first_slot_sell_lamports: null,
      bucket_size_amount: 0.1,
      ix_labels: null,
    })).toBe('cu_limit=200K');
  });

  it('falls back to ALL when nothing configured', () => {
    expect(
      fingerprintAutoName({
        wildcard: false,
        cu_limit: null,
        cu_price: null,
        init_buy_lamports: null,
        max_cost_lamports: null,
        spendable_lamports_in: null,
        first_slot_buy_lamports: null,
        first_slot_sell_lamports: null,
        bucket_size_amount: 0.1,
        ix_labels: null,
      }),
    ).toBe('ALL');
  });

  it('names a wildcard for the token set it matches, not its inert width', () => {
    // Golden string — keep byte-equal with Rust `a_wildcard_auto_names_all_...`.
    // A wildcard carries no axis and never reads its bucket width, so `bkt=exact`
    // must not leak into the name of the one row that matches everything.
    const anyToken = {
      wildcard: true,
      cu_limit: null,
      cu_price: null,
      init_buy_lamports: null,
      max_cost_lamports: null,
      spendable_lamports_in: null,
      first_slot_buy_lamports: null,
      first_slot_sell_lamports: null,
      ix_labels: null,
    };
    expect(fingerprintAutoName({ ...anyToken, bucket_size_amount: null })).toBe('ALL');
    expect(fingerprintAutoName({ ...anyToken, bucket_size_amount: 0.5 })).toBe('ALL');
  });

  it('always shows bkt=exact', () => {
    expect(
      fingerprintAutoName({
        wildcard: false,
        cu_limit: null,
        cu_price: null,
        init_buy_lamports: null,
        max_cost_lamports: 1_000_000_000,
        spendable_lamports_in: null,
        first_slot_buy_lamports: null,
        first_slot_sell_lamports: null,
        bucket_size_amount: null,
        ix_labels: ['Pump.Fun: Buy'],
      }),
    ).toBe('1ix:Buy · max=1 · bkt=exact');
  });
});

describe('fingerprintNameFromGroupKey', () => {
  it('names from group-key lo-edges the same as identity axes', () => {
    expect(
      fingerprintNameFromGroupKey(
        {
          cu_limit: '∅',
          cu_price: '∅',
          first_slot_buy_sol: '19.5–20.0',
          first_slot_sell_sol: '0.0–0.5',
          max_cost_lamports: '0.0–0.5',
          spendable_lamports_in: '∅',
          ix_labels:
            'Pump.Fun: Create_v2 | Associated Token: CreateIdempotent | Pump.Fun: Buy',
        },
        0.5,
      ),
    ).toBe('3ix:Buy · max=0 · fs_buy=19.5 · fs_sell=0 · bkt=0.5');
  });

  it('skips grouping-only axes and ∅', () => {
    expect(
      fingerprintNameFromGroupKey(
        { cu_limit: '200000', is_cashback_enabled: 'true' },
        0.1,
      ),
    ).toBe('cu_limit=200K');
    expect(fingerprintNameFromGroupKey({}, 0.1)).toBe('ALL');
    expect(fingerprintNameFromGroupKey({ cu_limit: '∅' }, 0.1)).toBe('ALL');
  });
});

describe('isLegacyAutoName', () => {
  it('detects retired generator shapes only', () => {
    expect(isLegacyAutoName('')).toBe(true);
    expect(isLegacyAutoName('  ')).toBe(true);
    expect(isLegacyAutoName('sweep 0f53d622 · group 12')).toBe(true);
    expect(isLegacyAutoName('c · max1 · b1')).toBe(true);
    expect(isLegacyAutoName('f · cu200000')).toBe(true);
    expect(isLegacyAutoName('s · ALL')).toBe(true);
    expect(isLegacyAutoName('flow-discovery bind')).toBe(true);
    expect(isLegacyAutoName('3ix:Buy · max=1 · bkt=1')).toBe(false);
    expect(isLegacyAutoName('max-buy launcher')).toBe(false);
  });
});
