import { Fragment, type CSSProperties, type ReactNode } from 'react';
import type { ColumnDef } from 'components/table/types';
import { IxLabelsDisplay } from 'components/ui/IxLabelsDisplay';
import { ruleParamsCell } from 'components/strategy/RuleParamsSummary';
import { cn } from 'lib/cn';
import { formatDecimalTrim } from 'utils/format';
import { GROUP_FIELD_LABELS, type GroupField, type GroupedSweepGroupRecord } from './groupedTypes';
import type { SweepResultRecord } from './types';

// Generic-engine sweep columns (redesign FE5.2). The stat columns mirror the
// legacy `sweepColumns`/`groupColumns` shape, but the swept params are now a
// nested `RuleParams` blob (TP/SL + entry/exit metric conditions), rendered as
// compact chips instead of one flat column per knob. The legacy per-strategy
// exit-reason columns (trailing/stall/time/liquidity/next-kill) collapse to the
// engine's single `Metrics` exit.

// --- formatters -------------------------------------------------------------

export function fmtSecs(v: number | null | undefined): string {
  if (v == null || !Number.isFinite(v) || v <= 0) return '—';
  if (v < 90) return `${Math.round(v)}s`;
  if (v < 5400) return `${(v / 60).toFixed(1)}m`;
  return `${(v / 3600).toFixed(1)}h`;
}
export const pctText = (v: number | null | undefined) =>
  v == null || !Number.isFinite(v) ? '—' : `${v >= 0 ? '+' : ''}${formatDecimalTrim(v, 1)}%`;
export const solText = (v: number | null | undefined) =>
  v == null || !Number.isFinite(v) ? '—' : `◎${v >= 0 ? '+' : ''}${formatDecimalTrim(v, 4)}`;
const tone = (text: ReactNode, cls: string): ReactNode => (
  <span className={cn('font-medium', cls)}>{text}</span>
);
export const goodBad = (v: number | null | undefined, pivot = 0) =>
  v != null && Number.isFinite(v) && v >= pivot ? 'text-green' : 'text-red';
function chip(text: ReactNode, cls?: string, style?: CSSProperties): ReactNode {
  return (
    <span
      className={cn(
        'inline-block rounded border border-white/10 bg-surface px-1.5 py-0.5 font-mono text-[11px] leading-tight',
        cls,
      )}
      style={style}
    >
      {text}
    </span>
  );
}

/** The nested `params` blob a generic combo / group carries. */
interface RuleParamsJson {
  take_profit?: number | null;
  stop_loss?: number | null;
}

/** The `take_profit`/`stop_loss` numeric off `params` (for the sortable columns).
 *  Backend sort key `p_take_profit` → `(params->>'take_profit')::numeric`. */
function tpsl(raw: unknown, key: 'take_profit' | 'stop_loss'): number | null {
  const v = (raw as RuleParamsJson | undefined)?.[key];
  return typeof v === 'number' ? v : null;
}

// --- combo columns ----------------------------------------------------------

/** Build the generic-engine combo-results table columns. */
export function buildGenericComboColumns(): ColumnDef<SweepResultRecord>[] {
  return [
    {
      key: 'p_take_profit',
      label: 'TP',
      group: 'params',
      sortable: true,
      render: (r) => {
        const v = tpsl(r.params, 'take_profit');
        return tone(v == null ? '—' : `${formatDecimalTrim(v, 1)}%`, v == null ? 'text-text-dim' : 'text-green');
      },
      sortValue: (r) => tpsl(r.params, 'take_profit'),
      filterNumber: (r) => tpsl(r.params, 'take_profit'),
      searchValue: () => '',
    },
    {
      key: 'p_stop_loss',
      label: 'SL',
      group: 'params',
      sortable: true,
      render: (r) => {
        const v = tpsl(r.params, 'stop_loss');
        return tone(v == null ? '—' : `${formatDecimalTrim(v, 1)}%`, v == null ? 'text-text-dim' : 'text-red');
      },
      sortValue: (r) => tpsl(r.params, 'stop_loss'),
      filterNumber: (r) => tpsl(r.params, 'stop_loss'),
      searchValue: () => '',
    },
    {
      key: 'conditions',
      label: 'Conditions',
      group: 'params',
      sortable: false,
      render: (r) => ruleParamsCell(r.params),
      searchValue: () => '',
    },
    ...genericStatColumns(),
  ];
}

/** The stat/count/exit columns shared by the combo table (order mirrors the
 *  legacy sweep so the two read the same). */
