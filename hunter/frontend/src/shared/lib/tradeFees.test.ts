import { describe, expect, it } from 'vitest';

import { tradePriorityLamports, tradePrioritySol, tradeTipSol } from './tradeFees';

/** Only the three fee-budget fields matter here. */
const t = (
  cu_limit: number | null,
  cu_price: number | null,
  tip_lamports: number | null,
) => ({ cu_limit, cu_price, tip_lamports });

describe('tradePriorityLamports', () => {
  it('sums both rails', () => {
    // 300k CU at 3,333,333 micro-lamports/CU = 1,000,000 lamports, plus a 0.002
    // SOL tip.
    expect(tradePriorityLamports(t(300_000, 3_333_333, 2_000_000))).toBe(3_000_000);
  });

  it('collapses the pairs that encode one decision', () => {
    // The whole reason the parts are not comparable: these are the same spend.
    const spend = 1_000_000;
    expect(tradePriorityLamports(t(300_000, 3_333_333, null))).toBe(spend);
    expect(tradePriorityLamports(t(100_000, 10_000_000, null))).toBe(spend);
    expect(tradePriorityLamports(t(1_000_000, 1_000_000, null))).toBe(spend);
    // ...while cu_price alone splits them three ways.
  });

  it('rounds the compute rail UP, the way the chain charges it', () => {
    expect(tradePriorityLamports(t(1, 1, null))).toBe(1);
  });

  it('reads a tip-only sender', () => {
    expect(tradePriorityLamports(t(null, null, 500_000))).toBe(500_000);
  });

  it('reads a compute-only sender', () => {
    expect(tradePriorityLamports(t(200_000, 5_000_000, null))).toBe(1_000_000);
  });

  it('needs BOTH compute parts before it charges the compute rail', () => {
    // A limit with no price buys no priority — the rail costs nothing until a
    // price is set, so half the pair contributes 0, not a guess.
    expect(tradePriorityLamports(t(300_000, null, 7))).toBe(7);
    expect(tradePriorityLamports(t(null, 3_333_333, 7))).toBe(7);
  });

  it('is null only when NOTHING was captured', () => {
    expect(tradePriorityLamports(t(null, null, null))).toBeNull();
  });

  it('keeps a real zero tip as a reading, not an absence', () => {
    // `tip_lamports === 0` means "transfers, none to a recognised tip account".
    // It must not collapse to null the way a missing field does.
    expect(tradePriorityLamports(t(null, null, 0))).toBe(0);
    expect(tradePriorityLamports(t(300_000, 3_333_333, 0))).toBe(1_000_000);
  });
});

describe('tradeTipSol', () => {
  it('converts lamports to SOL', () => {
    expect(tradeTipSol({ tip_lamports: 1_000_000 })).toBe(0.001);
  });

  it('distinguishes "no transfer" from "no recognised tip"', () => {
    expect(tradeTipSol({ tip_lamports: null })).toBeNull();
    expect(tradeTipSol({ tip_lamports: 0 })).toBe(0);
  });
});

describe('tradePrioritySol', () => {
  it('converts the summed lamports to SOL and propagates null', () => {
    expect(tradePrioritySol(t(300_000, 3_333_333, 2_000_000))).toBe(0.003);
    expect(tradePrioritySol(t(null, null, null))).toBeNull();
  });
});
