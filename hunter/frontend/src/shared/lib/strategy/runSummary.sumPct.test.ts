/**
 * `Σ trade %` — the positions table's PnL% column added up.
 *
 * It sits one tile away from `Return %`, which is capital-weighted, so the two
 * are easy to conflate. Guarded here because the tile is only honest while it
 * covers exactly the closed cohort its neighbours describe: fold an open
 * position in and it silently adds a `0` percent that position never earned.
 */

import { describe, expect, it } from 'vitest';
import {
  runSummaryFromRows,
  runSummarySections,
  type RunMetrics,
  type RunOutcomeRow,
  type RunSummary,
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

describe('runSummaryFromRows Σ trade %', () => {
  it('adds the per-trade percents, ignoring buy size', () => {
    // Same rows as the capital-weighting example: count-weighted arithmetic is
    // wrong for `return_pct` and is exactly the point of this tile.
    const rows: RunOutcomeRow[] = [
      ...Array.from({ length: 10 }, () => row({ pnl_sol: -0.05, pnl_pct: -5, entry_sol: 1.0 })),
      ...Array.from({ length: 10 }, () => row({ pnl_sol: 0.01, pnl_pct: 20, entry_sol: 0.05 })),
    ];
    const { realized } = runSummaryFromRows(rows);
    // 10×(-5) + 10×(+20).
    expect(realized.sum_pnl_pct).toBeCloseTo(150, 10);
    // And it is n × the mean, by construction — the tally, not the average.
    expect(realized.sum_pnl_pct).toBeCloseTo(realized.mean_pnl_pct * realized.n_closed, 10);
  });

  it('sums the closed cohort only, never the open marks', () => {
    const rows: RunOutcomeRow[] = [
      row({ pnl_sol: 0.2, pnl_pct: 20, entry_sol: 1.0 }),
      row({ exit: 'Open', pnl_sol: -0.5, pnl_pct: -50, entry_sol: 1.0 }),
    ];
    const { realized, mtm } = runSummaryFromRows(rows);
    expect(realized.sum_pnl_pct).toBeCloseTo(20, 10);
    // The MTM band reclassifies the open row as settled; it has no realized
    // percent, so the band reports none at all rather than a narrower sum.
    expect(mtm.sum_pnl_pct).toBeNull();
  });

  it('reports nothing — not 0% — on an empty cohort', () => {
    expect(runSummaryFromRows([]).realized.sum_pnl_pct).toBeNull();
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
  sum_pnl_pct: 150,
  best_pct: 20,
  worst_pct: 20,
  migrated: 0,
  ...over,
});

describe('live/paper summary → Σ trade %', () => {
  it('carries the SQL aggregate onto the realized band only', () => {
    const { realized, mtm } = toRunSummaryFromPositions(positionsSummary());
    expect(realized.sum_pnl_pct).toBe(150);
    expect(mtm.sum_pnl_pct).toBeNull();
  });

  it('drops the tile on a live binary that predates the aggregate', () => {
    // `undefined` on the wire must not render as a measured `+0%`.
    const { realized } = toRunSummaryFromPositions(positionsSummary({ sum_pnl_pct: undefined }));
    expect(realized.sum_pnl_pct).toBeNull();
  });
});

/**
 * The server wire: the Rust `RunMetrics` derives `mean_pnl_pct` from a per-trade
 * sum it then discards, and ships no `return_pct` (a backtest's notional is
 * fixed). Simulate and grouped sweep both serialize that struct, so strip both
 * fields to reproduce exactly what those surfaces receive.
 */
const serverWire = (rows: RunOutcomeRow[]): RunSummary => {
  const strip = (m: RunMetrics): RunMetrics => {
    const { sum_pnl_pct: _sum, return_pct: _ret, ...rest } = m;
    return rest as RunMetrics;
  };
  const { realized, mtm } = runSummaryFromRows(rows);
  return { realized: strip(realized), mtm: strip(mtm) };
};

const tileOf = (s: RunSummary, section: string, label: string) =>
  runSummarySections(s).sections.find((x) => x.title === section)?.stats.find((t) => t.label === label);

describe('runSummarySections Σ trade % on a server summary', () => {
  it('recovers the sum from mean × n_closed', () => {
    // Ten at -5% and ten at +20%: the tile must read the same +150% the live SQL
    // aggregate sums directly, not the +7.5% mean sitting one tile away.
    const rows: RunOutcomeRow[] = [
      ...Array.from({ length: 10 }, () => row({ pnl_sol: -0.05, pnl_pct: -5 })),
      ...Array.from({ length: 10 }, () => row({ pnl_sol: 0.01, pnl_pct: 20 })),
    ];
    expect(tileOf(serverWire(rows), 'Realized', 'Σ trade %')?.value).toBe('+150%');
  });

  it('leaves the MTM band without one', () => {
    // The open row is reclassified as settled there, so a sum would cover a
    // cohort with no realized percent in it.
    const s = serverWire([
      row({ pnl_sol: 0.2, pnl_pct: 20 }),
      row({ exit: 'Open', pnl_sol: -0.5, pnl_pct: -50 }),
    ]);
    expect(tileOf(s, 'Realized', 'Σ trade %')?.value).toBe('+20%');
    expect(tileOf(s, 'Incl. open (MTM)', 'Σ trade %')).toBeUndefined();
  });

  it('never derives one where the notional varies', () => {
    // `return_pct` present ⇒ `mean_pnl_pct` carries the capital-weighted return,
    // and mean × n is a sum of nothing. That wire ships its own real sum, so the
    // guard costs the tile nothing — dropping it here is the whole point.
    const { realized } = toRunSummaryFromPositions(positionsSummary({ sum_pnl_pct: undefined }));
    const s: RunSummary = { realized, mtm: realized };
    expect(tileOf(s, 'Realized', 'Σ trade %')).toBeUndefined();
  });
});
