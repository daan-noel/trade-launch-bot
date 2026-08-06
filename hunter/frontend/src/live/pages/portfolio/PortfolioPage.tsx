import { useCallback, useMemo, useState } from 'react';
import { Link, useSearchParams } from 'react-router-dom';
import { DataTable } from 'components/table/DataTable';
import type { ColumnDef } from 'components/table/types';
import { StatTile } from 'components/ui/StatTile';
import { PageHeader } from 'components/ui/PageHeader';
import { LinkIcon } from 'components/ui/icons';
import { consoleHref, consoleHistoryHref, rulesHref } from 'lib/strategy/nav';
import { formatCompact } from 'utils/format';
import {
  formatSigned,
  formatSignedPct,
  pctGradeClass,
  signedStatTone,
  signedToneClass,
  winRateGradeClass,
} from 'lib/signedTone';
import { pnlPctFromSol } from 'lib/pnlPct';
import { useTimezone } from 'context/TimezoneContext';
import { RankedPnlBars } from 'components/analytics/RankedPnlBars';
import { PnlSparkline } from 'components/analytics/PnlSparkline';
import {
  groupDailyPnl,
  groupTrends,
  type GroupTrend,
  type PnlPoint,
} from 'components/analytics/pnlSeries';
import {
  useGetPortfolioClosesSeriesQuery,
  useGetPortfolioPerformanceQuery,
  useGetPortfolioSummaryQuery,
} from '@live/store/liveEndpoints';
import { PortfolioRuleDetail } from '@live/components/portfolio/PortfolioRuleDetail';
import type { PortfolioRulePnl } from 'types';

type Range = 'today' | '7d' | '30d' | 'all';
type Mode = 'real' | 'paper';

/** Trades per rolling window for the decay comparison. Small enough that a
 *  low-volume rule still gets two windows; large enough that a single tail
 *  trade can't flip the verdict on its own. */
const DECAY_WINDOW = 20;

/**
 * Portfolio — the **rule scoreboard**: which rules earn their keep, and is any
 * of them decaying. Rules = keep/kill · Console History = the trade browser ·
 * Portfolio = am I making money, and from what?
 */