function genericStatColumns(): ColumnDef<SweepResultRecord>[] {
  const metric = (
    key: string,
    label: string,
    group: string,
    value: (r: SweepResultRecord) => number,
    render: (r: SweepResultRecord) => ReactNode,
    opts: { tooltip?: string; defaultVisible?: boolean } = {},
  ): ColumnDef<SweepResultRecord> => ({
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
  });
  const count = (key: string, label: string, cls: string, value: (r: SweepResultRecord) => number, opts?: { defaultVisible?: boolean; tooltip?: string }) =>
    metric(key, label, 'exits', value, (r) => tone(String(value(r)), cls), opts);

  return [
    metric('n_fired', 'Fired', 'counts', (r) => r.n_fired, (r) => tone(String(r.n_fired), 'text-info'), {
      tooltip: 'Tokens this combo took a position on',
    }),
    // Closed/Open are visible by default: every realized stat in this table is
    // measured over `n_closed` alone, so the open count is the context that says
    // how much of `Fired` those stats actually speak for.
    metric('n_closed', 'Closed', 'counts', (r) => r.n_closed, (r) => tone(String(r.n_closed), 'text-info'), {
      tooltip: 'Positions that exited — the sample every realized stat is computed over',
    }),
    metric(
      'n_open',
      'Open',
      'counts',
      (r) => r.n_open,
      (r) => tone(String(r.n_open), r.n_open > 0 ? 'text-warning' : 'text-text-dim'),
      { tooltip: 'Still holding at the end of the corpus window — excluded from every realized stat' },
    ),
    metric(
      'open_share',
      'Open %',
      'counts',
      (r) => (r.n_fired > 0 ? r.n_open / r.n_fired : 0),
      (r) => {
        if (!r.n_fired) return tone('—', 'text-text-dim');
        const share = r.n_open / r.n_fired;
        // High open share = the realized headline rests on a thin closed sample.
        return tone(`${(share * 100).toFixed(0)}%`, share >= 0.5 ? 'text-red' : share >= 0.25 ? 'text-warning' : 'text-text-dim');
      },
      { tooltip: 'Share of fired positions still open. High = realized stats cover only a small slice of the sample.' },
    ),
    metric(
      'score',
      'Score',
      'pnl',
      (r) => r.score ?? Number.NEGATIVE_INFINITY,
      (r) => (r.score == null ? tone('—', 'text-text-dim') : tone(pctText(r.score), goodBad(r.score))),
      { tooltip: 'Robust rank: mean − 1.64·σ/√n over closed trades. Blank when < 2 closed trades.' },
    ),
    metric(
      'win_rate',
      'Win %',
      'pnl',
      (r) => r.win_rate,
      (r) =>
        r.win_rate == null || !Number.isFinite(r.win_rate)
          ? tone('—', 'text-text-dim')
          : tone(`${(r.win_rate * 100).toFixed(0)}%`, goodBad(r.win_rate, 0.5)),
      { tooltip: 'Share of fired tokens with PnL > 0' },
    ),
    metric('total_pnl_sol', 'Total PnL', 'pnl', (r) => r.total_pnl_sol, (r) => tone(solText(r.total_pnl_sol), goodBad(r.total_pnl_sol)), {
      tooltip: 'Realized only — sum of CLOSED positions. Still-open positions are excluded; see Open PnL.',
    }),
    metric(
      'open_pnl_sol',
      'Open PnL',
      'pnl',
      (r) => r.open_pnl_sol ?? 0,
      (r) => tone(solText(r.open_pnl_sol ?? 0), goodBad(r.open_pnl_sol ?? 0)),
      { tooltip: 'Unrealized: still-open positions marked to their last price. Not included in Total PnL.' },
    ),
    metric(
      'pnl_mtm_sol',
      'PnL (MTM)',
      'pnl',
      (r) => r.total_pnl_sol + (r.open_pnl_sol ?? 0),
      (r) => {
        const mtm = r.total_pnl_sol + (r.open_pnl_sol ?? 0);
        return tone(solText(mtm), goodBad(mtm));
      },
      {
        tooltip:
          'Mark-to-market: realized Total PnL + unrealized Open PnL. What the combo is actually worth if every open bag were sold at its last price.',
      },
    ),
    metric('expectancy_sol', 'Expectancy', 'pnl', (r) => r.expectancy_sol, (r) => tone(solText(r.expectancy_sol), goodBad(r.expectancy_sol))),
    metric(
      'profit_factor',
      'Profit factor',
      'pnl',
      (r) => r.profit_factor ?? Number.POSITIVE_INFINITY,
      (r) => tone(r.profit_factor == null ? '∞' : r.profit_factor.toFixed(2), goodBad(r.profit_factor ?? 10, 1)),
      { tooltip: 'Gross wins ÷ gross losses' },
    ),
    metric('median_pnl_pct', 'Median %', 'pnl', (r) => r.median_pnl_pct, (r) => tone(pctText(r.median_pnl_pct), goodBad(r.median_pnl_pct))),
    metric('mean_pnl_pct', 'Mean %', 'pnl', (r) => r.mean_pnl_pct, (r) => tone(pctText(r.mean_pnl_pct), goodBad(r.mean_pnl_pct)), { defaultVisible: false }),
    metric('avg_holding_secs', 'Avg hold', 'holding', (r) => r.avg_holding_secs, (r) => tone(fmtSecs(r.avg_holding_secs), 'text-accent')),
    count('n_exit_take_profit', 'TP', 'text-green', (r) => r.n_exit_take_profit, { tooltip: 'Exited on take-profit' }),
    count('n_exit_stop_loss', 'SL', 'text-red', (r) => r.n_exit_stop_loss, { tooltip: 'Exited on stop-loss' }),
    count('n_exit_metrics', 'Metrics', 'text-text-mid', (r) => r.n_exit_metrics ?? 0, {
      tooltip: 'Exited because any exit metric condition became true',
    }),
    count('n_exit_dead', 'Dead', 'text-red', (r) => r.n_exit_dead, {
      tooltip: 'Analysis death-close: token died silent, booked at the last meaningful trade',
    }),
    count('n_exit_open', 'Still open', 'text-text-dim', (r) => r.n_exit_open, { defaultVisible: false }),
  ];
}

