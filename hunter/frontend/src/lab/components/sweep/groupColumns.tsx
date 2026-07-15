import { Fragment, type ReactNode } from 'react';
import type { ColumnDef } from 'components/table/types';
import { cn } from 'lib/cn';
import { formatDecimalTrim } from 'utils/format';
import { GROUP_FIELD_LABELS, type GroupField, type GroupedSweepGroupRecord } from './groupedTypes';

// --- formatters (shared shape with sweepColumns, kept local to stay decoupled) ---

function fmtSecs(v: number): string {
  if (v <= 0) return '—';
  if (v < 90) return `${Math.round(v)}s`;
  if (v < 5400) return `${(v / 60).toFixed(1)}m`;
  return `${(v / 3600).toFixed(1)}h`;
}

/** Format a swept param by the unit its key implies (mirrors sweepColumns). */
function fmtParam(key: string, v: number | null): string {
  if (v == null) return '—';
  if (key.endsWith('_secs')) return fmtSecs(v);
  if (key.endsWith('_sol')) return `◎${formatDecimalTrim(v, 4)}`;
  if (key.endsWith('_pct') || key === 'exit_take_profit' || key === 'exit_stop_loss') {
    return `${formatDecimalTrim(v, 1)}%`;
  }
  return formatDecimalTrim(v, 4);
}

function solText(v: number) {
  return `◎${v >= 0 ? '+' : ''}${formatDecimalTrim(v, 4)}`;
}

/** Signed percent, 1 decimal (mirrors sweepColumns `pctText`) — `+20.6%`. */
function pctText(v: number) {
  return `${v >= 0 ? '+' : ''}${formatDecimalTrim(v, 1)}%`;
}

/** Wrap cell text in a colored span (mirrors sweepColumns `tone`). */
function tone(text: ReactNode, cls: string): ReactNode {
  return <span className={cn('font-medium', cls)}>{text}</span>;
}

const goodBad = (v: number, pivot = 0) => (v >= pivot ? 'text-green' : 'text-red');

function chip(text: ReactNode, cls?: string): ReactNode {
  return (
    <span
      className={cn(
        'inline-block rounded border border-white/10 bg-surface px-1.5 py-0.5 font-mono text-[11px] leading-tight',
        cls,
      )}
    >
      {text}
    </span>
  );
}

/** A human label + value string for one group-key field (used in chips + search). */
function keyParts(group: GroupedSweepGroupRecord): { label: string; value: string }[] {
  return Object.entries(group.group_key).map(([k, v]) => ({
    label: GROUP_FIELD_LABELS[k as GroupField] ?? k,
    value: v,
  }));
}

// --- param columns ----------------------------------------------------------

/**
 * Master left-to-right order for the param columns: entry gates first (mirroring
 * the TPSL2 rule modal), then the exit knobs (TP/SL lead as the headline pair).
 * The `entry`/`exit` column `group` then draws the divider + tint between the two
 * blocks; the spanning group-header banner labels them. Keys not listed fall in
 * after, keeping build order.
 */
const PARAM_ORDER = [
  'entry_min_age_secs',
  'entry_max_age_secs',
  'entry_min_alive_sol',
  'entry_min_net_buy_sol',
  'entry_pullback_pct',
  'entry_higher_low_secs',
  'entry_min_liquidity_sol',
  'exit_take_profit',
  'exit_stop_loss',
  'exit_trailing_stop_pct',
  'exit_time_stop_secs',
  'exit_stall_secs',
  'exit_liquidity_drop_pct',
];

/** High-leverage knobs shown by default; the rest hide behind the Columns toggle
 *  so the table stays scannable until you opt a knob in. Mirrors the knobs that
 *  ship a real candidate grid in `TPSL2_AXES` (the rest default to off). */
const DEFAULT_VISIBLE_PARAMS = new Set([
  'exit_take_profit',
  'exit_stop_loss',
  'exit_trailing_stop_pct',
  'exit_time_stop_secs',
  'exit_stall_secs',
  'entry_min_age_secs',
  'entry_pullback_pct',
  'entry_min_liquidity_sol',
]);

/** One sortable + numerically-filterable column for a single swept param,
 *  reading the winning combo's value from `best_params`. */
function paramColumn(k: string): ColumnDef<GroupedSweepGroupRecord> {
  const side = k.startsWith('entry_') ? 'entry' : 'exit';
  // TP/SL echo their good/bad colors; off → dim; everything else the param accent.
  const cls =
    k === 'exit_take_profit'
      ? 'text-green'
      : k === 'exit_stop_loss'
        ? 'text-red'
        : 'text-secondary';
  return {
    key: `p_${k}`,
    label: k.replace(/^(exit|entry)_/, '').replace(/_/g, ' '),
    group: side,
    sortable: true,
    defaultVisible: DEFAULT_VISIBLE_PARAMS.has(k),
    render: (g) => {
      const v = g.best_params[k];
      return tone(fmtParam(k, v), v == null ? 'text-text-dim' : cls);
    },
    sortValue: (g) => g.best_params[k],
    filterNumber: (g) => g.best_params[k],
    searchValue: () => '',
  };
}

