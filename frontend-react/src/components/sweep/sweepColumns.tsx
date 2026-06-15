import type { ReactNode } from 'react';
import type { ColumnDef } from 'components/table/types';
import { cn } from 'lib/cn';
import { formatDecimalTrim } from 'utils/format';
import type { SweepResultRecord } from './types';

// --- formatters -------------------------------------------------------------

function fmtNum(v: number | null): string {
  if (v == null) return '—';
  return formatDecimalTrim(v, 4);
}

function fmtSecs(v: number): string {
  if (v <= 0) return '—';
  if (v < 90) return `${Math.round(v)}s`;
  if (v < 5400) return `${(v / 60).toFixed(1)}m`;
  return `${(v / 3600).toFixed(1)}h`;
}

function pctCell(v: number) {
  return (
    <span className={cn('font-medium', v >= 0 ? 'text-green' : 'text-red')}>
      {v >= 0 ? '+' : ''}
      {formatDecimalTrim(v, 1)}%
    </span>
  );
}

function solCell(v: number) {
  return (
    <span className={cn('font-medium', v >= 0 ? 'text-green' : 'text-red')}>
      {v >= 0 ? '+' : ''}
      {formatDecimalTrim(v, 4)}
    </span>
  );
}

// --- column builders --------------------------------------------------------

/** A numeric metric column rendered with `render`, sorted/filtered numerically. */
function metric(
  key: string,
  label: string,
  group: string,
  value: (r: SweepResultRecord) => number,
  render: (r: SweepResultRecord) => ReactNode,
  opts: { tooltip?: string; defaultVisible?: boolean } = {},
): ColumnDef<SweepResultRecord> {
  return {
    key,
    label,
    group,
    tooltip: opts.tooltip,
    defaultVisible: opts.defaultVisible,
    sortable: true,
    render,
    sortValue: value,
    filterNumber: value,
    searchValue: () => '',
  };
}

function pct(
  key: string,
  label: string,
  value: (r: SweepResultRecord) => number,
  opts?: { tooltip?: string; defaultVisible?: boolean },
) {
  return metric(key, label, 'pnl', value, (r) => pctCell(value(r)), opts);
}

function count(
  key: string,
  label: string,
  group: string,
  value: (r: SweepResultRecord) => number,
  opts?: { defaultVisible?: boolean; tooltip?: string },
) {
  return metric(key, label, group, value, (r) => String(value(r)), opts);
}

/**
 * Build the sweep results table columns. The metric columns are fixed; the
 * leading param columns are derived from the run's param keys, so a different
 * strategy's knobs render without changing this file.
 */
export function buildSweepColumns(paramKeys: string[]): ColumnDef<SweepResultRecord>[] {
  const paramCols: ColumnDef<SweepResultRecord>[] = paramKeys.map((k) => ({
    key: `p_${k}`,
    label: k.replace(/_/g, ' '),
    group: 'params',
    sortable: true,
    render: (r) => fmtNum(r.params[k]),
    sortValue: (r) => r.params[k],
    filterNumber: (r) => r.params[k],
    searchValue: () => '',
  }));

  return [
    ...paramCols,

    count('n_fired', 'Fired', 'counts', (r) => r.n_fired, {
      tooltip: 'Tokens this combo took a position on',
    }),
    count('n_closed', 'Closed', 'counts', (r) => r.n_closed, { defaultVisible: false }),
    count('n_open', 'Open', 'counts', (r) => r.n_open, { defaultVisible: false }),

    metric('win_rate', 'Win %', 'pnl', (r) => r.win_rate, (r) => `${(r.win_rate * 100).toFixed(0)}%`, {
      tooltip: 'Share of fired tokens with PnL > 0',
    }),
    metric('total_pnl_sol', 'Total PnL', 'pnl', (r) => r.total_pnl_sol, (r) => solCell(r.total_pnl_sol), {
      tooltip: 'Summed net PnL across all fired tokens (SOL)',
    }),
    metric('expectancy_sol', 'Expectancy', 'pnl', (r) => r.expectancy_sol, (r) => solCell(r.expectancy_sol), {
      tooltip: 'Mean PnL per trade (SOL)',
    }),
    metric(
      'profit_factor',
      'Profit factor',
      'pnl',
      (r) => r.profit_factor ?? Number.POSITIVE_INFINITY,
      (r) => (r.profit_factor == null ? '∞' : r.profit_factor.toFixed(2)),
      { tooltip: 'Gross wins ÷ gross losses' },
    ),
    pct('median_pnl_pct', 'Median %', (r) => r.median_pnl_pct, { tooltip: 'Median per-trade return' }),
    pct('mean_pnl_pct', 'Mean %', (r) => r.mean_pnl_pct, { defaultVisible: false }),
    pct('p90_pnl_pct', 'P90 %', (r) => r.p90_pnl_pct, { defaultVisible: false }),
    pct('best_pnl_pct', 'Best %', (r) => r.best_pnl_pct, { defaultVisible: false }),
    pct('worst_pnl_pct', 'Worst %', (r) => r.worst_pnl_pct, { defaultVisible: false }),

    metric('avg_holding_secs', 'Avg hold', 'holding', (r) => r.avg_holding_secs, (r) => fmtSecs(r.avg_holding_secs)),
    metric(
      'median_holding_secs',
      'Median hold',
      'holding',
      (r) => r.median_holding_secs,
      (r) => fmtSecs(r.median_holding_secs),
      { defaultVisible: false },
    ),

    count('exit_take_profit', 'TP', 'exits', (r) => r.exit_take_profit, { tooltip: 'Exited on take-profit' }),
    count('exit_stop_loss', 'SL', 'exits', (r) => r.exit_stop_loss, { tooltip: 'Exited on stop-loss' }),
    count('exit_trailing', 'Trail', 'exits', (r) => r.exit_trailing, { tooltip: 'Exited on trailing stop' }),
    count('exit_stall', 'Stall', 'exits', (r) => r.exit_stall, { tooltip: 'Exited on stall' }),
    count('exit_time', 'Time', 'exits', (r) => r.exit_time, { tooltip: 'Exited on time stop' }),
    count('exit_liquidity', 'Liq', 'exits', (r) => r.exit_liquidity, { defaultVisible: false }),
    count('exit_cohort', 'Cohort', 'exits', (r) => r.exit_cohort, { defaultVisible: false }),
    count('exit_open', 'Still open', 'exits', (r) => r.exit_open, { defaultVisible: false }),
  ];
}