// --- group columns ----------------------------------------------------------

/** A human label + value string for one group-key field (chips + search). */
function keyParts(group: GroupedSweepGroupRecord): { label: string; value: string }[] {
  return Object.entries(group.group_key).map(([k, v]) => ({
    label: GROUP_FIELD_LABELS[k as GroupField] ?? k,
    value: v,
  }));
}

/** Build the generic-engine group-summary table columns. */
export function buildGenericGroupColumns(): ColumnDef<GroupedSweepGroupRecord>[] {
  // `group` mirrors the combo table's banners (counts / pnl / holding) so the
  // two stacked tables on the sweep page read the same. Columns must stay in
  // group order — DataTable bands *consecutive* runs of same-`group` columns.
  const gm = (
    key: string,
    label: string,
    group: 'counts' | 'pnl' | 'holding',
    value: (g: GroupedSweepGroupRecord) => number | null,
    render: (g: GroupedSweepGroupRecord) => ReactNode,
    opts: { tooltip?: string; defaultVisible?: boolean } = {},
  ): ColumnDef<GroupedSweepGroupRecord> => ({
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
  });

  return [
    {
      key: 'group_key',
      label: 'Group (fingerprint)',
      group: 'group',
      sortable: false,
      render: (g) => {
        const parts = keyParts(g);
        if (parts.length === 0) return chip('ALL tokens', 'text-text-dim');
        return (
          <div className="grid grid-cols-[auto_1fr] items-start gap-x-3 gap-y-1 text-left">
            {parts.map((p) => {
              const isIxLabels = p.label === GROUP_FIELD_LABELS.ix_labels;
              const ixParts = isIxLabels ? p.value.split(' | ') : null;
              return (
                <Fragment key={p.label}>
                  <span className="text-[11px] leading-tight text-text-dim" title={`${p.label}: ${p.value}`}>
                    {p.label}:
                  </span>
                  {ixParts ? (
                    <IxLabelsDisplay labels={ixParts} className="text-secondary" />
                  ) : (
                    <span>{chip(p.value, 'text-secondary')}</span>
                  )}
                </Fragment>
              );
            })}
          </div>
        );
      },
      searchValue: (g) => keyParts(g).map((p) => `${p.label} ${p.value}`).join(' '),
    },
    gm('token_count', 'Tokens', 'counts', (g) => g.token_count, (g) => tone(String(g.token_count), 'text-info'), {
      tooltip: 'Tokens in this fingerprint group',
    }),
    gm('fired_count', 'Fired', 'counts', (g) => g.fired_count, (g) => tone(String(g.fired_count), 'text-info'), {
      tooltip: "The best combo's fired count — the sample size behind its score",
    }),
    gm(
      'best_n_closed',
      'Closed',
      'counts',
      (g) => g.best_n_closed ?? 0,
      (g) => tone(String(g.best_n_closed ?? 0), 'text-info'),
      { tooltip: 'Winning combo positions that exited — the sample every realized stat on this row is computed over' },
    ),
    gm(
      'best_n_open',
      'Open',
      'counts',
      (g) => g.best_n_open ?? 0,
      (g) => tone(String(g.best_n_open ?? 0), (g.best_n_open ?? 0) > 0 ? 'text-warning' : 'text-text-dim'),
      { tooltip: 'Winning combo positions still open — excluded from every realized stat on this row' },
    ),
    gm(
      'best_open_share',
      'Open %',
      'counts',
      (g) => (g.fired_count > 0 ? (g.best_n_open ?? 0) / g.fired_count : 0),
      (g) => {
        if (!g.fired_count) return tone('—', 'text-text-dim');
        const share = (g.best_n_open ?? 0) / g.fired_count;
        return tone(`${(share * 100).toFixed(0)}%`, share >= 0.5 ? 'text-red' : share >= 0.25 ? 'text-warning' : 'text-text-dim');
      },
      { tooltip: 'Share of the winning combo’s fired positions still open. High = the realized headline rests on a thin closed sample.' },
    ),
    gm(
      'best_score',
      'Score',
      'pnl',
      (g) => g.best_score,
      (g) =>
        g.best_score == null
          ? tone('—', 'text-text-dim')
          : tone(pctText(g.best_score), goodBad(g.best_score)),
      { tooltip: "Robust score of this group's best combo (matches the drill-in default sort)." },
    ),
    gm('best_win_rate', 'Win %', 'pnl', (g) => g.best_win_rate, (g) =>
      g.best_win_rate == null || !Number.isFinite(g.best_win_rate)
        ? tone('—', 'text-text-dim')
        : tone(`${(g.best_win_rate * 100).toFixed(0)}%`, goodBad(g.best_win_rate, 0.5)),
    ),
    gm('best_total_pnl_sol', 'Total PnL', 'pnl', (g) => g.best_total_pnl_sol, (g) => tone(solText(g.best_total_pnl_sol), goodBad(g.best_total_pnl_sol)), {
      tooltip: 'Realized only — sum of CLOSED positions. Still-open positions are excluded; see Open PnL.',
    }),
    gm(
      'best_open_pnl_sol',
      'Open PnL',
      'pnl',
      (g) => g.best_open_pnl_sol ?? 0,
      (g) => tone(solText(g.best_open_pnl_sol ?? 0), goodBad(g.best_open_pnl_sol ?? 0)),
      { tooltip: 'Unrealized: still-open positions marked to their last price. Not included in Total PnL.' },
    ),
    gm(
      'best_pnl_mtm_sol',
      'PnL (MTM)',
      'pnl',
      (g) => g.best_total_pnl_sol + (g.best_open_pnl_sol ?? 0),
      (g) => {
        const mtm = g.best_total_pnl_sol + (g.best_open_pnl_sol ?? 0);
        return tone(solText(mtm), goodBad(mtm));
      },
      {
        tooltip:
          'Mark-to-market: realized Total PnL + unrealized Open PnL. What the group is actually worth if every open bag were sold at its last price.',
      },
    ),
    gm('best_expectancy_sol', 'Expectancy', 'pnl', (g) => g.best_expectancy_sol, (g) => tone(solText(g.best_expectancy_sol), goodBad(g.best_expectancy_sol))),
    gm('best_profit_factor', 'Profit factor', 'pnl', (g) => g.best_profit_factor ?? Number.POSITIVE_INFINITY, (g) =>
      tone(g.best_profit_factor == null ? '∞' : g.best_profit_factor.toFixed(2), goodBad(g.best_profit_factor ?? 10, 1)),
    ),
    gm('best_median_pnl_pct', 'Median %', 'pnl', (g) => g.best_median_pnl_pct, (g) => tone(pctText(g.best_median_pnl_pct), goodBad(g.best_median_pnl_pct))),
    gm('best_avg_holding_secs', 'Avg hold', 'holding', (g) => g.best_avg_holding_secs, (g) => tone(fmtSecs(g.best_avg_holding_secs), 'text-accent')),
    {
      key: 'best_params',
      label: "Best combo's rule",
      group: 'params',
      sortable: false,
      render: (g) => ruleParamsCell(g.best_params),
      searchValue: () => '',
    },
  ];
}
