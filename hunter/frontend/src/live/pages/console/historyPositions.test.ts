/**
 * The client fold's rows must carry their own entry cost.
 *
 * Guarded because the failure is silent and only appears under a lens SQL can't
 * express (heat, hold band, Metric±): drop `entry_sol` and the fold has no
 * denominator, so `bandReturnPct` falls back to an equal-weighted mean of
 * percents — a plausible number of the right shape, in the wrong direction, with
 * nothing on screen to say the strip switched definitions.
 */

import { describe, expect, it } from 'vitest';
import { bandReturnPct, runSummaryFromRows } from 'lib/strategy/runSummary';
import type { RulePositionRecord } from 'types';
import { positionsToRunOutcomes } from './historyPositions';

const position = (over: Partial<RulePositionRecord> = {}): RulePositionRecord =>
  ({
    id: 'p1',
    mint_address: 'Mint111',
    wallet: 'W',
    target_price: null,
    target_token_amount: null,
    target_time: null,
    target_tx: null,
    entry_price: 1,
    entry_token_amount: 1,
    entry_time: '2026-08-01T00:00:00Z',
    entry_tx: 'tx',
    exit_price: 1,
    exit_token_amount: 1,
    exit_time: '2026-08-01T00:01:00Z',
    exit_tx: 'tx2',
    pnl_percent: null,
    pnl_sol: 0,
    status: 'End',
    strategy: 's',
    entry_sol: 1,
    rule_id: 'r1',
    exit_reason: 'TakeProfit',
    created_at: '2026-08-01T00:00:00Z',
    updated_at: '2026-08-01T00:01:00Z',
    ...over,
  }) as RulePositionRecord;

describe('positionsToRunOutcomes', () => {
  it("carries each position's entry cost onto the fold", () => {
    const [out] = positionsToRunOutcomes([position({ entry_sol: 0.75, pnl_sol: 0.15 })]);
    expect(out.entry_sol).toBe(0.75);
  });

  it('folds to a capital-weighted return, not a mean of percents', () => {
    // The worked example from docs/plans/strategies/pnl-percent-definition.md.
    // Count-weighted this reads +7.5% green; the book is down 0.4 SOL.
    const rows = [
      ...Array.from({ length: 10 }, (_unused, i) =>
        position({ id: `l${i}`, entry_sol: 1.0, pnl_sol: -0.05 }),
      ),
      ...Array.from({ length: 10 }, (_unused, i) =>
        position({ id: `w${i}`, entry_sol: 0.05, pnl_sol: 0.01 }),
      ),
    ];
    const { realized } = runSummaryFromRows(positionsToRunOutcomes(rows));

    expect(realized.total_pnl_sol).toBeCloseTo(-0.4, 10);
    expect(realized.mean_pnl_pct).toBeCloseTo(7.5, 10);
    expect(bandReturnPct(realized)).toBeCloseTo(-3.8095, 3);
    // The one invariant a reader can check by eye: the two never disagree.
    expect(Math.sign(bandReturnPct(realized)!)).toBe(Math.sign(realized.total_pnl_sol));
  });

  it('leaves a costless row undefined rather than 0, so the fold can detect it', () => {
    // A `0` would read as free capital and deflate the denominator; `undefined`
    // trips the all-or-nothing guard in `metricsOf` instead.
    const [out] = positionsToRunOutcomes([position({ entry_sol: null, entry_price: 1 })]);
    expect(out.entry_sol).toBeUndefined();
  });
});
