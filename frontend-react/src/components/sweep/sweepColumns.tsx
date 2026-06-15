import type { ReactNode } from 'react';
import type { ColumnDef } from 'components/table/types';
import { cn } from 'lib/cn';
import { formatDecimalTrim } from 'utils/format';
import type { SweepResultRecord } from './types';

// --- formatters -------------------------------------------------------------

function fmtSecs(v: number): string {
  if (v <= 0) return '—';
  if (v < 90) return `${Math.round(v)}s`;
  if (v < 5400) return `${(v / 60).toFixed(1)}m`;
  return `${(v / 3600).toFixed(1)}h`;
}

/**
 * Format a swept param value with the unit its key implies, so the table reads
 * `50%`, `2m`, `5◎` instead of bare numbers. Unit is inferred from the key
 * suffix, so a new strategy's knobs pick up the right unit without edits here.
 */
function fmtParam(key: string, v: number | null): string {
  if (v == null) return '—';
  if (key.endsWith('_secs')) return fmtSecs(v);
  if (key.endsWith('_sol')) return `◎${formatDecimalTrim(v, 4)}`;
  if (key.endsWith('_pct') || key === 'exit_take_profit' || key === 'exit_stop_loss') {
    return `${formatDecimalTrim(v, 1)}%`;
  }
  return formatDecimalTrim(v, 4);
}

function pctText(v: number) {
  return `${v >= 0 ? '+' : ''}${formatDecimalTrim(v, 1)}%`;
}

function solText(v: number) {
  return `◎${v >= 0 ? '+' : ''}${formatDecimalTrim(v, 4)}`;
}

// --- tones ------------------------------------------------------------------
// No backgrounds — only the *text* is colored, by group (params/counts/holding)
// and by meaning (green = good, red = bad on the signed metrics) so the table
// reads at a glance.

/** Wrap cell text in a colored span. `cls` is a Tailwind text-color utility. */
function tone(text: ReactNode, cls: string): ReactNode {
  return <span className={cn('font-medium', cls)}>{text}</span>;
}

/** Green when `v` meets/exceeds `pivot` (good), red below (bad). */
const goodBad = (v: number, pivot = 0) => (v >= pivot ? 'text-green' : 'text-red');

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

/** Diverging-metric column (PnL %): green text when ≥0, red below. */
function pct(
  key: string,
  label: string,
  value: (r: SweepResultRecord) => number,
  opts?: { tooltip?: string; defaultVisible?: boolean },
) {
  return metric(key, label, 'pnl', value, (r) => tone(pctText(value(r)), goodBad(value(r))), opts);
}

/** SOL-metric column: green text when ≥0, red below. */
function sol(
  key: string,
  label: string,
  value: (r: SweepResultRecord) => number,
  opts?: { tooltip?: string; defaultVisible?: boolean },
) {
  return metric(key, label, 'pnl', value, (r) => tone(solText(value(r)), goodBad(value(r))), opts);
}

/** Count column tinted by group color (`cls`), no good/bad meaning. */
function count(
  key: string,
  label: string,
  group: string,
  cls: string,
  value: (r: SweepResultRecord) => number,
  opts?: { defaultVisible?: boolean; tooltip?: string },
) {
  return metric(key, label, group, value, (r) => tone(String(value(r)), cls), opts);
}

/**
 * Master display order for every column — param knobs and result metrics alike.
 * Keys listed here render left-to-right in this order; any column whose key is
 * not listed falls in after them, keeping its build order. Param column keys are
 * prefixed `p_` (e.g. `p_take_profit`); only params the run actually swept get a
 * column, so unswept keys here are harmless (they just never match). This is the
 * single place to reorder the whole table — edit this list.
 */
const COLUMN_ORDER = [
  // params (knobs) — ordered to match the TPSL2 rule modal: entry gates above
  // the trailing/time/stall exit knobs (TP/SL stay leading as the headline pair).
  'p_exit_take_profit',
  'p_exit_stop_loss',
  'p_entry_min_age_secs',
  'p_entry_pullback_pct',
  'p_entry_min_liquidity_sol',
  'p_exit_trailing_stop_pct',
  'p_exit_time_stop_secs',
  'p_exit_stall_secs',
  'p_liquidity_drop_pct',
  // counts
  'n_fired',
  'n_closed',
  'n_open',
  // pnl
  'win_rate',
  'total_pnl_sol',
  'expectancy_sol',
  'profit_factor',
  'median_pnl_pct',
  'mean_pnl_pct',
  'p90_pnl_pct',
  'best_pnl_pct',
  'worst_pnl_pct',
  // holding
  'avg_holding_secs',
  'median_holding_secs',
  // exits
  'exit_take_profit',
  'exit_stop_loss',
  'exit_trailing',
  'exit_stall',
  'exit_time',
  'exit_liquidity',
  'exit_cohort',
  'exit_open',
];

/** Reorder built columns by `COLUMN_ORDER` (stable for unlisted keys). */
function orderColumns(cols: ColumnDef<SweepResultRecord>[]): ColumnDef<SweepResultRecord>[] {
  return [...cols].sort((a, b) => {
    const ia = COLUMN_ORDER.indexOf(a.key);
    const ib = COLUMN_ORDER.indexOf(b.key);
    return (ia === -1 ? Infinity : ia) - (ib === -1 ? Infinity : ib);
  });
}

