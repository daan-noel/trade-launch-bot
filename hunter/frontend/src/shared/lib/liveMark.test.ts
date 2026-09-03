import { describe, expect, it } from 'vitest';
import {
  liveTradeSpotSolPerRaw,
  netProceedsSol,
  spotSolPerRawToUsd,
  unrealizedFromValue,
  valueSolAtSpot,
} from './liveMark';
import type { CostModel, LiveTrade } from 'types';

function trade(partial: Partial<LiveTrade>): LiveTrade {
  return {
    mint_address: 'Mint111',
    wallet: 'Wal111',
    trade_type: 'buy',
    amount_sol: 1,
    token_amount: 1_000_000,
    price_per_token: 0.000001,
    tx_signature: 'Sig111',
    tx_index: 0,
    leg_index: 0,
    slot: 1,
    timestamp: '2026-01-01T00:00:00Z',
    ...partial,
  };
}

describe('liveTradeSpotSolPerRaw', () => {
  it('prefers reserves over execution price', () => {
    expect(
      liveTradeSpotSolPerRaw(
        trade({ price_per_token: 0.001, reserve_sol: 30, reserve_token: 1_000_000 }),
      ),
    ).toBeCloseTo(30 / 1_000_000);
  });

  it('falls back to price_per_token', () => {
    expect(liveTradeSpotSolPerRaw(trade({ price_per_token: 0.000002 }))).toBe(0.000002);
  });

  it('returns null when no usable spot', () => {
    expect(liveTradeSpotSolPerRaw(trade({ price_per_token: 0, reserve_sol: null }))).toBeNull();
  });
});

describe('spotSolPerRawToUsd', () => {
  it('scales raw→UI then × SOL/USD', () => {
    // 1e-9 SOL/raw × 1e6 decimals = 0.001 SOL/UI × $150 = $0.15
    expect(spotSolPerRawToUsd(1e-9, 6, 150)).toBeCloseTo(0.15);
  });
});

describe('valueSolAtSpot', () => {
  it('multiplies spot × raw amount', () => {
    expect(valueSolAtSpot(1e-6, 2_000_000)).toBeCloseTo(2);
  });
});

/**
 * Explicit constants, not the served ones: these vectors pin the ARITHMETIC, and
 * must not move when `.env` changes a tip.
 */
const COSTS: CostModel = {
  fee_bps_per_leg: 125,
  fixed_cost_sol_per_leg: 0.00025,
  price_impact: true,
};

describe('netProceedsSol', () => {
  /**
   * The cross-language guard. Every row here is also asserted by the Rust
   * `mark_open_bag_golden_vectors` test in `hunter/core/src/strategies/kernel.rs`
   * against the same literals — the browser nets a mark between holdings polls,
   * so if these two drift, one open position has two values.
   */
  it('netProceedsMatchesRust', () => {
    const cases: [number, number, number | null, number, number][] = [
      // [value, costBasis, reserve, wantNetProceeds, wantPnl]
      [0.05, 0.050875000000000004, null, 0.049125000000000002, -0.0017500000000000016],
      [0.1, 0.050875000000000004, 70, 0.09835892857142858, 0.04748392857142858],
      [0.030784, 0.03022, null, 0.030149200000000004, -7.079999999999587e-5],
      [30, 60.75025, 3, -0.00025, -60.7505],
    ];
    for (const [value, costBasis, reserve, wantNet, wantPnl] of cases) {
      expect(netProceedsSol(value, reserve, COSTS)).toBeCloseTo(wantNet, 12);
      expect(unrealizedFromValue(value, costBasis, reserve, COSTS).pnlSol).toBeCloseTo(
        wantPnl,
        12,
      );
    }
  });

  it('charges no impact without depth, and more impact in a shallower pool', () => {
    const flat = netProceedsSol(0.1, null, COSTS);
    const deep = netProceedsSol(0.1, 1000, COSTS);
    const shallow = netProceedsSol(0.1, 10, COSTS);
    expect(flat).toBeGreaterThan(deep);
    expect(deep).toBeGreaterThan(shallow);
  });

  it('clamps proceeds at zero when impact would exceed the pool', () => {
    expect(netProceedsSol(30, 3, COSTS)).toBeCloseTo(-COSTS.fixed_cost_sol_per_leg, 12);
  });

  /** An unmoved price is NOT break-even — the defect this whole path exists to fix. */
  it('reports a flat mark as a loss', () => {
    const { pnlSol, pnlPct } = unrealizedFromValue(0.05, 0.050875, null, COSTS);
    expect(pnlSol).toBeLessThan(0);
    expect(pnlPct).toBeLessThan(0);
  });

  /** No basis ⇒ no percent, so a tile renders a dash instead of a fake 0%. */
  it('has no percent without a basis', () => {
    expect(unrealizedFromValue(0.05, 0, null, COSTS).pnlPct).toBeNull();
  });
});
