import { Fragment, type CSSProperties, type ReactNode } from 'react';
import { Link } from 'react-router-dom';

import type { ColumnDef } from 'components/table/types';
import { Badge } from 'components/ui/Badge';
import { IxLabelsDisplay } from 'components/ui/IxLabelsDisplay';
import { LinkIcon } from 'components/ui/icons';
import { ModeBadge } from 'components/strategy/ModeBadge';
import { ruleParamsCell } from 'components/strategy/RuleParamsSummary';
import { RuleHoverTip } from 'components/strategy/RuleHoverTip';
import { cn } from 'lib/cn';
import { fingerprintsHref, rulesHref } from 'lib/strategy/nav';
import { ruleParamsJsonEqual } from 'lib/strategy/ruleParams';
import type { Fingerprint, StrategyRule } from 'lib/strategy/types';
import { formatDecimalTrim } from 'utils/format';
import type { ExitMetricLegendEntry } from '@lab/hooks/useStreamedSweepResults';
import { GROUP_FIELD_LABELS, type GroupField, type GroupedSweepGroupRecord } from './groupedTypes';
import type { SweepResultRecord } from './types';

// Generic-engine sweep columns (redesign FE5.2). The stat columns mirror the
// legacy `sweepColumns`/`groupColumns` shape, but the swept params are now a
// nested `RuleParams` blob (TP/SL + entry/exit metric conditions), rendered as
// compact chips instead of one flat column per knob. The legacy per-strategy
// exit-reason columns (trailing/stall/time/liquidity/next-kill) collapse to the
// engine's single `Metrics` exit.

// --- formatters -------------------------------------------------------------

// Single-sourced with the shared run-summary renderer, so a metric reads the same
// in a sweep table cell as it does in any summary panel (parity plan F4-F8).
// Re-exported because this module is the established import site for them.
export { fmtSecs, pctText, solText, goodBad } from 'lib/strategy/runSummary';
import { fmtSecs, pctText, solText, goodBad } from 'lib/strategy/runSummary';
import { pctGradeClass, winRateGradeClass } from 'lib/signedTone';

const tone = (text: ReactNode, cls: string): ReactNode => (
  <span className={cn('font-medium', cls)}>{text}</span>
);

/** Mark-to-market SOL: realized total + unrealized open (shared by combo/group cols). */
function mtmPnlSol(totalPnlSol: number, openPnlSol: number | null | undefined): number {
  return totalPnlSol + (openPnlSol ?? 0);
}
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

/** Persisted sweep quantiles come from a 64-bucket DDSketch (~15% rel. error) — the
 *  sweep is an approximate *ranking* tool. Ranking itself is unaffected (`score` is
 *  exact); only the displayed value is sketched. Single-sourced so every sketched
 *  column says the same thing, and prefixed `≈` so the gap is visible without hover.
 *  See docs/plans/sweep/sim-parity.md (D3). */
const SKETCHED_QUANTILE_TOOLTIP =
  'Approximate (~15%): sketched from a 64-bucket DDSketch, not computed exactly. ' +
  'Ranking is unaffected — open the combo drill-in or Simulate for an exact median.';

/** Above this share of still-open positions the realized headline (Total PnL, Win%)
 *  rests on a thin closed sample. A sweep is a **screener, not a backtest** — its
 *  numbers are uncapped per-token-tail estimates (parity plan D1/D2) — so flag the
 *  realized PnL for re-simulation. Matches the Open% column's warning tint threshold. */
const SCREENER_OPEN_SHARE = 0.25;
const SCREENER_ESTIMATE_TOOLTIP =
  'Screener estimate — re-simulate for PnL. A large share of fired positions are still ' +
  'open, so this realized total is computed over a thin closed sample; the sweep is also ' +
  'uncapped vs a live rule (both optimistic). Promote + Simulate the combo for a trustworthy PnL. ' +
  'Check “Data through” on the run above first: a stale lake export freezes these opens at ' +
  'old prices that a Simulate (which splices the fresh PG tail) will close.';