export function PortfolioPage() {
  const [params, setParams] = useSearchParams();
  const { timezone } = useTimezone();
  const range = (params.get('range') as Range | null) ?? 'today';
  const mode = (params.get('mode') as Mode | null) ?? 'real';
  const selectedRule = params.get('rule');
  const setRange = (r: Range) => {
    const next = new URLSearchParams(params);
    next.set('range', r);
    setParams(next, { replace: true });
  };
  const setMode = (m: Mode) => {
    const next = new URLSearchParams(params);
    next.set('mode', m);
    setParams(next, { replace: true });
  };

  const { data: summary } = useGetPortfolioSummaryQuery();
  const { data, isLoading, isFetching } = useGetPortfolioPerformanceQuery({ range, mode });
  // The per-close series (B2) — one fetch behind BOTH the sparkline column and
  // the decay marker, so they can't disagree with each other or with the
  // Console History charts, which fold the same payload.
  const { data: series } = useGetPortfolioClosesSeriesQuery({ range, mode });

  const openMtm = summary?.total_unrealized_pnl_sol ?? null;

  const ruleNameOf = useCallback(
    (id: string) => data?.by_rule.find((r) => r.rule_id === id)?.rule_name ?? `${id.slice(0, 8)}…`,
    [data?.by_rule],
  );

  const points = useMemo<PnlPoint[]>(
    () =>
      (series?.closes ?? []).map((c, i) => ({
        key: `${c.exit_time}:${i}`,
        timeMs: Date.parse(c.exit_time),
        pnlSol: c.pnl_sol,
        pnlPct: c.entry_sol > 0 ? (c.pnl_sol / c.entry_sol) * 100 : null,
        label: c.rule_id ?? 'unknown',
        groupId: c.rule_id,
      })),
    [series],
  );

  /** rule_id → daily realized PnL (the sparkline series). */
  const dailyByRule = useMemo(() => groupDailyPnl(points, timezone), [points, timezone]);
  /** rule_id → rolling-window trend (the decay marker). */
  const trendByRule = useMemo(() => {
    const m = new Map<string, GroupTrend>();
    for (const t of groupTrends(points, ruleNameOf, DECAY_WINDOW)) m.set(t.groupId, t);
    return m;
  }, [points, ruleNameOf]);

  const decayingCount = useMemo(
    () => [...trendByRule.values()].filter((t) => t.decaying).length,
    [trendByRule],
  );

  const rankedBars = useMemo(
    () =>
      (data?.by_rule ?? []).map((r) => ({
        key: r.rule_id,
        label: r.rule_name ?? r.rule_id.slice(0, 8),
        value: r.realized_pnl_sol,
        tag: trendByRule.get(r.rule_id)?.decaying ? 'decaying' : null,
        title: r.rule_name ?? r.rule_id,
      })),
    [data?.by_rule, trendByRule],
  );

  // Recompute the realized/win-rate/rules tiles over the table's own filtered
  // cohort (search + column filters), the way a server summary tracks its table.
  // `onFilteredRowsChange` fires with every matching row (pre-pagination). Only
  // treated as "filtered" when it narrows the set, so the unfiltered view keeps
  // the server's authoritative totals (which may include unattributed closes).
  const [filteredRows, setFilteredRows] = useState<PortfolioRulePnl[] | null>(null);
  const onFilteredRowsChange = useCallback((r: PortfolioRulePnl[]) => setFilteredRows(r), []);
  const tiles = useMemo(() => {
    const full = data?.by_rule.length ?? 0;
    const isFiltered = filteredRows != null && full > 0 && filteredRows.length < full;
    if (!isFiltered || !filteredRows) {
      return data
        ? {
            filtered: false,
            realized: data.realized_pnl_sol,
            closed: data.closed,
            win: data.win,
            loss: data.loss,
            winRate: data.win_rate,
            rules: full,
          }
        : null;
    }
    let realized = 0;
    let closed = 0;
    let win = 0;
    let loss = 0;
    for (const r of filteredRows) {
      realized += r.realized_pnl_sol;
      closed += r.closed;
      win += r.win;
      loss += r.loss;
    }
    const decided = win + loss;
    return {
      filtered: true,
      realized,
      closed,
      win,
      loss,
      winRate: decided > 0 ? (win / decided) * 100 : 0,
      rules: filteredRows.length,
    };
  }, [data, filteredRows]);

  const columns: ColumnDef<PortfolioRulePnl>[] = useMemo(
    () => [
      {
        key: 'rule',
        label: 'Rule',
        render: (r) => (
          <Link
            to={rulesHref(r.rule_id)}
            className="inline-flex items-center gap-0.5 font-medium text-accent hover:text-primary hover:underline"
            onClick={(e) => e.stopPropagation()}
          >
            <span>{r.rule_name ?? r.rule_id.slice(0, 8)}</span>
            <LinkIcon className="h-3.5 w-3.5 shrink-0" />
          </Link>
        ),
        searchValue: (r) => r.rule_name ?? r.rule_id,
        sortValue: (r) => r.rule_name ?? r.rule_id,
        sortable: true,
      },
      {
        key: 'pnl',
        label: 'PnL',
        sortable: true,
        render: (r) => (
          <span className={`tabular-nums text-xs font-semibold ${signedToneClass(r.realized_pnl_sol)}`}>
            {formatSigned(r.realized_pnl_sol, 3)}◎
          </span>
        ),
        sortValue: (r) => r.realized_pnl_sol,
        searchValue: (r) => String(r.realized_pnl_sol),
        filterNumber: (r) => r.realized_pnl_sol,
      },
      {
        key: 'pnl_pct',
        label: 'PnL%',
        sortable: true,
        render: (r) => {
          const pct = pnlPctFromSol(r.realized_pnl_sol, r.total_entry_sol);
          return pct != null ? (
            <span className={`tabular-nums text-xs ${pctGradeClass(pct)}`}>
              {formatSignedPct(pct, 1)}
            </span>
          ) : (
            <span className="text-text-dim">—</span>
          );
        },
        sortValue: (r) => pnlPctFromSol(r.realized_pnl_sol, r.total_entry_sol) ?? 0,
        searchValue: (r) => String(pnlPctFromSol(r.realized_pnl_sol, r.total_entry_sol) ?? ''),
        filterNumber: (r) => pnlPctFromSol(r.realized_pnl_sol, r.total_entry_sol),
      },
      {
        key: 'win',
        label: 'Win%',
        sortable: true,
        render: (r) =>
          r.closed > 0 ? (
            <span className={`tabular-nums text-xs ${winRateGradeClass(r.win_rate / 100)}`}>
              {Math.round(r.win_rate)}%
            </span>
          ) : (
            <span className="text-text-dim">—</span>
          ),
        sortValue: (r) => r.win_rate,
        searchValue: (r) => String(r.win_rate),
        filterNumber: (r) => (r.closed > 0 ? r.win_rate : null),
      },
      {
        key: 'wl',
        label: 'W/L',
        render: (r) => (
          <span className="tabular-nums text-xs">
            <span className="text-green">{r.win}</span>
            <span className="text-text-dim">/</span>
            <span className="text-red">{r.loss}</span>
          </span>
        ),
        searchValue: (r) => `${r.win}/${r.loss}`,
        sortValue: (r) => r.win * 1_000_000 + r.loss,
        sortable: true,
      },
      {
        key: 'n',
        label: 'N',
        sortable: true,
        render: (r) => <span className="tabular-nums text-xs text-text-mid">{r.closed}</span>,
        sortValue: (r) => r.closed,
        searchValue: (r) => String(r.closed),
        filterNumber: (r) => r.closed,
      },
      {
        key: 'entry',
        label: 'Entry ◎',
        sortable: true,
        render: (r) => (
          <span className="tabular-nums text-xs text-text-dim">
            {formatCompact(r.total_entry_sol, 2)}
          </span>
        ),
        sortValue: (r) => r.total_entry_sol,
        searchValue: (r) => String(r.total_entry_sol),
        filterNumber: (r) => r.total_entry_sol,
      },
      {
        key: 'trend',
        label: 'Trend',
        width: '100px',
        render: (r) => {
          const days = dailyByRule.get(r.rule_id) ?? [];
          if (days.length === 0) return <span className="text-text-dim">—</span>;
          return (
            <PnlSparkline
              values={days.map((d) => d.pnlSol)}
              title={`${r.rule_name ?? r.rule_id} — cumulative realized PnL by day`}
            />
          );
        },
        searchValue: () => '',
      },
      {
        // The decay detector: last-N vs prior-N win rate AND expectancy. Both
        // must be down to flag — either one alone flips on a single tail trade.
        key: 'decay',
        label: 'Δ Win%',
        sortable: true,
        render: (r) => {
          const t = trendByRule.get(r.rule_id);
          if (!t || t.winRateDeltaPp == null) {
            return (
              <span
                className="text-text-dim"
                title={`Needs ${DECAY_WINDOW * 2} closes to compare two windows`}
              >
                —
              </span>
            );
          }
          return (
            <span
              className={`tabular-nums text-xs ${signedToneClass(t.winRateDeltaPp)}`}
              title={
                `last ${DECAY_WINDOW}: ${t.recent.winRate?.toFixed(0)}% win, ` +
                `${formatSigned(t.recent.expectancySol ?? 0, 4)}◎/trade\n` +
                `prior ${DECAY_WINDOW}: ${t.prior.winRate?.toFixed(0)}% win, ` +
                `${formatSigned(t.prior.expectancySol ?? 0, 4)}◎/trade`
              }
            >
              {t.decaying && <span className="mr-0.5 font-bold text-red">▼</span>}
              {formatSignedPct(t.winRateDeltaPp, 0)}
            </span>
          );
        },
        sortValue: (r) => trendByRule.get(r.rule_id)?.winRateDeltaPp ?? null,
        searchValue: (r) => (trendByRule.get(r.rule_id)?.decaying ? 'decaying' : ''),
        filterNumber: (r) => trendByRule.get(r.rule_id)?.winRateDeltaPp ?? null,
      },
      {
        key: 'history',
        label: '',
        width: '76px',
        render: (r) => (
          <Link
            to={consoleHistoryHref({ ruleId: r.rule_id, range, mode })}
            className="text-[11px] font-semibold text-accent hover:underline"
            onClick={(e) => e.stopPropagation()}
            title="Open this rule's trades in the Console History cohort"
          >
            History
          </Link>
        ),
        searchValue: () => '',
      },
    ],
    [mode, range, dailyByRule, trendByRule],
  );

  const selectedRow = useMemo(
    () => (selectedRule ? data?.by_rule.find((r) => r.rule_id === selectedRule) : undefined),
    [data?.by_rule, selectedRule],
  );

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Portfolio"
        description={
          <>
            Which rules earn their keep ·{' '}
            <Link to={rulesHref()} className="text-accent hover:underline">
              Rules
            </Link>
            {' · '}
            <Link to={consoleHref()} className="text-accent hover:underline">
              Console
            </Link>
            {' · '}
            <Link to={consoleHistoryHref({ range, mode })} className="text-accent hover:underline">
              Trade history
            </Link>
          </>
        }
      />

      <div className="flex flex-wrap items-center gap-2">
        {(
          [
            ['today', 'Today'],
            ['7d', '7 days'],
            ['30d', '30 days'],
            ['all', 'All-time'],
          ] as const
        ).map(([key, label]) => (
          <button
            key={key}
            type="button"
            onClick={() => setRange(key)}
            className={`rounded-md px-2.5 py-1 text-xs font-semibold ${
              range === key
                ? 'bg-primary/20 text-primary'
                : 'bg-white/5 text-text-dim hover:bg-white/8'
            }`}
          >
            {label}
          </button>
        ))}
        <span className="mx-1 text-text-dim">·</span>
        {(['real', 'paper'] as const).map((m) => (
          <button
            key={m}
            type="button"
            onClick={() => setMode(m)}
            className={`rounded-md px-2.5 py-1 text-xs font-semibold capitalize ${
              mode === m
                ? 'bg-primary/20 text-primary'
                : 'bg-white/5 text-text-dim hover:bg-white/8'
            }`}
          >
            {m}
          </button>
        ))}
      </div>

      <div className="grid grid-cols-2 gap-2.5 sm:grid-cols-4">
        <StatTile
          label={tiles?.filtered ? 'Realized · filtered' : 'Realized'}
          value={tiles ? `◎${formatSigned(tiles.realized, 3)}` : '—'}
          sub={tiles ? `${tiles.closed} closed` : undefined}
          tone={signedStatTone(tiles?.realized ?? null)}
        />
        <StatTile
          label="Win rate"
          value={tiles && tiles.closed > 0 ? `${Math.round(tiles.winRate)}%` : '—'}
          sub={tiles ? `${tiles.win}W / ${tiles.loss}L` : undefined}
        />
        <StatTile
          label="Open MTM"
          value={openMtm != null ? `◎${formatSigned(openMtm, 3)}` : '—'}
          sub="wallet unrealized"
          tone={signedStatTone(openMtm)}
        />
        <StatTile
          label="Decaying rules"
          value={trendByRule.size > 0 ? decayingCount : '—'}
          sub={
            trendByRule.size > 0
              ? `of ${tiles?.rules ?? 0} · last ${DECAY_WINDOW} vs prior`
              : isFetching
                ? 'refreshing…'
                : 'needs 2 windows'
          }
          tone={decayingCount > 0 ? 'red' : 'muted'}
        />
      </div>

      {rankedBars.length > 0 && (
        <div className="rounded-lg border border-white/6 bg-bg-panel p-3">
          <h2 className="mb-2 text-[10px] font-bold uppercase tracking-wider text-text-dim">
            Realized PnL by rule — every rule in the window
          </h2>
          <RankedPnlBars rows={rankedBars} emptyMessage="No closed trades in this window." />
        </div>
      )}

      <DataTable
        columns={columns}
        rows={data?.by_rule ?? []}
        rowKey={(r) => r.rule_id}
        loading={isLoading}
        searchable
        defaultSort={{ col: 'pnl', dir: 'desc' }}
        tableId="portfolio-by-rule"
        pinnable
        emptyMessage="No closed trades in this window."
        onFilteredRowsChange={onFilteredRowsChange}
        selectedKey={selectedRule}
        rowDetail={(r) => <PortfolioRuleDetail row={r} range={range} mode={mode} />}
        onSelect={(key) => {
          const next = new URLSearchParams(params);
          if (!key) next.delete('rule');
          else next.set('rule', key);
          setParams(next, { replace: true });
        }}
      />

      {/* Keep selection keyed even if table remounts mid-fetch */}
      {selectedRule && !selectedRow && !isLoading ? (
        <p className="text-[11px] text-text-dim">Selected rule has no closes in this window.</p>
      ) : null}
    </div>
  );
}
