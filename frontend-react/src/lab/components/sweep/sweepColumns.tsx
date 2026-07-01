import type { ReactNode } from 'react';
import type { ColumnDef } from 'components/table/types';
import { cn } from 'lib/cn';
import { formatDecimalTrim } from 'utils/format';
import type { ParamColumnColor } from 'lib/sweepParamColors';
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
 * Build the sweep results table columns. Like `tokenColumns`, this is just a
 * plain ordered array: column order = array order, grouping = each column's
 * `group` field. To reorder/regroup, move entries (metrics here, params in the
 * page's `*_PARAM_KEYS` list) or change a `group` — no separate order list.
 *
 * Param columns are derived from the run's swept param keys (so a different
 * strategy's knobs render without changing this file) and keep `paramKeys`
 * order; cell text is colored by group (entry/exit knobs, counts, pnl, holding,
 * exits) and, on the PnL metrics, by meaning (green = good, red = bad).
 *
 * `pnlColors` applies the same per-value cell-band tint to the PnL metric
 * columns that `paramColors` applies to param columns — rows sharing an
 * identical metric value get the same background so clusters read at a glance.
 */
export function buildSweepColumns(
  paramKeys: string[],
  paramColors?: Map<string, ParamColumnColor>,
  pnlColors?: Map<string, ParamColumnColor>,
): ColumnDef<SweepResultRecord>[] {
  // One column per swept knob, in `paramKeys` order (the page owns that order).
  const paramCols: ColumnDef<SweepResultRecord>[] = paramKeys.map((k) => {
    // The TP/SL knobs echo their exit-column colors (green/red) so the ladder
    // reads at a glance; the rest of the knobs stay the neutral param accent.
    const cls = k === 'exit_take_profit' ? 'text-green' : k === 'exit_stop_loss' ? 'text-red' : 'text-secondary';
    // Group by trade side so entry gates and exit knobs tint as separate bands.
    const group = k.startsWith('entry_') ? 'entry' : 'exit';
    // Per-column tint plan (when supplied): a knob that's constant across the
    // group dims out so the eye skips it; a varying knob keeps its accent and
    // gets a per-value full-cell background (via `cellClassName`) so equal values
    // read as a color band down the column.
    const color = paramColors?.get(k);
    return {
      key: `p_${k}`,
      label: k.replace(/_/g, ' '),
      group,
      sortable: true,
      render: (r) => tone(fmtParam(k, r.params[k]), color?.constant ? 'text-text-dim' : cls),
      cellClassName:
        color && !color.constant
          ? (r) => {
              const v = r.params[k];
              return v == null ? undefined : color.byValue.get(v);
            }
          : undefined,
      sortValue: (r) => r.params[k],
      filterNumber: (r) => r.params[k],
      searchValue: () => '',
    };
  });

  const cols: ColumnDef<SweepResultRecord>[] = [
    ...paramCols,

    count('n_fired', 'Fired', 'counts', 'text-info', (r) => r.n_fired, {
      tooltip: 'Tokens this combo took a position on',
    }),
    count('n_closed', 'Closed', 'counts', 'text-info', (r) => r.n_closed, { defaultVisible: false }),
    count('n_open', 'Open', 'counts', 'text-info', (r) => r.n_open, { defaultVisible: false }),

    // Headline rank: robust per-trade edge (μ − z·σ/√n over closed trades). Null
    // (fewer than 2 closed trades) renders '—' and sinks to the bottom of the
    // default desc sort. Scored in pnl% units, so green/red like the other %s.
    metric(
      'score',
      'Score',
      'pnl',
      (r) => r.score ?? Number.NEGATIVE_INFINITY,
      (r) => (r.score == null ? tone('—', 'text-text-dim') : tone(pctText(r.score), goodBad(r.score))),
      {
        tooltip:
          'Robust rank: mean − 1.64·σ/√n over closed trades (lower-confidence edge). ' +
          'Rewards a high, consistent per-trade return; penalizes variance and small samples. ' +
          'Blank when fewer than 2 closed trades.',
      },
    ),
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
    metric(
      'std_pnl_pct',
      'Std %',
      'pnl',
      (r) => r.std_pnl_pct,
      (r) => tone(`${formatDecimalTrim(r.std_pnl_pct, 1)}%`, 'text-text-mid'),
      { tooltip: 'Stddev of realized per-trade return — the dispersion/risk term in Score', defaultVisible: false },
    ),

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

    // Exit-reason counts (how closed trades terminated): TP/SL carry good/bad
    // meaning (green/red); the rest are neutral. These are counts, not the
    // exit_take_profit/exit_stop_loss *threshold* knobs rendered in the params.
    count('n_exit_take_profit', 'TP', 'exits', 'text-green', (r) => r.n_exit_take_profit, {
      tooltip: 'Exited on take-profit',
    }),
    count('n_exit_stop_loss', 'SL', 'exits', 'text-red', (r) => r.n_exit_stop_loss, { tooltip: 'Exited on stop-loss' }),
    count('n_exit_trailing', 'Trail', 'exits', 'text-text-mid', (r) => r.n_exit_trailing, {
      tooltip: 'Exited on trailing stop',
    }),
    count('n_exit_stall', 'Stall', 'exits', 'text-text-mid', (r) => r.n_exit_stall, { tooltip: 'Exited on stall' }),
    count('n_exit_time', 'Time', 'exits', 'text-text-mid', (r) => r.n_exit_time, { tooltip: 'Exited on time stop' }),
    count('n_exit_liquidity', 'Liq', 'exits', 'text-text-mid', (r) => r.n_exit_liquidity, { defaultVisible: false }),
    count('n_exit_cohort', 'Cohort', 'exits', 'text-text-mid', (r) => r.n_exit_cohort, { defaultVisible: false }),
    count('n_exit_next_kill', 'NextKill', 'exits', 'text-text-mid', (r) => r.n_exit_next_kill, {
      tooltip: 'Exited on swing1 symmetric next-kill flee',
    }),
    count('n_exit_open', 'Still open', 'exits', 'text-text-dim', (r) => r.n_exit_open, { defaultVisible: false }),
  ];

  // Apply per-value cell tints to pnl-group columns (same palette mechanism as
  // param columns). Column key === SweepResultRecord field name so `unknown` cast
  // gives safe indexed access. Rows sharing an identical metric value get the same
  // background band so clusters of equivalent combos are visible at a glance.
  if (!pnlColors) return cols;
  return cols.map((col) => {
    if (col.group !== 'pnl') return col;
    const color = pnlColors.get(col.key);
    if (!color || color.constant) return col;
    const { byValue } = color;
    return {
      ...col,
      cellClassName: (r: SweepResultRecord) => {
        const v = (r as unknown as Record<string, number | null>)[col.key];
        return v == null ? undefined : byValue.get(v);
      },
    };
  });
}
