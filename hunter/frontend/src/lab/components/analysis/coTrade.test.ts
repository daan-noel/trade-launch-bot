import { describe, expect, it } from 'vitest';

// The backend's own source, read raw — the co-trade cap lives there and this
// file's copy has to follow it. Same one-copy discipline as the flow-split
// parity fixture.
import walletsHandlerSrc from '../../../../../lab/src/api/handlers/wallets.rs?raw';

import { COMPARE_MARKER_COLORS } from 'components/token-price-chart/constants';
import type { CoTrader, TraderTokenRow } from 'types';
import {
  coBucket,
  coLagSlots,
  coTradeMix,
  coBucketKey,
  coDepthCounts,
  coTradePerWallet,
  coupledCount,
  firstMover,
  formatLagSlots,
  hasCoTrader,
  matchesCoBuckets,
  MAX_COMPARE_WALLETS,
  passesCoFilter,
  pickCoTrader,
  tightestCoTrader,
} from './coTrade';

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

describe('MAX_COMPARE_WALLETS', () => {
  it('equals the backend cap it mirrors', () => {
    // `wallets.rs` takes the first N of `with=` and drops the rest without a
    // word, so a picker that allows more silently shows wallets in the legend
    // that were never queried - they read as "compare only works on the first
    // few". One number, asserted against the source that enforces it.
    const m = /const MAX_COMPARISON_WALLETS: usize = (\d+);/.exec(walletsHandlerSrc);
    expect(m, 'MAX_COMPARISON_WALLETS not found in wallets.rs').not.toBeNull();
    expect(Number(m![1])).toBe(MAX_COMPARE_WALLETS);
  });

  it('has one distinct comparison hue per slot the cap allows', () => {
    // `compareWalletColor` wraps modulo the palette, so a palette shorter than
    // the cap hands two compared wallets the same color on every chart.
    expect(COMPARE_MARKER_COLORS.length).toBeGreaterThanOrEqual(MAX_COMPARE_WALLETS);
    expect(new Set(COMPARE_MARKER_COLORS).size).toBe(COMPARE_MARKER_COLORS.length);
  });
});

describe('a focused comparison wallet', () => {
  const r = row([co('near', 1), co('far', -8)]);

  it('answers for that wallet even when another sits closer', () => {
    expect(pickCoTrader(r, 'far')?.wallet).toBe('far');
    expect(coLagSlots(r, 'far')).toBe(-8);
    expect(coBucket(r, 'far')).toBe('independent');
    // Unfocused, the same row still reports its tightest coupling.
    expect(coLagSlots(r)).toBe(1);
  });

  it('is blank - never the tightest of the others - on a row it is absent from', () => {
    expect(pickCoTrader(r, 'elsewhere')).toBeNull();
    expect(coLagSlots(r, 'elsewhere')).toBeNull();
    expect(coBucket(r, 'elsewhere')).toBeNull();
  });

  it('narrows the co-traded-only predicate to itself', () => {
    expect(hasCoTrader(r)).toBe(true);
    expect(hasCoTrader(r, 'near')).toBe(true);
    expect(hasCoTrader(r, 'elsewhere')).toBe(false);
    expect(hasCoTrader(row([]), null)).toBe(false);
  });

  it('re-buckets the mix on its own entries', () => {
    const rows = [row([co('a', 0), co('b', 30)]), row([co('b', 0)]), row([co('a', 12)])];
    // Unfocused every row counts once, on the tightest: two co-slot, one independent.
    const all = coTradeMix(rows);
    expect(all.overlap).toBe(3);
    expect(all.byBucket['co-slot']).toBe(2);
    // `b` was on two of them, coupled on only one.
    const b = coTradeMix(rows, 'b');
    expect(b.overlap).toBe(2);
    expect(b.byBucket['co-slot']).toBe(1);
    expect(b.byBucket.independent).toBe(1);
    expect(b.total).toBe(3);
  });
});

describe('coTradePerWallet', () => {
  it('splits overlap and coupling per wallet, in slot order', () => {
    // The read the headline cannot give: `busy` is on every row, `rare` on one.
    const rows = [row([co('busy', 0), co('rare', -1)]), row([co('busy', 2)]), row([co('busy', 40)])];
    const [busy, rare] = coTradePerWallet(rows, ['busy', 'rare']);
    expect(busy.wallet).toBe('busy');
    expect(busy.overlap).toBe(3);
    expect(busy.coupled).toBe(2);
    expect(rare.overlap).toBe(1);
    expect(rare.coupled).toBe(1);
    // Both share the cohort as their denominator, so the shares are comparable.
    expect(busy.total).toBe(3);
    expect(rare.total).toBe(3);
  });

  it('keeps a wallet with no overlap at all rather than dropping it', () => {
    const [ghost] = coTradePerWallet([row([co('a', 0)])], ['ghost']);
    expect(ghost.overlap).toBe(0);
    expect(coupledCount(ghost)).toBe(0);
  });
});