/** Realized-PnL cell with an "est" flag when the open share is high — a screener
 *  estimate to re-simulate. One renderer shared by the combo + group Total PnL cells. */
function pnlCellWithScreenerFlag(pnlSol: number, openShare: number | null): ReactNode {
  const cell = tone(solText(pnlSol), goodBad(pnlSol));
  if (openShare == null || openShare < SCREENER_OPEN_SHARE) return cell;
  return (
    <span className="inline-flex items-center gap-1">
      {cell}
      <Badge variant="warning" size="sm" title={SCREENER_ESTIMATE_TOOLTIP}>
        est
      </Badge>
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

/** Per-trade MTM % from SOL totals + run notional (opens included). */
function mtmPctOf(totalPnl: number, openPnl: number, nFired: number, buySol: number): number | null {
  if (!(nFired > 0) || !(buySol > 0)) return null;
  return ((totalPnl + openPnl) / (buySol * nFired)) * 100;
}

/** Build the generic-engine combo-results table columns. */

export function buildGenericComboColumns(
  buyAmountSol = 1,
  exitMetricLegend: ExitMetricLegendEntry[] = [],
): ColumnDef<SweepResultRecord>[] {
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
    ...genericStatColumns(buyAmountSol, exitMetricLegend),
  ];
}

/** The stat/count/exit columns shared by the combo table (order mirrors the
 *  legacy sweep so the two read the same). */
function genericStatColumns(
  buyAmountSol: number,
  exitMetricLegend: ExitMetricLegendEntry[],
): ColumnDef<SweepResultRecord>[] {
  const metric = (
    key: string,
    label: string,
    group: string,
    value: (r: SweepResultRecord) => number | null,
    render: (r: SweepResultRecord) => ReactNode,
    opts: {
      tooltip?: string;
      defaultVisible?: boolean;
      /** Scale the wire value into displayed units for filter/search (e.g. fraction → %). */
      displayUnits?: (n: number) => number;
    } = {},
  ): ColumnDef<SweepResultRecord> => {
    const units = opts.displayUnits ?? ((n: number) => n);
    return {
      key,
      label,
      group,
      tooltip: opts.tooltip,
      defaultVisible: opts.defaultVisible,
      sortable: true,
      render,
      // Sort stays on the wire value (order is identical under a positive scale).
      sortValue: value,
      filterNumber: (r) => {
        const v = value(r);
        return v == null || !Number.isFinite(v) ? null : units(v);
      },
      searchValue: () => '',
    };
  };
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
      (r) => (r.n_fired > 0 ? r.n_open / r.n_fired : null),
      (r) => {
        if (!r.n_fired) return tone('—', 'text-text-dim');
        const share = r.n_open / r.n_fired;
        // High open share = the realized headline rests on a thin closed sample.
        return tone(`${(share * 100).toFixed(0)}%`, share >= 0.5 ? 'text-red' : share >= 0.25 ? 'text-warning' : 'text-text-dim');
      },
      {
        tooltip: 'Share of fired positions still open. High = realized stats cover only a small slice of the sample.',
        // Display is percent points; wire value is a 0..1 fraction.
        displayUnits: (n) => n * 100,
      },
    ),
    metric(
      'score',
      'Score',
      'pnl',
      (r) => r.score ?? Number.NEGATIVE_INFINITY,
      (r) =>
        r.score == null
          ? tone('—', 'text-text-dim')
          : tone(
              `${r.score >= 0 ? '+' : ''}${formatDecimalTrim(r.score, 4)}`,
              goodBad(r.score),
            ),
      {
        tooltip:
          'MTM% × (fired/matched) × (1 − 0.5·open%) × win%. Matches the manual checklist. Blank when never fired.',
      },
    ),
    metric(
      'win_rate',
      'Win %',
      'pnl',
      (r) => r.win_rate,
      (r) =>
        r.win_rate == null || !Number.isFinite(r.win_rate)
          ? tone('—', 'text-text-dim')
          : tone(`${(r.win_rate * 100).toFixed(0)}%`, winRateGradeClass(r.win_rate)),
      {
        tooltip: 'Share of CLOSED fired tokens with PnL > 0',
        // Display is percent points; wire value is a 0..1 fraction (same as Simulate).
        displayUnits: (n) => n * 100,
      },
    ),
    metric(
      'total_pnl_sol',
      'Total PnL',
      'pnl',
      (r) => r.total_pnl_sol,
      (r) => pnlCellWithScreenerFlag(r.total_pnl_sol, r.n_fired > 0 ? r.n_open / r.n_fired : null),
      {
        tooltip:
          'Realized only — sum of CLOSED positions. Still-open positions are excluded; see Open PnL. ' +
          'An “est” flag marks a high open-share row whose realized total is a screener estimate — re-simulate.',
      },
    ),
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
      (r) => mtmPnlSol(r.total_pnl_sol, r.open_pnl_sol),
      (r) => {
        const mtm = mtmPnlSol(r.total_pnl_sol, r.open_pnl_sol);
        return tone(solText(mtm), goodBad(mtm));
      },
      {
        tooltip:
          'Mark-to-market: realized Total PnL + unrealized Open PnL. What the combo is actually worth if every open bag were sold at its last price.',
      },
    ),
    metric(
      'mtm_pnl_pct',
      'MTM %',
      'pnl',
      (r) => mtmPctOf(r.total_pnl_sol, r.open_pnl_sol ?? 0, r.n_fired, buyAmountSol) ?? Number.NEGATIVE_INFINITY,
      (r) => {
        const v = mtmPctOf(r.total_pnl_sol, r.open_pnl_sol ?? 0, r.n_fired, buyAmountSol);
        return v == null ? tone('—', 'text-text-dim') : tone(pctText(v), pctGradeClass(v));
      },
      {
        tooltip:
          'Average per-trade return % including still-open marks (MTM PnL ÷ (buy × fired)). Feeds the Score.',
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
    metric(
      'median_pnl_pct',
      '≈ Median %',
      'pnl',
      (r) => r.median_pnl_pct,
      (r) => tone(pctText(r.median_pnl_pct), pctGradeClass(r.median_pnl_pct)),
      { tooltip: SKETCHED_QUANTILE_TOOLTIP },
    ),
    metric('mean_pnl_pct', 'Mean %', 'pnl', (r) => r.mean_pnl_pct, (r) => tone(pctText(r.mean_pnl_pct), pctGradeClass(r.mean_pnl_pct)), { defaultVisible: false }),
    metric('avg_holding_secs', 'Avg hold', 'holding', (r) => r.avg_holding_secs, (r) => tone(fmtSecs(r.avg_holding_secs), 'text-accent')),
    count('n_exit_take_profit', 'TP', 'text-green', (r) => r.n_exit_take_profit, { tooltip: 'Exited on take-profit' }),
    count('n_exit_stop_loss', 'SL', 'text-red', (r) => r.n_exit_stop_loss, { tooltip: 'Exited on stop-loss' }),
    metricsExitColumn(exitMetricLegend),
    count('n_exit_dead', 'Dead', 'text-red', (r) => r.n_exit_dead, {
      tooltip: 'Analysis death-close: token died silent, booked at the last meaningful trade',
    }),
    count('n_exit_open', 'Still open', 'text-text-dim', (r) => r.n_exit_open, { defaultVisible: false }),
  ];
}

