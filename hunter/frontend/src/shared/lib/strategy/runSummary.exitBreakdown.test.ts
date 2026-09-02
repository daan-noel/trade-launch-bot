import { describe, expect, it } from 'vitest';
import {
  EXIT_KEY_BY_REASON,
  EXIT_KINDS,
  exitBreakdown,
  exitBreakdownFromRows,
  exitReasonToneClass,
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

describe('exitBreakdownFromRows', () => {
  it('keeps each metric detail label distinct (not Metric±)', () => {
    const slices = exitBreakdownFromRows([
      { exit: 'stall > 300', pnl_sol: -0.2 },
      { exit: 'stall > 300', pnl_sol: 0.5 },
      { exit: 'trail >= 20', pnl_sol: -0.1 },
      { exit: 'TakeProfit', pnl_sol: 1 },
    ]);
    const labels = slices.map((s) => s.label);
    expect(labels).toContain('stall > 300');
    expect(labels).toContain('trail >= 20');
    expect(labels).toContain('Take profit');
    expect(labels).not.toContain('Metric+');
    expect(labels).not.toContain('Metric-');
    expect(slices.find((s) => s.label === 'stall > 300')?.n).toBe(2);
    expect(slices.find((s) => s.label === 'trail >= 20')?.n).toBe(1);
  });

  it('still splits legacy bare Metrics into Metric+ / Metric-', () => {
    const slices = exitBreakdownFromRows([
      { exit: 'Metrics', pnl_sol: 1 },
      { exit: 'Metrics', pnl_sol: -0.5 },
    ]);
    expect(slices.find((s) => s.label === 'Metric+')?.n).toBe(1);
    expect(slices.find((s) => s.label === 'Metric-')?.n).toBe(1);
  });
});

describe('exitReasonToneClass', () => {
  it('matches EXIT_KINDS hues for persisted reasons and History filter aliases', () => {
    expect(exitReasonToneClass('TakeProfit')).toBe('text-green');
    expect(exitReasonToneClass('StopLoss')).toBe('text-red');
    expect(exitReasonToneClass('Dead')).toBe('text-accent');
    // History cohort uses substring keys (`Trailing` → TrailingStop).
    expect(exitReasonToneClass('Trailing')).toBe('text-primary');
    expect(exitReasonToneClass('Time')).toBe('text-warning');
    expect(exitReasonToneClass('Liquidity')).toBe('text-text-mid');
  });
});

/**
 * **The exit-vocabulary lock.**
 *
 * `EXIT_KINDS` is the one list the breakdown renders from, precisely so a reason
 * cannot be silently dropped — but nothing checked it against the engine, and
 * `n_exit_migrated` was added to the Rust `RunMetrics` and never reached it. On a
 * graduation-heavy rule the bars then summed short of `n_closed` while looking
 * complete, which is the failure the list was introduced to prevent.
 *
 * Reading the kernel directly means the next reason fails HERE.
 */
describe('the exit vocabulary matches the engine', () => {
  const rust = Object.values(
    (
      import.meta as unknown as {
        glob(
          pattern: string,
          opts: { eager: true; query: string; import: string },
        ): Record<string, string>;
      }
    ).glob('../../../../../core/src/strategies/kernel.rs', {
      eager: true,
      query: '?raw',
      import: 'default',
    }),
  )[0];

  /** Every `n_exit_*` counter on the Rust `RunMetrics`. */
  const counters = new Set(
    [...rust.matchAll(/^\s*pub (n_exit_[a-z_]+): u32,$/gm)].map((m) => m[1]),
  );
  /** Every persisted `ExitReason` string `ExitCode::from_reason` maps. */
  const reasons = new Set(
    [...rust.matchAll(/^\s*"([A-Za-z]+)" => ExitCode::/gm)].map((m) => m[1]),
  );

  it('reads the Rust kernel — this guard is the lock', () => {
    // A regex that stops matching makes every assertion below vacuously pass.
    expect(rust).toBeTruthy();
    expect(counters.has('n_exit_migrated')).toBe(true);
    expect(reasons.has('Migrated')).toBe(true);
    expect(counters.size).toBeGreaterThanOrEqual(10);
  });

  it('renders every exit counter the engine emits', () => {
    const rendered = new Set<string>(EXIT_KINDS.map((k) => k.key));
    for (const c of counters) {
      // `n_exit_open` is the still-open tally, not a way a position left.
      if (c === 'n_exit_open') continue;
      expect(rendered, `${c} is an engine exit counter with no EXIT_KINDS row — its closes would vanish from the breakdown`)
        .toContain(c);
    }
  });

  it('maps every persisted exit reason to a counter', () => {
    for (const r of reasons) {
      // Neither is an exit: `Open` has not left, `NoEntry` never entered.
      if (r === 'Open' || r === 'NoEntry') continue;
      // A metric exit carries the condition text as its label and is split by
      // realized PnL in `countExits`, so it has no fixed reason string.
      if (r === 'Metrics') continue;
      expect(EXIT_KEY_BY_REASON, `ExitCode::from_reason accepts "${r}" but no counter is mapped to it`)
        .toHaveProperty(r);
    }
  });

  it('counts every rendered reason exactly once', () => {
    // Duplicate keys double-count a close against `n_closed`; duplicate labels
    // collide as React keys and merge two segments into one.
    expect(new Set(EXIT_KINDS.map((k) => k.key)).size).toBe(EXIT_KINDS.length);
    expect(new Set(EXIT_KINDS.map((k) => k.label)).size).toBe(EXIT_KINDS.length);
  });
});