/** Reorder param columns by `PARAM_ORDER` (stable for unlisted keys). */
function orderParams(keys: string[]): string[] {
  return [...keys].sort((a, b) => {
    const ia = PARAM_ORDER.indexOf(a);
    const ib = PARAM_ORDER.indexOf(b);
    return (ia === -1 ? Infinity : ia) - (ib === -1 ? Infinity : ib);
  });
}

/**
 * Columns for the group-summary table (group list → drill into combos). The
 * fingerprint key renders as labeled chips; the per-group metrics and the winning
 * combo's params are each their own sortable + numerically-filterable column,
 * with the params grouped into Entry / Exit blocks (divider + tint + spanning
 * banner). `paramKeys` is the run's swept knobs — the same list the drill-in
 * `buildSweepColumns` receives, so the two tables stay consistent.
 */
export function buildGroupColumns(paramKeys: string[]): ColumnDef<GroupedSweepGroupRecord>[] {
  return [
    {
      key: 'group_key',
      label: 'Group (fingerprint)',
      group: 'group',
      sortable: false,
      render: (g) => {
        const parts = keyParts(g);
        if (parts.length === 0) return chip('ALL tokens', 'text-text-dim');
        // Label/value grid: field labels in the left lane, values in the right.
        // For `ix_labels` (a `" | "`-joined list) each child instruction stacks as
        // its own value row in the right lane so they're scannable at a glance.
        return (
          <div className="grid grid-cols-[auto_1fr] items-start gap-x-3 gap-y-1 text-left">
            {parts.map((p) => {
              const isIxLabels = p.label === GROUP_FIELD_LABELS.ix_labels;
              const ixParts = isIxLabels ? p.value.split(' | ') : null;
              return (
                <Fragment key={p.label}>
                  <span
                    className="text-[11px] leading-tight text-text-dim"
                    title={`${p.label}: ${p.value}`}
                  >
                    {p.label}:
                  </span>
                  {ixParts ? (
                    <pre className="select-all whitespace-pre font-mono text-[11px] leading-relaxed text-secondary">
                      {JSON.stringify(ixParts, null, 2)}
                    </pre>
                  ) : (
                    <span>{chip(p.value, 'text-secondary')}</span>
                  )}
                </Fragment>
              );
            })}
          </div>
        );
      },
      searchValue: (g) =>
        keyParts(g)
          .map((p) => `${p.label} ${p.value}`)
          .join(' '),
    },

    {
      key: 'token_count',
      label: 'Tokens',
      group: 'metrics',
      tooltip: 'Tokens in this fingerprint group',
      sortable: true,
      render: (g) => tone(String(g.token_count), 'text-info'),
      sortValue: (g) => g.token_count,
      filterNumber: (g) => g.token_count,
      searchValue: () => '',
    },
    {
      key: 'fired_count',
      label: 'Fired',
      group: 'metrics',
      tooltip: "The best combo's fired count — the sample size behind its expectancy",
      sortable: true,
      render: (g) => tone(String(g.fired_count), 'text-info'),
      sortValue: (g) => g.fired_count,
      filterNumber: (g) => g.fired_count,
      searchValue: () => '',
    },
    // The winning combo's full stat line — same metrics (and formatting) as the
    // drill-in "Combos for group" table, so the group row reads as a complete
    // readout of its crowned combo. Order mirrors that table; the low-signal
    // columns (Mean/P90/Std %, Median hold) start hidden behind the Columns
    // toggle, matching the combos table.
    {
      key: 'best_score',
      label: 'Score',
      group: 'metrics',
      tooltip:
        'Robust realized score (μ−1.64·σ/√n over closed trades, in PnL %) of this group’s best combo — the ranking metric (matches the drill-in table’s default sort). Blank when < 2 closed trades.',
      sortable: true,
      render: (g) =>
        g.best_score == null
          ? tone('—', 'text-text-dim')
          : tone(pctText(g.best_score), goodBad(g.best_score)),
      sortValue: (g) => g.best_score,
      filterNumber: (g) => g.best_score,
      searchValue: () => '',
    },
    {
      key: 'best_win_rate',
      label: 'Win %',
      group: 'metrics',
      tooltip: 'Share of the best combo’s fired tokens with PnL > 0',
      sortable: true,
      render: (g) => tone(`${(g.best_win_rate * 100).toFixed(0)}%`, goodBad(g.best_win_rate, 0.5)),
      sortValue: (g) => g.best_win_rate,
      filterNumber: (g) => g.best_win_rate,
      searchValue: () => '',
    },
    {
      key: 'best_total_pnl_sol',
      label: 'Total PnL',
      group: 'metrics',
      tooltip: 'Summed net PnL across all of the best combo’s fired tokens (SOL)',
      sortable: true,
      render: (g) => tone(solText(g.best_total_pnl_sol), goodBad(g.best_total_pnl_sol)),
      sortValue: (g) => g.best_total_pnl_sol,
      filterNumber: (g) => g.best_total_pnl_sol,
      searchValue: () => '',
    },
    {
      key: 'best_expectancy_sol',
      label: 'Expectancy',
      group: 'metrics',
      tooltip: 'Mean net PnL per trade (SOL) of this group’s best combo',
      sortable: true,
      render: (g) => tone(solText(g.best_expectancy_sol), goodBad(g.best_expectancy_sol)),
      sortValue: (g) => g.best_expectancy_sol,
      filterNumber: (g) => g.best_expectancy_sol,
      searchValue: () => '',
    },
    {
      key: 'best_profit_factor',
      label: 'Profit factor',
      group: 'metrics',
      tooltip: 'Gross wins ÷ gross losses for the best combo (∞ = no losing trades)',
      sortable: true,
      render: (g) =>
        tone(
          g.best_profit_factor == null ? '∞' : g.best_profit_factor.toFixed(2),
          goodBad(g.best_profit_factor ?? 10, 1),
        ),
      sortValue: (g) => g.best_profit_factor ?? Number.POSITIVE_INFINITY,
      filterNumber: (g) => g.best_profit_factor ?? Number.POSITIVE_INFINITY,
      searchValue: () => '',
    },
    {
      key: 'best_median_pnl_pct',
      label: 'Median %',
      group: 'metrics',
      tooltip: 'Median per-trade return of the best combo',
      sortable: true,
      render: (g) => tone(pctText(g.best_median_pnl_pct), goodBad(g.best_median_pnl_pct)),
      sortValue: (g) => g.best_median_pnl_pct,
      filterNumber: (g) => g.best_median_pnl_pct,
      searchValue: () => '',
    },
    {
      key: 'best_mean_pnl_pct',
      label: 'Mean %',
      group: 'metrics',
      defaultVisible: false,
      tooltip: 'Mean per-trade return of the best combo',
      sortable: true,
      render: (g) => tone(pctText(g.best_mean_pnl_pct), goodBad(g.best_mean_pnl_pct)),
      sortValue: (g) => g.best_mean_pnl_pct,
      filterNumber: (g) => g.best_mean_pnl_pct,
      searchValue: () => '',
    },
    {
      key: 'best_p90_pnl_pct',
      label: 'P90 %',
      group: 'metrics',
      defaultVisible: false,
      tooltip: '90th-percentile per-trade return of the best combo',
      sortable: true,
      render: (g) => tone(pctText(g.best_p90_pnl_pct), goodBad(g.best_p90_pnl_pct)),
      sortValue: (g) => g.best_p90_pnl_pct,
      filterNumber: (g) => g.best_p90_pnl_pct,
      searchValue: () => '',
    },
    {
      key: 'best_std_pnl_pct',
      label: 'Std %',
      group: 'metrics',
      defaultVisible: false,
      tooltip: 'Stddev of the best combo’s per-trade return — the dispersion/risk term in Score',
      sortable: true,
      render: (g) => tone(`${formatDecimalTrim(g.best_std_pnl_pct, 1)}%`, 'text-text-mid'),
      sortValue: (g) => g.best_std_pnl_pct,
      filterNumber: (g) => g.best_std_pnl_pct,
      searchValue: () => '',
    },
    {
      key: 'best_avg_holding_secs',
      label: 'Avg hold',
      group: 'metrics',
      tooltip: 'Mean holding time of the best combo’s closed trades',
      sortable: true,
      render: (g) => tone(fmtSecs(g.best_avg_holding_secs), 'text-accent'),
      sortValue: (g) => g.best_avg_holding_secs,
      filterNumber: (g) => g.best_avg_holding_secs,
      searchValue: () => '',
    },
    {
      key: 'best_median_holding_secs',
      label: 'Median hold',
      group: 'metrics',
      defaultVisible: false,
      tooltip: 'Median holding time of the best combo’s closed trades',
      sortable: true,
      render: (g) => tone(fmtSecs(g.best_median_holding_secs), 'text-accent'),
      sortValue: (g) => g.best_median_holding_secs,
      filterNumber: (g) => g.best_median_holding_secs,
      searchValue: () => '',
    },

    ...orderParams(paramKeys).map(paramColumn),
  ];
}
