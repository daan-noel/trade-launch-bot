import { describe, expect, it } from 'vitest';

import { aggregateTradesToBars, aggregateTradesToBarsBySlot } from './chartBars';
import { buildLensMatch, formatLensSol, lensItemMatches } from './lensTint';
import type { ChartTrade } from './types';

/**
 * The tint's whole claim is "this much of THAT candle was the thing you picked".
 * It can only hold while the numerator counts exactly the trades the denominator
 * (`OhlcBar.volume`) counted — so the bars here are built by the real aggregator
 * rather than hand-written, and every case checks the ratio against a candle the
 * chart would actually draw.
 */

const ISO = (sec: number) => new Date(sec * 1000).toISOString();

function trade(p: Partial<ChartTrade> & { slot: number }): ChartTrade {
  return {
    block_time: ISO(1_700_000_000 + p.slot),
    price_per_token: 1e-7,
    trade_type: 'buy',
    amount_sol: 1,
    reserve_sol: 30,
    reserve_token: 3e14,
    wallet_address: 'W',
    ...p,
  };
}

const identity = (v: number) => v;
const bySlot = (trades: ChartTrade[]) =>
  aggregateTradesToBarsBySlot(trades, identity, 'price');

describe('buildLensMatch', () => {
  it('scales the tint by the matched share of the bar, not by presence', () => {
    const trades = [
      trade({ slot: 10, wallet_address: 'A', amount_sol: 1 }),
      trade({ slot: 10, wallet_address: 'B', amount_sol: 3 }),
      trade({ slot: 11, wallet_address: 'A', amount_sol: 4 }),
    ];
    const bars = bySlot(trades);
    const m = buildLensMatch(trades, bars, 'slot', 1, 'price', (t) => t.wallet_address === 'A');

    expect(m.tint).toEqual([
      { barTime: 10, share: 0.25, trades: [trades[0]] },
      { barTime: 11, share: 1, trades: [trades[2]] },
    ]);
    expect(m.buys).toBe(2);
    expect(m.buySol).toBe(5);
    expect(m.firstBarTime).toBe(10);
    expect(m.lastBarTime).toBe(11);
  });

  it('never paints a share above 1 on a bar whose dust it also dropped', () => {
    // 1e-6 is below MIN_CHART_SOL, so the bar's own volume excludes it. Counting
    // it in the numerator would put the wash past full on a candle that never
    // held the trade.
    const trades = [
      trade({ slot: 10, wallet_address: 'A', amount_sol: 1e-6 }),
      trade({ slot: 10, wallet_address: 'A', amount_sol: 2 }),
    ];
    const bars = bySlot(trades);
    const m = buildLensMatch(trades, bars, 'slot', 1, 'price', (t) => t.wallet_address === 'A');

    // The dust leg is dropped from the bar's trade list too: a size label for a
    // trade the candle never held would point at nothing.
    expect(m.tint).toEqual([{ barTime: 10, share: 1, trades: [trades[1]] }]);
    expect(m.buys).toBe(1);
    expect(m.buySol).toBe(2);
  });

  it('splits buys from sells and reports both sides in SOL', () => {
    const trades = [
      trade({ slot: 10, wallet_address: 'A', amount_sol: 2, trade_type: 'buy' }),
      trade({ slot: 12, wallet_address: 'A', amount_sol: 5, trade_type: 'sell' }),
    ];
    const m = buildLensMatch(
      trades,
      bySlot(trades),
      'slot',
      1,
      'price',
      (t) => t.wallet_address === 'A',
    );

    expect(m.buys).toBe(1);
    expect(m.sells).toBe(1);
    expect(m.buySol).toBe(2);
    expect(m.sellSol).toBe(5);
  });

  it('buckets by wall clock in time mode, collapsing slots inside one candle', () => {
    const trades = [
      trade({ slot: 10, wallet_address: 'A', amount_sol: 1 }),
      trade({ slot: 11, wallet_address: 'A', amount_sol: 1 }),
      trade({ slot: 12, wallet_address: 'B', amount_sol: 2 }),
    ];
    const bars = aggregateTradesToBars(trades, 60, identity, 'price');
    const m = buildLensMatch(trades, bars, 'time', 60, 'price', (t) => t.wallet_address === 'A');

    // All three land in one 60s bucket: 2 of 4 SOL is the lens'.
    expect(m.tint).toHaveLength(1);
    expect(m.tint[0].share).toBeCloseTo(0.5);
  });

  it('matches an ix structure only on the exact ordered sequence', () => {
    const trades = [
      trade({ slot: 10, instruction_labels: ['Create', 'Buy'] }),
      trade({ slot: 11, instruction_labels: ['Buy', 'Create'] }),
      trade({ slot: 12, instruction_labels: ['Create', 'Buy', 'Transfer'] }),
      trade({ slot: 13, instruction_labels: null }),
    ];
    const key = JSON.stringify(['Create', 'Buy']);
    const item = { kind: 'structure' as const, key, color: '#000000' };
    const m = buildLensMatch(trades, bySlot(trades), 'slot', 1, 'price', (t) =>
      lensItemMatches(item, t),
    );

    // A reorder, a superset and an unlabeled row are all misses.
    expect(m.tint).toEqual([{ barTime: 10, share: 1, trades: [trades[0]] }]);
  });

  it('matches nothing when no bars are on the chart', () => {
    const m = buildLensMatch([trade({ slot: 10 })], [], 'slot', 1, 'price', () => true);
    expect(m.tint).toEqual([]);
    expect(m.firstBarTime).toBeNull();
  });

  it('keeps every matched trade of a bar, in chart order', () => {
    const trades = [
      trade({ slot: 10, wallet_address: 'A', amount_sol: 0.5 }),
      trade({ slot: 10, wallet_address: 'B', amount_sol: 3 }),
      trade({ slot: 10, wallet_address: 'A', amount_sol: 0.2 }),
    ];
    const item = { kind: 'wallet' as const, key: 'A', color: '#000000' };
    const m = buildLensMatch(trades, bySlot(trades), 'slot', 1, 'price', (t) =>
      lensItemMatches(item, t),
    );

    expect(m.tint).toHaveLength(1);
    expect(m.tint[0].trades.map((t) => t.amount_sol)).toEqual([0.5, 0.2]);
  });
});

describe('formatLensSol', () => {
  it('prints three decimals under 1 SOL, two under 100, none above', () => {
    expect(formatLensSol(0.0421)).toBe('0.042');
    expect(formatLensSol(1.234)).toBe('1.23');
    expect(formatLensSol(123.4)).toBe('123');
    expect(formatLensSol(-0.5)).toBe('0.500');
  });
});
