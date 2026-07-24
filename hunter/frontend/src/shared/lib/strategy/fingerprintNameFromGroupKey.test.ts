import { describe, expect, it } from 'vitest';
import { fingerprintNameFromGroupKey } from './fingerprintNameFromGroupKey';

describe('fingerprintNameFromGroupKey (C3)', () => {
  it('skips ∅ axes, glues lo-edge buckets, collapses ix, appends non-default bucket', () => {
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
        'c',
        0.5,
      ),
    ).toBe('c · fsb19.5 · fss0 · max0 · 3ix · b0.5');
  });

  it('omits b{width} when bucket is the default 0.1', () => {
    expect(
      fingerprintNameFromGroupKey(
        { first_slot_buy_sol: '19.5–19.6', ix_labels: 'A | B' },
        'c',
        0.1,
      ),
    ).toBe('c · fsb19.5 · 2ix');
  });

  it('uses flow provenance and skips grouping-only axes', () => {
    expect(
      fingerprintNameFromGroupKey(
        { cu_limit: '200000', is_cashback_enabled: 'true' },
        'f',
      ),
    ).toBe('f · cu200000');
  });

  it('falls back to SOURCE · ALL when nothing configured', () => {
    expect(fingerprintNameFromGroupKey({}, 'c')).toBe('c · ALL');
    expect(fingerprintNameFromGroupKey({ cu_limit: '∅' }, 'c')).toBe('c · ALL');
  });
});
