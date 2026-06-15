import type { ReactNode } from 'react';
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
  if (v == null) return 'off';
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

/** Short best-combo summary: the headline TP/SL plus any other knobs, compact. */
function bestComboParts(params: Record<string, number | null>): string[] {
  const order = [
    'exit_take_profit',
    'exit_stop_loss',
    'exit_trailing_stop_pct',
    'exit_time_stop_secs',
    'exit_stall_secs',
    'entry_min_age_secs',
    'entry_pullback_pct',
    'entry_min_liquidity_sol',
  ];
  const keys = Object.keys(params).sort(
    (a, b) =>
      (order.indexOf(a) === -1 ? Infinity : order.indexOf(a)) -
      (order.indexOf(b) === -1 ? Infinity : order.indexOf(b)),
  );
  return keys.map((k) => `${k.replace(/^(exit|entry)_/, '').replace(/_/g, ' ')} ${fmtParam(k, params[k])}`);
}

/**
 * Columns for the group-summary table (group list → drill into combos). The
 * fingerprint key renders as labeled chips; the headline metric is the winning
 * combo's expectancy per trade, with its sample size (`fired_count`) beside it so
 * a "best" combo riding a handful of lucky tokens is obvious at a glance.
 */
export function buildGroupColumns(): ColumnDef<GroupedSweepGroupRecord>[] {
  return [
    {
      key: 'group_key',
      label: 'Group (fingerprint)',
      group: 'group',
      sortable: false,
      width: '320px',
      render: (g) => {
        const parts = keyParts(g);
        if (parts.length === 0) return chip('ALL tokens', 'text-text-dim');
        return (
          <div className="flex flex-wrap gap-1">
            {parts.map((p) => (
              <span key={p.label} title={`${p.label}: ${p.value}`}>
                {chip(
                  <>
                    <span className="text-text-dim">{p.label}:</span>{' '}
                    <span className="text-secondary">{p.value}</span>
                  </>,
                )}
              </span>
            ))}
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
      group: 'counts',
      sortable: true,
      render: (g) => <span className="font-medium text-info">{g.token_count}</span>,
      sortValue: (g) => g.token_count,
      filterNumber: (g) => g.token_count,
      searchValue: () => '',
      tooltip: 'Tokens in this fingerprint group',
    },
    {
      key: 'fired_count',
      label: 'Fired',
      group: 'counts',
      sortable: true,
      render: (g) => <span className="font-medium text-info">{g.fired_count}</span>,
      sortValue: (g) => g.fired_count,
      filterNumber: (g) => g.fired_count,
      searchValue: () => '',
      tooltip: "The best combo's fired count — the sample size behind its expectancy",
    },
    {
      key: 'best_expectancy_sol',
      label: 'Best expectancy',
      group: 'pnl',
      sortable: true,
      render: (g) => (
        <span className={cn('font-medium', goodBad(g.best_expectancy_sol))}>
          {solText(g.best_expectancy_sol)}
        </span>
      ),
      sortValue: (g) => g.best_expectancy_sol,
      filterNumber: (g) => g.best_expectancy_sol,
      searchValue: () => '',
      tooltip: 'Mean net PnL per trade (SOL) of this group’s best combo — the ranking metric',
    },
    {
      key: 'best_params',
      label: 'Best combo',
      group: 'params',
      sortable: false,
      width: '360px',
      render: (g) => (
        <div className="flex flex-wrap gap-1">
          {bestComboParts(g.best_params).map((p, i) => (
            <span key={i}>{chip(p, 'text-text-mid')}</span>
          ))}
        </div>
      ),
      searchValue: (g) => bestComboParts(g.best_params).join(' '),
    },
  ];
}