/** One legend entry as a short `metric op value` fragment (e.g. `stall > 3`),
 *  or just the metric name when the condition didn't resolve (rare — see the
 *  backend's `exit_metric_legend`). */
function legendFragment(l: ExitMetricLegendEntry): string {
  if (l.operator == null || l.value == null) return l.metric;
  return `${l.metric} ${l.operator} ${formatDecimalTrim(l.value, 4)}`;
}

/** The `Metrics` exit column: the same aggregate count as before, plus — when
 *  the page's `X-Exit-Metric-Legend` named its slots — a per-row hover
 *  breakdown of WHICH authored condition each of those exits fired on. Falls
 *  back to the old undifferentiated count when there's no legend (legacy rows,
 *  a rule the sweep never resolved a slot for, or a run predating this column). */
function metricsExitColumn(exitMetricLegend: ExitMetricLegendEntry[]): ColumnDef<SweepResultRecord> {
  const baseTooltip = 'Exited because any exit metric condition became true';
  const breakdown = (r: SweepResultRecord): string | undefined => {
    const slots = r.n_exit_metrics_by_slot;
    if (!slots || exitMetricLegend.length === 0) return undefined;
    const parts = exitMetricLegend
      .map((l) => (slots[l.slot] ? `${legendFragment(l)}: ${slots[l.slot]}` : null))
      .filter((s): s is string => s != null);
    return parts.length > 0 ? parts.join('\n') : undefined;
  };
  return {
    key: 'n_exit_metrics',
    label: 'Metrics',
    group: 'exits',
    tooltip: baseTooltip,
    sortable: true,
    render: (r) => {
      const total = r.n_exit_metrics ?? 0;
      const title = breakdown(r);
      return (
        <span className={cn('font-medium', 'text-text-mid')} title={title ? `${baseTooltip}\n${title}` : baseTooltip}>
          {total}
        </span>
      );
    },
    sortValue: (r) => r.n_exit_metrics ?? 0,
    filterNumber: (r) => r.n_exit_metrics ?? 0,
    searchValue: () => '',
  };
}