describe('passesCoFilter', () => {
  // Three tokens: one every wallet was on, one two of them, one just `a`.
  const all3 = row([co('a', 0), co('b', 1), co('c', -2)]);
  const two = row([co('a', 0), co('b', 40)]);
  const one = row([co('a', 3)]);
  const none = row([]);

  it('at depth 1 is the UNION - any single comparison wallet is enough', () => {
    expect([all3, two, one, none].map((r) => passesCoFilter(r, null, 1))).toEqual([
      true,
      true,
      true,
      false,
    ]);
  });

  it('at the set size is the INTERSECTION - every wallet had to be there', () => {
    expect([all3, two, one, none].map((r) => passesCoFilter(r, null, 3))).toEqual([
      true,
      false,
      false,
      false,
    ]);
  });

  it('accepts the depths in between, where a family turns out in part', () => {
    expect(passesCoFilter(two, null, 2)).toBe(true);
    expect(passesCoFilter(one, null, 2)).toBe(false);
  });

  it('composes with a focus rather than replacing it', () => {
    // Depth 2 is met, but the focused wallet is not one of the two.
    expect(passesCoFilter(two, 'b', 2)).toBe(true);
    expect(passesCoFilter(two, 'c', 2)).toBe(false);
    // And a focus alone does not lower the depth bar.
    expect(passesCoFilter(one, 'a', 2)).toBe(false);
  });

  it('treats a depth under 1 as 1 - the filter never admits an untouched token', () => {
    expect(passesCoFilter(none, null, 0)).toBe(false);
    expect(passesCoFilter(one, null, -3)).toBe(true);
  });
});

describe('coDepthCounts', () => {
  it('is a cumulative ladder: each rung counts the rows at that depth OR deeper', () => {
    const rows = [
      row([co('a', 0), co('b', 1), co('c', 2)]),
      row([co('a', 0), co('b', 1)]),
      row([co('a', 0)]),
      row([]),
    ];
    // 3 rows carry >=1 wallet, 2 carry >=2, 1 carries all 3.
    expect(coDepthCounts(rows, 3)).toEqual([3, 2, 1]);
  });

  it('reports a zero intersection rather than omitting the rung', () => {
    // The distinction the strip exists to make: "all 3" is a real, empty answer.
    expect(coDepthCounts([row([co('a', 0)]), row([co('b', 0)])], 3)).toEqual([2, 0, 0]);
  });

  it('never writes past the ladder when a row outruns the set size', () => {
    // A stale row fetched under a wider comparison set.
    expect(coDepthCounts([row([co('a', 0), co('b', 0), co('c', 0)])], 2)).toEqual([1, 1]);
  });

  it('is empty for an empty comparison set', () => {
    expect(coDepthCounts([row([co('a', 0)])], 0)).toEqual([]);
  });
});

describe('coBucketKey / matchesCoBuckets', () => {
  it('names the bucket a row is COUNTED under, so a badge selects its own count', () => {
    expect(coBucketKey(row([co('a', 0)]))).toBe('co-slot');
    expect(coBucketKey(row([co('a', -2)]))).toBe('leads');
    expect(coBucketKey(row([co('a', 2)]))).toBe('follows');
    expect(coBucketKey(row([co('a', 40)]))).toBe('independent');
  });

  it('calls an unorderable overlap `unordered`, not nothing', () => {
    // Shared, but one side has no entry leg in the window. Selectable like the
    // rest - otherwise those rows are unreachable from the strip.
    expect(coBucketKey(row([co('a', null)]))).toBe('unordered');
  });

  it('is null only when the row was not shared at all', () => {
    expect(coBucketKey(row([]))).toBeNull();
    // Under a focus, "not shared" means not shared BY THAT WALLET.
    expect(coBucketKey(row([co('a', 0)]), 'b')).toBeNull();
    expect(coBucketKey(row([co('a', 0), co('b', 9)]), 'b')).toBe('independent');
  });

  it('treats an empty selection as no narrowing, never as nothing matches', () => {
    expect(matchesCoBuckets(row([]), null, [])).toBe(true);
    expect(matchesCoBuckets(row([co('a', 0)]), null, [])).toBe(true);
  });

  it('combines the selected buckets as OR, and drops the unshared rows', () => {
    const sel = ['co-slot', 'leads'] as const;
    expect(matchesCoBuckets(row([co('a', 0)]), null, sel)).toBe(true);
    expect(matchesCoBuckets(row([co('a', -1)]), null, sel)).toBe(true);
    expect(matchesCoBuckets(row([co('a', 2)]), null, sel)).toBe(false);
    expect(matchesCoBuckets(row([]), null, sel)).toBe(false);
  });

  it('selects on the FOCUSED wallet when one is set', () => {
    // Tightest is `a` at same-slot; focused on `b` the row reads independent.
    const r = row([co('a', 0), co('b', 9)]);
    expect(matchesCoBuckets(r, null, ['co-slot'])).toBe(true);
    expect(matchesCoBuckets(r, 'b', ['co-slot'])).toBe(false);
    expect(matchesCoBuckets(r, 'b', ['independent'])).toBe(true);
  });
});