/**
 * Build the sweep results table columns. Param columns are derived from the
 * run's swept param keys, so a different strategy's knobs render without
 * changing this file; the full set (params + metrics) is then ordered by
 * `COLUMN_ORDER`. Cell text is colored by group (params/counts/holding/exits)
 * and, on the PnL metrics, by meaning (green = good, red = bad).
 */
export function buildSweepColumns(paramKeys: string[]): ColumnDef<SweepResultRecord>[] {
  // Params are inputs, not results — a yellow accent sets the knob block apart
  // from the colored result metrics to its right.
  const paramCols: ColumnDef<SweepResultRecord>[] = paramKeys.map((k) => {
    // The TP/SL knobs echo their exit-column colors (green/red) so the ladder
    // reads at a glance; the rest of the knobs stay the neutral param accent.
    const cls = k === 'exit_take_profit' ? 'text-green' : k === 'exit_stop_loss' ? 'text-red' : 'text-secondary';
    return {
      key: `p_${k}`,
      label: k.replace(/_/g, ' '),
      group: 'params',
      sortable: true,
      render: (r) => tone(fmtParam(k, r.params[k]), cls),
      sortValue: (r) => r.params[k],
      filterNumber: (r) => r.params[k],
      searchValue: () => '',
    };
  });

  return orderColumns([
    ...paramCols,

    count('n_fired', 'Fired', 'counts', 'text-info', (r) => r.n_fired, {
      tooltip: 'Tokens this combo took a position on',
    }),
    count('n_closed', 'Closed', 'counts', 'text-info', (r) => r.n_closed, { defaultVisible: false }),
    count('n_open', 'Open', 'counts', 'text-info', (r) => r.n_open, { defaultVisible: false }),

    metric(
      'win_rate',
      'Win %',
      'pnl',
      (r) => r.win_rate,
      (r) => tone(`${(r.win_rate * 100).toFixed(0)}%`, goodBad(r.win_rate, 0.5)),
      { tooltip: 'Share of fired tokens with PnL > 0' },
    ),
    sol('total_pnl_sol', 'Total PnL', (r) => r.total_pnl_sol, {
      tooltip: 'Summed net PnL across all fired tokens (SOL)',
    }),
    sol('expectancy_sol', 'Expectancy', (r) => r.expectancy_sol, {
      tooltip: 'Mean PnL per trade (SOL)',
    }),
    metric(
      'profit_factor',
      'Profit factor',
      'pnl',
      (r) => r.profit_factor ?? Number.POSITIVE_INFINITY,
      (r) => tone(r.profit_factor == null ? '∞' : r.profit_factor.toFixed(2), goodBad(r.profit_factor ?? 10, 1)),
      { tooltip: 'Gross wins ÷ gross losses' },
    ),
    pct('median_pnl_pct', 'Median %', (r) => r.median_pnl_pct, { tooltip: 'Median per-trade return' }),
    pct('mean_pnl_pct', 'Mean %', (r) => r.mean_pnl_pct, { defaultVisible: false }),
    pct('p90_pnl_pct', 'P90 %', (r) => r.p90_pnl_pct, { defaultVisible: false }),
    pct('best_pnl_pct', 'Best %', (r) => r.best_pnl_pct, { defaultVisible: false }),
    pct('worst_pnl_pct', 'Worst %', (r) => r.worst_pnl_pct, { defaultVisible: false }),

    metric('avg_holding_secs', 'Avg hold', 'holding', (r) => r.avg_holding_secs, (r) =>
      tone(fmtSecs(r.avg_holding_secs), 'text-accent'),
    ),
    metric(
      'median_holding_secs',
      'Median hold',
      'holding',
      (r) => r.median_holding_secs,
      (r) => tone(fmtSecs(r.median_holding_secs), 'text-accent'),
      { defaultVisible: false },
    ),

    // Exits: TP/SL carry good/bad meaning (green/red); the rest are neutral.
    count('exit_take_profit', 'TP', 'exits', 'text-green', (r) => r.exit_take_profit, {
      tooltip: 'Exited on take-profit',
    }),
    count('exit_stop_loss', 'SL', 'exits', 'text-red', (r) => r.exit_stop_loss, { tooltip: 'Exited on stop-loss' }),
    count('exit_trailing', 'Trail', 'exits', 'text-text-mid', (r) => r.exit_trailing, {
      tooltip: 'Exited on trailing stop',
    }),
    count('exit_stall', 'Stall', 'exits', 'text-text-mid', (r) => r.exit_stall, { tooltip: 'Exited on stall' }),
    count('exit_time', 'Time', 'exits', 'text-text-mid', (r) => r.exit_time, { tooltip: 'Exited on time stop' }),
    count('exit_liquidity', 'Liq', 'exits', 'text-text-mid', (r) => r.exit_liquidity, { defaultVisible: false }),
    count('exit_cohort', 'Cohort', 'exits', 'text-text-mid', (r) => r.exit_cohort, { defaultVisible: false }),
    count('exit_open', 'Still open', 'exits', 'text-text-dim', (r) => r.exit_open, { defaultVisible: false }),
  ]);
}
