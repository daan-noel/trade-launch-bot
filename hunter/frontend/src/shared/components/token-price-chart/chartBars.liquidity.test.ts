import { describe, expect, it } from 'vitest';

import { curveLiquiditySol, tradeLiquiditySol } from './chartBars';
import { PUMP_INITIAL_VIRTUAL_SOL } from './constants';
import type { ChartTrade } from './types';

/**
 * `tradeLiquiditySol` is the TS mirror of the Rust SSOT
 * `config::constants::approx_real_sol_reserves(reserve_sol, venue)`. The two live
 * either side of a language boundary, so this file is the guard that keeps the
 * copies equal (super-root CLAUDE.md: unavoidable duplication needs a guard test).
 *
 * The bug this locks: the field was read as `real_sol_reserves` while the backend
 * serializes `real_reserve_sol`, so it was *always* undefined and every row —
 * including post-migration AMM rows, which carry no virtual offset — fell through
 * to `reserve_sol − 30`.
 */
const base: ChartTrade = {
  trade_type: 'buy',
  amount_sol: 1,
  token_amount: 1,
  price_per_token: 1,
  tx_signature: 'sig',
  tx_index: 0,
  leg_index: 0,
  slot: 1,
  block_time: '2026-08-07T00:00:00Z',
} as ChartTrade;

describe('tradeLiquiditySol', () => {
  it('subtracts the virtual offset on curve rows', () => {
    const t = { ...base, venue: 'curve' as const, reserve_sol: 44.65 };
    expect(tradeLiquiditySol(t)).toBeCloseTo(44.65 - PUMP_INITIAL_VIRTUAL_SOL, 10);
    expect(tradeLiquiditySol(t)).toBe(curveLiquiditySol(t));
  });

  it('does NOT subtract the virtual offset on amm rows', () => {
    // Post-migration `reserve_sol` IS the pool balance. Subtracting 30 here
    // understated every AMM row, and floored small pools to 0.
    const t = { ...base, venue: 'amm' as const, reserve_sol: 12 };
    expect(tradeLiquiditySol(t)).toBe(12);
  });

  it('prefers the program-emitted real reserve on either venue', () => {
    expect(
      tradeLiquiditySol({ ...base, venue: 'curve', reserve_sol: 44.65, real_reserve_sol: 14.2 }),
    ).toBe(14.2);
    expect(
      tradeLiquiditySol({ ...base, venue: 'amm', reserve_sol: 12, real_reserve_sol: 11.8 }),
    ).toBe(11.8);
  });

  it('reads the field name the backend actually serializes', () => {
    // `Trade` (hunter/core/src/models/trade.rs) serializes `real_reserve_sol`.
    // A row keyed the old way must not resolve — that silent miss WAS the bug.
    const wrong = { ...base, venue: 'amm' as const, real_sol_reserves: 11.8 } as ChartTrade;
    expect(tradeLiquiditySol(wrong)).toBeNull();
  });

  it('is null when there is no reserve snapshot at all', () => {
    expect(tradeLiquiditySol({ ...base, venue: 'curve' })).toBeNull();
    expect(tradeLiquiditySol({ ...base, venue: 'amm' })).toBeNull();
  });

  it('clamps a curve row below the virtual floor to 0, never negative', () => {
    // The 2026-08-07 incident's fill: 30.276 virtual ⇒ 0.276 real.
    expect(tradeLiquiditySol({ ...base, venue: 'curve', reserve_sol: 30.276 })).toBeCloseTo(0.276, 10);
    expect(tradeLiquiditySol({ ...base, venue: 'curve', reserve_sol: 12 })).toBe(0);
  });
});
