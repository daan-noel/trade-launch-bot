import { describe, expect, it } from 'vitest';
import {
  exitBreakdown,
  runSummaryFromRows,
  zeroExitCounts,
  type RunMetrics,
} from './runSummary';

function metrics(partial: Partial<RunMetrics>): RunMetrics {
  return {
    n_fired: 10,
    n_open: 0,
    n_closed: 10,
    win_rate: 0.5,
    total_pnl_sol: 0,
    open_pnl_sol: 0,
    expectancy_sol: 0,
    mean_pnl_pct: 0,
    median_pnl_pct: 0,
    p90_pnl_pct: 0,
    best_pnl_pct: 0,
    worst_pnl_pct: 0,
    std_pnl_pct: 0,
    profit_factor: null,
    score: null,
    avg_holding_secs: 0,
    median_holding_secs: 0,
    n_exit_open: 0,
    ...zeroExitCounts(),
    ...partial,
  };
}

describe('exitBreakdown', () => {
  it('splits Metric exits into Metric+ / Metric- when win/loss are present', () => {
    const slices = exitBreakdown(
      metrics({
        n_closed: 8,
        n_exit_metrics: 5,
        n_exit_metrics_win: 2,
        n_exit_metrics_loss: 3,
        n_exit_take_profit: 2,
        n_exit_stop_loss: 1,
      }),
    );
    const labels = slices.map((s) => s.label);
    expect(labels).toContain('Metric+');
    expect(labels).toContain('Metric-');
    expect(labels).not.toContain('Metric');
    expect(slices.find((s) => s.label === 'Metric+')?.n).toBe(2);
    expect(slices.find((s) => s.label === 'Metric-')?.n).toBe(3);
    expect(slices.reduce((a, s) => a + s.n, 0)).toBe(8);
  });

  it('falls back to a single Metric slice for legacy totals without a split', () => {
    const slices = exitBreakdown(
      metrics({
        n_closed: 4,
        n_exit_metrics: 4,
        n_exit_metrics_win: 0,
        n_exit_metrics_loss: 0,
      }),
    );
    expect(slices.map((s) => s.label)).toContain('Metric');
    expect(slices.map((s) => s.label)).not.toContain('Metric+');
    expect(slices.find((s) => s.label === 'Metric')?.n).toBe(4);
  });
});

describe('runSummaryFromRows', () => {
  it('counts Metrics wins and losses by pnl_sol', () => {
    const { realized } = runSummaryFromRows([
      { fired: true, exit: 'Metrics', pnl_sol: 1, pnl_pct: 10, holding_secs: 5 },
      { fired: true, exit: 'Metrics', pnl_sol: -0.5, pnl_pct: -5, holding_secs: 5 },
      { fired: true, exit: 'TakeProfit', pnl_sol: 2, pnl_pct: 20, holding_secs: 5 },
    ]);
    expect(realized.n_exit_metrics).toBe(2);
    expect(realized.n_exit_metrics_win).toBe(1);
    expect(realized.n_exit_metrics_loss).toBe(1);
    expect(realized.n_exit_take_profit).toBe(1);
  });

  it('counts metric DETAIL labels as metric exits, not Other', () => {
    // What the grouped-sweep drill-in and the live engine actually stamp
    // (`exit_reason_string` / `format_metric_exit_label`) — bare `Metrics` is
    // legacy-only, so an exact-string test dumped the whole mix into `Other`.
    const { realized } = runSummaryFromRows([
      { fired: true, exit: 'pnl >= 20', pnl_sol: 1, pnl_pct: 20, holding_secs: 5 },
      { fired: true, exit: 'retrace >= 12.5', pnl_sol: -0.5, pnl_pct: -12, holding_secs: 5 },
      { fired: true, exit: 'stall>', pnl_sol: -0.2, pnl_pct: -3, holding_secs: 5 },
    ]);
    expect(realized.n_exit_metrics).toBe(3);
    expect(realized.n_exit_metrics_win).toBe(1);
    expect(realized.n_exit_metrics_loss).toBe(2);
    const slices = exitBreakdown(realized);
    expect(slices.map((s) => s.label)).not.toContain('Other');
  });
});