// --- group columns ----------------------------------------------------------

/**
 * A human label + value + provenance for every axis the group was selected by.
 *
 * Reads the backend's resolved `selection` (`lab/src/sweep/selection.rs`) — the
 * ONE place that merges the scope fingerprint, the run's `ix_labels_filter` /
 * `field_filters` and the group key. Falls back to the bare `group_key` only for
 * a response from an older backend, which is exactly the lossy view that made a
 * pinned run render as "ALL tokens": those filters live on the RUN, so a card
 * built from `group_key` alone cannot see them.
 */
function keyParts(
  group: GroupedSweepGroupRecord,
): { label: string; value: string; origin?: string }[] {
  const sel = group.selection;
  if (sel) {
    return sel.clauses.map((c) => ({
      label: GROUP_FIELD_LABELS[c.field as GroupField] ?? c.field,
      value: c.display,
      origin: c.origin,
    }));
  }
  return Object.entries(group.group_key).map(([k, v]) => ({
    label: GROUP_FIELD_LABELS[k as GroupField] ?? k,
    value: v,
  }));
}

/** Short provenance tag shown next to a clause: where the pin came from. */
const ORIGIN_LABEL: Record<string, string> = {
  scope: 'fp',
  filter: 'filter',
  group_by: 'group',
};

/** Optional lookups for the Used-by column (promote identity → fingerprint → rules). */
export type GroupFingerprintLookup = ReadonlyMap<string, Fingerprint>;
export type RulesByFingerprintId = ReadonlyMap<string, StrategyRule[]>;

export interface GroupColumnLookups {
  fingerprintByGroupId?: GroupFingerprintLookup;
  rulesByFingerprintId?: RulesByFingerprintId;
}

/** Compact rule chips: name + paper/real + Active/Idle/Disabled (Fingerprints
 *  used-by language). Rules whose `params` exactly match the group's best combo
 *  are pinned first and highlighted so the promoted twin is obvious among siblings. */
