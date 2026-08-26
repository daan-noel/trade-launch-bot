import { describe, expect, it } from 'vitest';

import type { CoTrader, TraderTokenRow } from 'types';
import { coBucket, coLagSlots, coTradeMix, firstMover, formatLagSlots, tightestCoTrader } from './coTrade';

/** A comparison wallet at a given signed lag from the primary. */
function co(wallet: string, lagSlots: number | null, lagTx: number | null = 0): CoTrader {
  return {
    wallet,
    entry_at: null,
    entry_slot: lagSlots == null ? null : 1000 + lagSlots,
    entry_tx_index: lagTx,
    entry_curve_sol: null,
    entry_curve_pct: null,
    exit_at: null,
    buy_count: 1,
    sell_count: 0,
    buy_sol: 1,
    sell_sol: 0,
    total_pnl_sol: 0,
    is_open: true,
    partial_data: false,
    entry_lag_slots: lagSlots,
    entry_lag_tx: lagSlots == null ? null : lagTx,
    bucket:
      lagSlots == null
        ? null
        : lagSlots === 0
          ? 'co-slot'
          : lagSlots > 0 && lagSlots <= 3
            ? 'follows'
            : lagSlots < 0 && lagSlots >= -3
              ? 'leads'
              : 'independent',
  };
}

/** A row whose only fields these helpers read are the entry slot + co-traders. */
function row(coTraders: CoTrader[], primaryEntrySlot: number | null = 1000): TraderTokenRow {
  return { wallet_entry_slot: primaryEntrySlot, co_traders: coTraders } as TraderTokenRow;
}

describe('tightestCoTrader', () => {
  it('picks the smallest absolute lag, not the earliest', () => {
    const r = row([co('far', -9), co('near', 2), co('mid', -4)]);
    expect(tightestCoTrader(r)?.wallet).toBe('near');
    expect(coLagSlots(r)).toBe(2);
    expect(coBucket(r)).toBe('follows');
  });

  it('breaks an equal-distance tie toward the wallet that was AHEAD', () => {
    // Both are 2 slots away; the one in front is the interesting reading.
    const r = row([co('behind', 2), co('ahead', -2)]);
    expect(tightestCoTrader(r)?.wallet).toBe('ahead');
    expect(coLagSlots(r)).toBe(-2);
  });

  it('skips wallets with no resolvable lag', () => {
    const r = row([co('unknown', null), co('known', 5)]);
    expect(tightestCoTrader(r)?.wallet).toBe('known');
  });

  it('is null when nothing has a lag', () => {
    expect(tightestCoTrader(row([co('a', null)]))).toBeNull();
    expect(coBucket(row([co('a', null)]))).toBeNull();
    expect(tightestCoTrader(row([]))).toBeNull();
  });
});

describe('firstMover', () => {
  it('names the primary when nobody got in ahead of it', () => {
    expect(firstMover(row([co('a', 1), co('b', 4)]))).toBe('');
  });

  it('names the most-ahead wallet', () => {
    expect(firstMover(row([co('a', -1), co('b', -6), co('c', 2)]))).toBe('b');
  });

  it('breaks a same-slot tie on the tx index — position inside a slot is real', () => {
    // Both landed in the primary's slot; only `entry_lag_tx` separates them.
    expect(firstMover(row([co('a', 0, 3), co('b', 0, -2)]))).toBe('b');
    // Nobody ahead inside the slot leaves the primary first.
    expect(firstMover(row([co('a', 0, 3), co('b', 0, 1)]))).toBe('');
  });

  it('is unknown — never a primary win — when the primary has no entry leg', () => {
    expect(firstMover(row([co('a', null)], null))).toBeNull();
  });
});

describe('coTradeMix', () => {
  it('counts overlaps by bucket and leaves untouched rows out', () => {
    const rows = [
      row([co('a', 0)]),
      row([co('a', 0), co('b', -1)]),
      row([co('a', 12)]),
      row([]),
      row([co('a', null)]),
    ];
    const mix = coTradeMix(rows);
    expect(mix.total).toBe(5);
    // The row with no co-traders is not an overlap; the unordered one still is.
    expect(mix.overlap).toBe(4);
    expect(mix.byBucket['co-slot']).toBe(2);
    expect(mix.byBucket.independent).toBe(1);
    expect(mix.unknown).toBe(1);
  });

  it('is all-zero on an empty cohort rather than dividing by nothing', () => {
    const mix = coTradeMix([]);
    expect(mix.total).toBe(0);
    expect(mix.overlap).toBe(0);
  });
});

describe('formatLagSlots', () => {
  it('names zero rather than printing it — 0 is the finding, not a blank', () => {
    expect(formatLagSlots(0)).toBe('same slot');
    expect(formatLagSlots(-2)).toBe('-2');
    expect(formatLagSlots(3)).toBe('+3');
    expect(formatLagSlots(null)).toBe('-');
  });
});
