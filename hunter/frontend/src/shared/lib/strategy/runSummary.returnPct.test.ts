/**
 * The percent beside a ◎ is money over the capital that produced it — guarded
 * here because the failure is silent: an equal-weighted mean renders a plausible
 * number of the right shape, in the wrong direction.
 */

import { describe, expect, it } from 'vitest';
import {
  bandReturnPct,
  runSummaryFromRows,
  weightedReturnPct,
  type RunOutcomeRow,
} from './runSummary';
import { toRunSummaryFromPositions } from 'components/strategy/SimSummaryCard';
import type { PositionsSummary } from 'types';

const row = (over: Partial<RunOutcomeRow> = {}): RunOutcomeRow => ({
  fired: true,
  exit: 'TakeProfit',
  pnl_sol: 0,
  pnl_pct: 0,
  holding_secs: 10,
  ...over,
});

describe('weightedReturnPct', () => {
  it('is pnl over capital, in percent', () => {
    expect(weightedReturnPct(0.5, 10)).toBeCloseTo(5, 10);
    expect(weightedReturnPct(-0.4, 10.5)).toBeCloseTo(-3.8095, 3);
  });

  it('returns null — not 0 — when no capital was deployed', () => {
    // A tile must render `—`; a `0` asserts an exact break-even nobody measured.
    expect(weightedReturnPct(1, 0)).toBeNull();
    expect(weightedReturnPct(1, -1)).toBeNull();
    expect(weightedReturnPct(1, Number.NaN)).toBeNull();
    expect(weightedReturnPct(Number.NaN, 10)).toBeNull();
  });
});

describe('runSummaryFromRows return %', () => {
  it('weights by capital, not by trade count', () => {
    // The worked example from docs/plans/strategies/pnl-percent-definition.md:
    // ten trades at -5% on 1.0 ◎ and ten at +20% on 0.05 ◎. Count-weighted that
    // reads +7.5% green; the book is down 0.4 ◎.
    const rows: RunOutcomeRow[] = [
      ...Array.from({ length: 10 }, () =>
        row({ pnl_sol: -0.05, pnl_pct: -5, entry_sol: 1.0 }),
      ),
      ...Array.from({ length: 10 }, () =>
        row({ pnl_sol: 0.01, pnl_pct: 20, entry_sol: 0.05 }),
      ),
    ];
    const { realized } = runSummaryFromRows(rows);

    expect(realized.total_pnl_sol).toBeCloseTo(-0.4, 10);
    // The mean of the per-trade percents is the trap, and it is still on the wire.
    expect(realized.mean_pnl_pct).toBeCloseTo(7.5, 10);
    // What the tile shows is not.
    expect(bandReturnPct(realized)).toBeCloseTo(-3.8095, 3);
  });

  it('sign-locks the percent to the ◎ beside it', () => {
    const rows: RunOutcomeRow[] = [
      row({ pnl_sol: -0.05, pnl_pct: -5, entry_sol: 1.0 }),
      row({ pnl_sol: 0.01, pnl_pct: 20, entry_sol: 0.05 }),
    ];
    const { realized, mtm } = runSummaryFromRows(rows);
    expect(Math.sign(bandReturnPct(realized)!)).toBe(Math.sign(realized.total_pnl_sol));
    expect(Math.sign(bandReturnPct(mtm)!)).toBe(Math.sign(mtm.total_pnl_sol));
  });

  it('values the MTM band over every fired position, open capital included', () => {
    const rows: RunOutcomeRow[] = [
      row({ pnl_sol: 0.2, pnl_pct: 20, entry_sol: 1.0 }),
      row({ exit: 'Open', pnl_sol: -0.5, pnl_pct: -50, entry_sol: 1.0 }),
    ];
    const { realized, mtm } = runSummaryFromRows(rows);
    // Realized divides by the 1.0 ◎ that closed; MTM by both positions' 2.0 ◎.
    expect(bandReturnPct(realized)).toBeCloseTo(20, 10);
    expect(bandReturnPct(mtm)).toBeCloseTo(-15, 10);
  });

  it('falls back to the mean when rows carry no entry cost (fixed-notional backtest)', () => {
    // The sweep prices every round trip at one notional and ships no per-row
    // entry, where the mean already IS the capital-weighted return.
    const { realized } = runSummaryFromRows([
      row({ pnl_sol: 0.1, pnl_pct: 10 }),
      row({ pnl_sol: -0.2, pnl_pct: -20 }),
    ]);
    expect(realized.return_pct).toBeNull();
    expect(bandReturnPct(realized)).toBeCloseTo(realized.mean_pnl_pct, 10);
  });
});

const positionsSummary = (over: Partial<PositionsSummary> = {}): PositionsSummary => ({
  tokens: 2,
  open: 1,
  win: 1,
  loss: 0,
  closed: 1,
  win_rate: 100,
  return_pct: 20,
  total_pnl_sol: 0.2,
  open_pnl_sol: -0.5,
  total_entry_sol: 2.0,
  closed_entry_sol: 1.0,
  total_holding_sol: 0.5,
  total_gains_sol: 0.2,
  total_losses_sol: 0,
  avg_hold_secs: 30,
  best_pct: 20,
  worst_pct: 20,
  migrated: 0,
  ...over,
});

describe('live/paper summary → run summary', () => {
  it('gives the MTM band its own denominator', () => {
    const { realized, mtm } = toRunSummaryFromPositions(positionsSummary());
    // Realized is the server's, over closed capital.
    expect(bandReturnPct(realized)).toBeCloseTo(20, 10);
    // MTM is (0.2 - 0.5) / 2.0. Copying realized's percent across printed +20%
    // beside a -0.3 ◎ mark — two headline numbers describing different cohorts.
    expect(mtm.total_pnl_sol).toBeCloseTo(-0.3, 10);
    expect(bandReturnPct(mtm)).toBeCloseTo(-15, 10);
  });

  it('drops the percent rather than dividing by nothing', () => {
    const { realized, mtm } = toRunSummaryFromPositions(
      positionsSummary({
        tokens: 0,
        open: 0,
        closed: 0,
        return_pct: 0,
        total_pnl_sol: 0,
        open_pnl_sol: 0,
        total_entry_sol: 0,
        closed_entry_sol: 0,
      }),
    );
    expect(realized.return_pct).toBe(0);
    expect(mtm.return_pct).toBeNull();
  });
});