function usedByRulesCell(
  rules: StrategyRule[],
  fingerprint?: Fingerprint | null,
  bestParams?: Record<string, unknown> | null,
): ReactNode {
  const ranked = bestParams
    ? [...rules].sort((a, b) => {
        const am = ruleParamsJsonEqual(a.params, bestParams) ? 0 : 1;
        const bm = ruleParamsJsonEqual(b.params, bestParams) ? 0 : 1;
        return am - bm;
      })
    : rules;

  return (
    <ul className="flex min-w-40 flex-col gap-1.5 text-left">
      {ranked.map((r) => {
        const isBest = !!bestParams && ruleParamsJsonEqual(r.params, bestParams);
        return (
          <li
            key={r.id}
            className={cn(
              'flex flex-wrap items-center gap-x-1.5 gap-y-0.5',
              isBest && 'rounded-md border border-primary/45 bg-primary/14 px-1.5 py-0.5 shadow-[0_0_0_1px_rgba(2,192,118,0.12)]',
            )}
          >
            <RuleHoverTip rule={r} fingerprint={fingerprint}>
              <Link
                to={rulesHref(r.id)}
                onClick={(e) => e.stopPropagation()}
                className={cn(
                  'inline-flex max-w-52 items-center gap-0.5 text-[12px] font-medium hover:underline',
                  isBest ? 'text-primary' : 'text-accent',
                )}
                title={`Open rule “${r.rule_name}”`}
              >
                <span className="truncate">{r.rule_name}</span>
                <LinkIcon className="h-3.5 w-3.5 shrink-0" />
              </Link>
            </RuleHoverTip>
            {isBest && (
              <Badge variant="primary" size="sm" title="Params match this group's best combo">
                best
              </Badge>
            )}
            <ModeBadge mode={r.trade_mode} />
            <Badge
              variant={!r.is_enabled ? 'danger' : r.is_active ? 'success' : 'neutral'}
              size="sm"
            >
              {!r.is_enabled ? 'Disabled' : r.is_active ? 'Active' : 'Idle'}
            </Badge>
          </li>
        );
      })}
    </ul>
  );
}

