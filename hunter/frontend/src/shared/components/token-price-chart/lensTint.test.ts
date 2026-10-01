import { describe, expect, it } from 'vitest';

import { aggregateTradesToBars, aggregateTradesToBarsBySlot } from './chartBars';
import { FEE_FIELDS, rowMatchesTrade, type IxPatternRow } from 'lib/strategy/ixPatternRows';
import {
  LENS_PIN_FIELDS,
  buildLensMatch,
  formatLensSol,
  lensItemMatches,
  structureLensKey,
} from './lensTint';
import type { ChartLensPins, ChartTrade } from './types';

/**
 * The tint's whole claim is "the thing you picked traded in THAT candle". It can
 * only hold while a hit counts exactly the trades the candle counted — so the bars
 * here are built by the real aggregator rather than hand-written, and every case
 * checks the hit against a candle the chart would actually draw.
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
  it('marks every bar the lens trades in, with only its own trades', () => {
    const trades = [
      trade({ slot: 10, wallet_address: 'A', amount_sol: 1 }),
      trade({ slot: 10, wallet_address: 'B', amount_sol: 3 }),
      trade({ slot: 11, wallet_address: 'A', amount_sol: 4 }),
    ];
    const bars = bySlot(trades);
    const m = buildLensMatch(trades, bars, 'slot', 1, 'price', (t) => t.wallet_address === 'A');

    expect(m.tint).toEqual([
      { barTime: 10, trades: [trades[0]] },
      { barTime: 11, trades: [trades[2]] },
    ]);
    expect(m.buys).toBe(2);
    expect(m.buySol).toBe(5);
    expect(m.firstBarTime).toBe(10);
    expect(m.lastBarTime).toBe(11);
  });

  it('drops the dust leg the bar itself dropped', () => {
    // 1e-6 is below MIN_CHART_SOL, so the bar's own volume excludes it. Counting
    // it would add SOL to the lens on a candle that never held the trade.
    const trades = [
      trade({ slot: 10, wallet_address: 'A', amount_sol: 1e-6 }),
      trade({ slot: 10, wallet_address: 'A', amount_sol: 2 }),
    ];
    const bars = bySlot(trades);
    const m = buildLensMatch(trades, bars, 'slot', 1, 'price', (t) => t.wallet_address === 'A');

    // The dust leg is dropped from the bar's trade list too: a size label for a
    // trade the candle never held would point at nothing.
    expect(m.tint).toEqual([{ barTime: 10, trades: [trades[1]] }]);
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

    // All three land in one 60s bucket; only A's two trades are the lens'.
    expect(m.tint).toHaveLength(1);
    expect(m.tint[0].trades).toEqual([trades[0], trades[1]]);
  });

  it('matches an ix structure only on the exact ordered sequence', () => {
    const trades = [
      trade({ slot: 10, instruction_labels: ['Create', 'Buy'] }),
      trade({ slot: 11, instruction_labels: ['Buy', 'Create'] }),
      trade({ slot: 12, instruction_labels: ['Create', 'Buy', 'Transfer'] }),
      trade({ slot: 13, instruction_labels: null }),
    ];
    const key = JSON.stringify(['Create', 'Buy']);
    const item = { kind: 'structure' as const, key };
    const m = buildLensMatch(trades, bySlot(trades), 'slot', 1, 'price', (t) =>
      lensItemMatches(item, t),
    );

    // A reorder, a superset and an unlabeled row are all misses.
    expect(m.tint).toEqual([{ barTime: 10, trades: [trades[0]] }]);
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
    const item = { kind: 'wallet' as const, key: 'A' };
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

describe('pinned structure lens', () => {
  const labels = ['ComputeBudget:SetLimit', 'Pump:Buy'];
  const pinned = (pins?: ChartLensPins) => ({
    kind: 'structure' as const,
    key: structureLensKey(labels, pins),
    ...(pins ? { pins } : {}),
  });

  it('keys an unpinned structure as the bare pattern key', () => {
    expect(structureLensKey(labels)).toBe(JSON.stringify(labels));
    expect(structureLensKey(labels, {})).toBe(JSON.stringify(labels));
    expect(structureLensKey(labels, { cu_price: 1000, cu_limit: 300_000 })).toBe(
      `${JSON.stringify(labels)}|cu_limit=300000,cu_price=1000`,
    );
  });

  it('needs every pinned reading to equal its pin, and ignores the rest', () => {
    const item = pinned({ cu_limit: 300_000 });
    const t = (p: Partial<ChartTrade>) => trade({ slot: 1, instruction_labels: labels, ...p });
    expect(lensItemMatches(item, t({ cu_limit: 300_000, cu_price: 5 }))).toBe(true);
    expect(lensItemMatches(item, t({ cu_limit: 200_000 }))).toBe(false);
    // A missing reading never satisfies a pin.
    expect(lensItemMatches(item, t({ cu_limit: null }))).toBe(false);
    // Unpinned: any budget.
    expect(lensItemMatches(pinned(), t({ cu_limit: 200_000 }))).toBe(true);
  });

  // The chart folder inlines the engine's fee match to stay portable; this keeps
  // the copy equal to `ixPatternRows.rowMatchesTrade`.
  it('agrees with rowMatchesTrade on every pin/reading combination', () => {
    expect([...LENS_PIN_FIELDS]).toEqual([...FEE_FIELDS]);
    const readings = [null, 0, 7];
    const pinsOf = [undefined, 0, 7];
    for (const cuLimit of pinsOf)
      for (const tip of pinsOf)
        for (const rl of readings)
          for (const rt of readings) {
            const pins: ChartLensPins = {};
            if (cuLimit != null) pins.cu_limit = cuLimit;
            if (tip != null) pins.tip_lamports = tip;
            const row: IxPatternRow = { labels, ...pins };
            const tr = trade({ slot: 1, instruction_labels: labels, cu_limit: rl, tip_lamports: rt });
            const item = pinned(Object.keys(pins).length > 0 ? pins : undefined);
            expect(lensItemMatches(item, tr)).toBe(rowMatchesTrade(row, labels, tr));
          }
  });
});