/** Build the generic-engine group-summary table columns. */
export function buildGenericGroupColumns(
  buyAmountSol = 1,
  lookups: GroupColumnLookups = {},
): ColumnDef<GroupedSweepGroupRecord>[] {
  const { fingerprintByGroupId, rulesByFingerprintId } = lookups;
  const rulesForGroup = (g: GroupedSweepGroupRecord): StrategyRule[] | null => {
    const fp = fingerprintByGroupId?.get(g.id);
    if (!fp) return null;
    return rulesByFingerprintId?.get(fp.id) ?? [];
  };

  // `group` mirrors the combo table's banners (counts / pnl / holding) so the
  // two stacked tables on the sweep page read the same. Columns must stay in
  // group order — DataTable bands *consecutive* runs of same-`group` columns.
  const gm = (
    key: string,
    label: string,
    group: 'counts' | 'pnl' | 'holding',
    value: (g: GroupedSweepGroupRecord) => number | null,
    render: (g: GroupedSweepGroupRecord) => ReactNode,
    opts: {
      tooltip?: string;
      defaultVisible?: boolean;
      /** Scale the wire value into displayed units for filter (e.g. fraction → %). */
      displayUnits?: (n: number) => number;
    } = {},
  ): ColumnDef<GroupedSweepGroupRecord> => {
    const units = opts.displayUnits ?? ((n: number) => n);
    return {
      key,
      label,
      group,
      tooltip: opts.tooltip,
      defaultVisible: opts.defaultVisible,
      sortable: true,
      render,
      sortValue: value,
      filterNumber: (g) => {
        const v = value(g);
        return v == null || !Number.isFinite(v) ? null : units(v);
      },
      searchValue: () => '',
    };
  };

  return [
    {
      key: 'group_key',
      label: 'Group (fingerprint)',
      group: 'group',
      sortable: false,
      render: (g) => {
        const parts = keyParts(g);
        const fp = fingerprintByGroupId?.get(g.id);
        const fpLabel = fp ? fp.name || fp.id.slice(0, 8) : null;
        const blockers = g.selection?.promotable === false ? g.selection.blockers : null;
        // "ALL tokens" now means what it says: nothing pinned the corpus and
        // nothing grouped it. A filtered run lands in the branch below.
        if (parts.length === 0 && !fp) return chip('ALL tokens', 'text-text-dim');
        return (
          <div className="flex flex-col gap-1.5 text-left">
            {blockers && blockers.length > 0 && (
              <span
                className="self-start rounded border border-warn/40 px-1 text-[10px] font-medium text-warn"
                title={`Can't promote this group:\n\n${blockers.join('\n\n')}`}
              >
                not promotable
              </span>
            )}
            {fp && (
              <Link
                to={fingerprintsHref(fp.id)}
                onClick={(e) => e.stopPropagation()}
                title={`Open fingerprint “${fpLabel}”`}
                aria-label={`Open fingerprint ${fpLabel}`}
                className="inline-flex items-center gap-1 self-start text-[12px] font-medium text-accent hover:text-primary hover:underline"
              >
                <LinkIcon className="h-3.5 w-3.5 shrink-0" />
                <span className="font-mono">{fpLabel}</span>
              </Link>
            )}
            {parts.length > 0 && (
              <div className="grid grid-cols-[auto_1fr] items-start gap-x-3 gap-y-1">
                {parts.map((p) => {
                  const isIxLabels = p.label === GROUP_FIELD_LABELS.ix_labels;
                  const ixParts = isIxLabels ? p.value.split(' | ') : null;
                  return (
                    <Fragment key={p.label}>
                      <span
                        className="text-[11px] leading-tight text-text-dim"
                        title={
                          p.origin
                            ? `${p.label}: ${p.value} (pinned by the ${
                                p.origin === 'scope'
                                  ? 'scope fingerprint'
                                  : p.origin === 'filter'
                                    ? "run's corpus filter"
                                    : 'group-by axis'
                              })`
                            : `${p.label}: ${p.value}`
                        }
                      >
                        {p.label}
                        {p.origin && p.origin !== 'group_by' && (
                          <span className="ml-1 text-[9px] uppercase text-text-dim/70">
                            {ORIGIN_LABEL[p.origin]}
                          </span>
                        )}
                        :
                      </span>
                      {ixParts ? (
                        <IxLabelsDisplay labels={ixParts} copyJson className="text-secondary" />
                      ) : (
                        <span>{chip(p.value, 'text-secondary')}</span>
                      )}
                    </Fragment>
                  );
                })}
              </div>
            )}
          </div>
        );
      },
      searchValue: (g) => {
        const fp = fingerprintByGroupId?.get(g.id);
        const fpText = fp ? fp.name || fp.id : '';
        return [fpText, ...keyParts(g).map((p) => `${p.label} ${p.value}`)].filter(Boolean).join(' ');
      },
    },
    {
      key: 'used_by',
      label: 'Used by',
      group: 'used_by',
      tooltip:
        'Saved rules whose fingerprint matches this group (same identity as promote). A "best" badge marks params that exactly match this group\'s winning combo. — = no saved fingerprint; none = fingerprint exists but no rules yet.',
      sortable: true,
      sortValue: (g) => rulesForGroup(g)?.length ?? -1,
      filterNumber: (g) => rulesForGroup(g)?.length ?? -1,
      render: (g) => {
        const rules = rulesForGroup(g);
        if (rules == null) return tone('—', 'text-text-dim');
        if (rules.length === 0) return tone('none', 'text-text-dim');
        return usedByRulesCell(rules, fingerprintByGroupId?.get(g.id), g.best_params);
      },
      searchValue: (g) => {
        const rules = rulesForGroup(g);
        if (rules == null) return '';
        if (rules.length === 0) return 'none';
        return rules
          .map(
            (r) =>
              `${r.rule_name} ${r.trade_mode} ${!r.is_enabled ? 'disabled' : r.is_active ? 'active' : 'idle'}`,
          )
          .join(' ');
      },
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
      (g) => (g.fired_count > 0 ? (g.best_n_open ?? 0) / g.fired_count : null),
      (g) => {
        if (!g.fired_count) return tone('—', 'text-text-dim');
        const share = (g.best_n_open ?? 0) / g.fired_count;
        return tone(`${(share * 100).toFixed(0)}%`, share >= 0.5 ? 'text-red' : share >= 0.25 ? 'text-warning' : 'text-text-dim');
      },
      {
        tooltip: 'Share of the winning combo’s fired positions still open. High = the realized headline rests on a thin closed sample.',
        displayUnits: (n) => n * 100,
      },
    ),
    gm(
      'best_score',
      'Score',
      'pnl',
      (g) => g.best_score,
      (g) =>
        g.best_score == null
          ? tone('—', 'text-text-dim')
          : tone(
              `${g.best_score >= 0 ? '+' : ''}${formatDecimalTrim(g.best_score, 4)}`,
              goodBad(g.best_score),
            ),
      {
        tooltip:
          "Checklist score of this group's best combo (MTM% × fire-rate × open-drag × win%).",
      },
    ),
    gm(
      'best_win_rate',
      'Win %',
      'pnl',
      (g) => g.best_win_rate,
      (g) =>
        g.best_win_rate == null || !Number.isFinite(g.best_win_rate)
          ? tone('—', 'text-text-dim')
          : tone(`${(g.best_win_rate * 100).toFixed(0)}%`, winRateGradeClass(g.best_win_rate)),
      { displayUnits: (n) => n * 100 },
    ),
    gm(
      'best_total_pnl_sol',
      'Total PnL',
      'pnl',
      (g) => g.best_total_pnl_sol,
      (g) =>
        pnlCellWithScreenerFlag(
          g.best_total_pnl_sol,
          g.fired_count > 0 ? (g.best_n_open ?? 0) / g.fired_count : null,
        ),
      {
        tooltip:
          'Realized only — sum of CLOSED positions. Still-open positions are excluded; see Open PnL. ' +
          'An “est” flag marks a high open-share winner whose realized total is a screener estimate — re-simulate.',
      },
    ),
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
      (g) => mtmPnlSol(g.best_total_pnl_sol, g.best_open_pnl_sol),
      (g) => {
        const mtm = mtmPnlSol(g.best_total_pnl_sol, g.best_open_pnl_sol);
        return tone(solText(mtm), goodBad(mtm));
      },
      {
        tooltip:
          'Mark-to-market: realized Total PnL + unrealized Open PnL. What the group is actually worth if every open bag were sold at its last price.',
      },
    ),
    gm(
      'best_mtm_pnl_pct',
      'MTM %',
      'pnl',
      (g) =>
        mtmPctOf(
          g.best_total_pnl_sol,
          g.best_open_pnl_sol ?? 0,
          g.fired_count,
          buyAmountSol,
        ),
      (g) => {
        const v = mtmPctOf(
          g.best_total_pnl_sol,
          g.best_open_pnl_sol ?? 0,
          g.fired_count,
          buyAmountSol,
        );
        return v == null ? tone('—', 'text-text-dim') : tone(pctText(v), pctGradeClass(v));
      },
      {
        tooltip:
          "Winning combo's average per-trade return % including still-open marks.",
      },
    ),
    gm('best_expectancy_sol', 'Expectancy', 'pnl', (g) => g.best_expectancy_sol, (g) => tone(solText(g.best_expectancy_sol), goodBad(g.best_expectancy_sol))),
    gm('best_profit_factor', 'Profit factor', 'pnl', (g) => g.best_profit_factor ?? Number.POSITIVE_INFINITY, (g) =>
      tone(g.best_profit_factor == null ? '∞' : g.best_profit_factor.toFixed(2), goodBad(g.best_profit_factor ?? 10, 1)),
    ),
    gm(
      'best_median_pnl_pct',
      '≈ Median %',
      'pnl',
      (g) => g.best_median_pnl_pct,
      (g) => tone(pctText(g.best_median_pnl_pct), pctGradeClass(g.best_median_pnl_pct)),
      { tooltip: SKETCHED_QUANTILE_TOOLTIP },
    ),
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
