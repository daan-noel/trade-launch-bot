import { useCallback, useEffect, useMemo, useState } from 'react';
import { Link, useSearchParams } from 'react-router-dom';
import { DataTable } from 'components/table/DataTable';
import type { ColumnDef } from 'components/table/types';
import { PageHeader } from 'components/ui/PageHeader';
import { DateTimeRangePicker } from 'components/ui/DateTimeRangePicker';
import { ModeToggle } from 'components/strategy/ModeToggle';
import { LinkIcon } from 'components/ui/icons';
import { consoleHistoryHref, rulesHref } from 'lib/strategy/nav';
import {
  formatSigned,
  formatSignedPct,
  signedToneClass,
} from 'lib/signedTone';
import { useTimezone } from 'context/TimezoneContext';
import { RankedPnlBars } from 'components/analytics/RankedPnlBars';
import { PnlSparkline } from 'components/analytics/PnlSparkline';
import {
  foldPnlDeck,
  type GroupTrend,
  type PnlPoint,
} from 'components/analytics/pnlSeries';
import {
  useGetPortfolioClosesSeriesQuery,
  useGetPortfolioPerformanceQuery,
} from '@live/store/liveEndpoints';
import type { PortfolioRulePnl } from 'types';

const portfolioRuleRowKey = (r: PortfolioRulePnl) => r.rule_id;

type Range = 'today' | '7d' | '30d' | 'all';
type Mode = 'real' | 'paper';

const PORTFOLIO_RANGE_PRESETS: { value: Range; label: string }[] = [
  { value: 'today', label: 'Today' },
  { value: '7d', label: '7 days' },
  { value: '30d', label: '30 days' },
  { value: 'all', label: 'All-time' },
];

/** Trades per rolling window for the decay comparison. Small enough that a
 *  low-volume rule still gets two windows; large enough that a single tail
 *  trade can't flip the verdict on its own. */
const DECAY_WINDOW = 20;

/** Window expectancy — mean SOL per closed trade. */
function expectancySol(r: PortfolioRulePnl): number | null {
  return r.closed > 0 ? r.realized_pnl_sol / r.closed : null;
}

/**
 * Portfolio — keep/kill review board for a calendar window.
 * Rank who earned SOL, flag decay, hand off to History (trades) or Rules (act).
 */
export function PortfolioPage() {
  const [params, setParams] = useSearchParams();
  const { timezone } = useTimezone();
  const rawRange = params.get('range');
  const range: Range =
    rawRange === 'today' ||
    rawRange === '7d' ||
    rawRange === '30d' ||
    rawRange === 'all'
      ? rawRange
      : '7d';
  const mode = (params.get('mode') as Mode | null) ?? 'real';
  const selectedRule = params.get('rule');
  const decayOnly = params.get('decay') === '1';

  /** Every URL write here patches `prev` rather than a closed-over snapshot, so
   *  no writer can resurrect a key another one just changed, and the handlers
   *  stay referentially stable for the memoized children below. */
  const patchParams = useCallback(
    (mutate: (next: URLSearchParams) => void) => {
      setParams(
        (prev) => {
          const next = new URLSearchParams(prev);
          mutate(next);
          return next;
        },
        { replace: true },
      );
    },
    [setParams],
  );

  const setRange = useCallback(
    (r: Range) => patchParams((p) => p.set('range', r)),
    [patchParams],
  );
  const setMode = useCallback(
    (m: Mode) => patchParams((p) => p.set('mode', m)),
    [patchParams],
  );
  const selectRule = useCallback(
    (key: string | null) =>
      patchParams((p) => {
        if (!key) p.delete('rule');
        else p.set('rule', key);
      }),
    [patchParams],
  );
  const toggleDecayFilter = useCallback(
    () =>
      patchParams((p) => {
        if (decayOnly) p.delete('decay');
        else p.set('decay', '1');
      }),
    [decayOnly, patchParams],
  );

  const { data, isLoading, isFetching } = useGetPortfolioPerformanceQuery({ range, mode });
  // B2 series — portfolio spark + decay alerts/Form share one fold with History.
  const { data: series } = useGetPortfolioClosesSeriesQuery({ range, mode });

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

  const deck = useMemo(
    () =>
      foldPnlDeck(points, {
        timeZone: timezone,
        labelOf: ruleNameOf,
        window: DECAY_WINDOW,
        only: ['days', 'trends'],
      }),
    [points, timezone, ruleNameOf],
  );

  const windowSpark = useMemo(() => deck.days.map((d) => d.pnlSol), [deck.days]);
  const trendByRule = useMemo(() => {
    const m = new Map<string, GroupTrend>();
    for (const t of deck.trends) m.set(t.groupId, t);
    return m;
  }, [deck.trends]);

  const decaying = useMemo(
    () => deck.trends.filter((t) => t.decaying),
    [deck.trends],
  );
  const decayingCount = decaying.length;

  const effectiveDecayOnly = decayOnly && decayingCount > 0;
  useEffect(() => {
    if (decayOnly && decayingCount === 0 && trendByRule.size > 0) {
      patchParams((p) => p.delete('decay'));
    }
  }, [decayOnly, decayingCount, trendByRule.size, patchParams]);

  const tableRows = useMemo(() => {
    const all = data?.by_rule ?? [];
    if (!effectiveDecayOnly) return all;
    const rows = all.filter((r) => trendByRule.get(r.rule_id)?.decaying);
    return [...rows].sort((a, b) => {
      const da = expectancySol(a) ?? 0;
      const db = expectancySol(b) ?? 0;
      return da - db;
    });
  }, [data?.by_rule, effectiveDecayOnly, trendByRule]);

  const rankedBars = useMemo(
    () =>
      tableRows.map((r) => ({
        key: r.rule_id,
        label: r.rule_name ?? r.rule_id.slice(0, 8),
        value: r.realized_pnl_sol,
        tag: trendByRule.get(r.rule_id)?.decaying ? 'decaying' : null,
        title: r.rule_name ?? r.rule_id,
      })),
    [tableRows, trendByRule],
  );

  const [filteredRows, setFilteredRows] = useState<PortfolioRulePnl[] | null>(null);
  const onFilteredRowsChange = useCallback((r: PortfolioRulePnl[]) => setFilteredRows(r), []);
  const summary = useMemo(() => {
    const full = data?.by_rule.length ?? 0;
    const base = effectiveDecayOnly ? tableRows : (data?.by_rule ?? []);
    const baseLen = base.length;
    const isFiltered =
      filteredRows != null && baseLen > 0 && filteredRows.length < baseLen;
    const rows = isFiltered && filteredRows ? filteredRows : base;
    if (!data && rows.length === 0) return null;
    if (!isFiltered && !effectiveDecayOnly && data) {
      return {
        filtered: false,
        realized: data.realized_pnl_sol,
        capital: data.closed_entry_sol,
        returnPct: data.closed_entry_sol > 0 ? data.return_pct : null,
        closed: data.closed,
        rules: full,
      };
    }
    let realized = 0;
    let closed = 0;
    // Re-weight by capital when the row set narrows — Σ pnl / Σ capital, never
    // a mean of the surviving rules' percents (a 0.05 ◎ rule would then count
    // as much as a 1.0 ◎ one).
    let capital = 0;
    for (const r of rows) {
      realized += r.realized_pnl_sol;
      closed += r.closed;
      capital += r.closed_entry_sol;
    }
    return {
      filtered: isFiltered || effectiveDecayOnly,
      realized,
      capital,
      returnPct: capital > 0 ? (realized / capital) * 100 : null,
      closed,
      rules: rows.length,
    };
  }, [data, filteredRows, effectiveDecayOnly, tableRows]);

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
            title="Open in Rules (keep / kill)"
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
        // The PnL column in the units that compare across rules: realized SOL
        // over the capital that produced it. Server-derived (`weighted_return_pct`)
        // so it is the same definition as the Rules board's Return%, and
        // sign-locked to the PnL beside it.
        key: 'return_pct',
        label: 'Return%',
        sortable: true,
        render: (r) =>
          r.closed_entry_sol > 0 ? (
            <span
              className={`tabular-nums text-xs ${signedToneClass(r.return_pct)}`}
              title={`${formatSigned(r.realized_pnl_sol, 3)}◎ on ${r.closed_entry_sol.toFixed(3)}◎ deployed`}
            >
              {formatSignedPct(r.return_pct, 1)}
            </span>
          ) : (
            <span className="text-text-dim">—</span>
          ),
        sortValue: (r) => (r.closed_entry_sol > 0 ? r.return_pct : null),
        searchValue: (r) => String(r.return_pct),
        filterNumber: (r) => (r.closed_entry_sol > 0 ? r.return_pct : null),
      },
      {
        key: 'exp',
        label: 'Exp',
        sortable: true,
        render: (r) => {
          const exp = expectancySol(r);
          return exp != null ? (
            <span className={`tabular-nums text-xs ${signedToneClass(exp)}`} title="◎ per closed trade">
              {formatSigned(exp, 4)}◎
            </span>
          ) : (
            <span className="text-text-dim">—</span>
          );
        },
        sortValue: (r) => expectancySol(r),
        searchValue: (r) => String(expectancySol(r) ?? ''),
        filterNumber: (r) => expectancySol(r),
      },
      {
        // Last-N vs prior-N: ▼ only when BOTH win rate AND expectancy fell.
        key: 'form',
        label: 'Form',
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
          const expDelta = t.expectancyDeltaSol;
          return (
            <span
              className={`tabular-nums text-xs ${signedToneClass(t.winRateDeltaPp)}`}
              title={
                `last ${DECAY_WINDOW}: ${t.recent.winRate?.toFixed(0)}% win, ` +
                `${formatSigned(t.recent.expectancySol ?? 0, 4)}◎/trade\n` +
                `prior ${DECAY_WINDOW}: ${t.prior.winRate?.toFixed(0)}% win, ` +
                `${formatSigned(t.prior.expectancySol ?? 0, 4)}◎/trade\n` +
                `Δ win ${formatSignedPct(t.winRateDeltaPp, 0)}` +
                (expDelta != null ? ` · Δ exp ${formatSigned(expDelta, 4)}◎/trade` : '') +
                (t.decaying
                  ? '\n▼ decaying — win rate and expectancy both down'
                  : '')
              }
            >
              {t.decaying && <span className="mr-0.5 font-bold text-red">▼</span>}
              {formatSignedPct(t.winRateDeltaPp, 0)}
              {expDelta != null && (
                <span className={`ml-1 ${signedToneClass(expDelta)}`}>
                  {formatSigned(expDelta, 3)}◎
                </span>
              )}
            </span>
          );
        },
        sortValue: (r) => trendByRule.get(r.rule_id)?.winRateDeltaPp ?? null,
        searchValue: (r) => (trendByRule.get(r.rule_id)?.decaying ? 'decaying' : ''),
        filterNumber: (r) => trendByRule.get(r.rule_id)?.winRateDeltaPp ?? null,
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
        key: 'history',
        label: '',
        width: '72px',
        render: (r) => (
          <Link
            to={consoleHistoryHref({ ruleId: r.rule_id, range, mode })}
            className="text-[11px] font-semibold text-accent hover:underline"
            onClick={(e) => e.stopPropagation()}
            title="Open this rule's trades in Console History"
          >
            History
          </Link>
        ),
        searchValue: () => '',
      },
    ],
    [mode, range, trendByRule],
  );

  const entryFailed = series?.entry_failed ?? 0;

  return (
    <div className="flex flex-col gap-4">
      <PageHeader title="Portfolio" description="Keep / kill review for a calendar window." />

      <div className="flex flex-col gap-1">
        <div className="flex flex-wrap items-center gap-2">
          <DateTimeRangePicker
            aria-label="Date range"
            size="sm"
            zoneLabel={null}
            allowCustom={false}
            presets={PORTFOLIO_RANGE_PRESETS}
            value={{ preset: range, from: '', to: '' }}
            onChange={({ preset }) => setRange(preset)}
          />
          <ModeToggle
            includeAll={false}
            layout="ops"
            size="sm"
            value={mode}
            onChange={setMode}
          />
        </div>
        <p className="text-[11px] text-text-dim">
          Calendar-window closed PnL — not Rules Control current-run / all-time scores.
        </p>
      </div>

      {/* Window money — spark answers "am I making money?" before the table. */}
      <div className="flex flex-wrap items-center gap-x-5 gap-y-2 rounded-lg border border-white/6 bg-bg-panel px-3 py-2.5">
        <div className="flex items-center gap-2.5">
          <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">
            {summary?.filtered ? 'Realized · filtered' : 'Realized'}
          </span>
          <PnlSparkline
            values={windowSpark}
            width={120}
            height={24}
            title="Daily realized PnL in this window"
          />
          <span
            className={`tabular-nums text-lg font-semibold ${signedToneClass(summary?.realized ?? 0)}`}
          >
            {summary ? `${formatSigned(summary.realized, 3)}◎` : '—'}
          </span>
          {summary?.returnPct != null && (
            <span
              className={`tabular-nums text-sm font-semibold ${signedToneClass(summary.returnPct)}`}
              title={`Return on the ${summary.capital.toFixed(3)}◎ deployed across this window's closed trades`}
            >
              {formatSignedPct(summary.returnPct, 1)}
            </span>
          )}
        </div>
        <span className="text-[11px] text-text-dim">
          {summary
            ? `${summary.closed} closed · ${summary.rules} rule${summary.rules === 1 ? '' : 's'}`
            : isFetching
              ? 'refreshing…'
              : '—'}
        </span>
        {entryFailed > 0 && (
          <span
            className="text-[11px] text-text-dim"
            title="Buys that never filled in this window — no SOL deployed"
          >
            {entryFailed} entry-failed
          </span>
        )}
        <Link
          to={consoleHistoryHref({ range, mode })}
          className="ml-auto text-[11px] font-semibold text-accent hover:underline"
        >
          Review all trades →
        </Link>
      </div>

      {decayingCount > 0 && (
        <div className="flex flex-col gap-1.5 rounded-lg border border-red/25 bg-red/5 px-3 py-2.5">
          <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
            <span className="text-[10px] font-bold uppercase tracking-wider text-red">
              Rule alerts · {decayingCount}
            </span>
            <button
              type="button"
              onClick={toggleDecayFilter}
              className="text-[11px] font-semibold text-accent hover:underline"
            >
              {effectiveDecayOnly ? 'Show all rules' : 'Show only decaying'}
            </button>
          </div>
          {decaying.map((t) => (
            <Link
              key={t.groupId}
              to={rulesHref(t.groupId)}
              className="text-[11px] text-text-mid hover:text-text hover:underline"
            >
              <span className="font-semibold text-red">▼ {t.label}</span>
              : win{' '}
              <span className="tabular-nums">{t.recent.winRate?.toFixed(0)}%</span>
              {' '}over last {DECAY_WINDOW} vs{' '}
              <span className="tabular-nums">{t.prior.winRate?.toFixed(0)}%</span>
              , and{' '}
              <span className={`tabular-nums ${signedToneClass(t.recent.expectancySol ?? 0)}`}>
                {formatSigned(t.recent.expectancySol ?? 0, 4)}◎
              </span>
              /trade vs{' '}
              <span className="tabular-nums">
                {formatSigned(t.prior.expectancySol ?? 0, 4)}◎
              </span>
            </Link>
          ))}
        </div>
      )}

      {rankedBars.length > 0 && (
        <div className="rounded-lg border border-white/6 bg-bg-panel p-3">
          <h2 className="mb-2 text-[10px] font-bold uppercase tracking-wider text-text-dim">
            Realized PnL by rule
            {effectiveDecayOnly ? ' · decaying only' : ''}
          </h2>
          <RankedPnlBars
            rows={rankedBars}
            selectedKey={selectedRule}
            onSelectRow={(key) => selectRule(selectedRule === key ? null : key)}
            emptyMessage="No closed trades in this window."
          />
        </div>
      )}

      <DataTable
        columns={columns}
        rows={tableRows}
        rowKey={portfolioRuleRowKey}
        loading={isLoading}
        searchable
        defaultSort={{ col: 'pnl', dir: 'desc' }}
        tableId="portfolio-by-rule"
        pinnable
        emptyMessage={
          effectiveDecayOnly
            ? 'No decaying rules in this window.'
            : 'No closed trades in this window.'
        }
        onFilteredRowsChange={onFilteredRowsChange}
        selectedKey={selectedRule}
        onSelect={selectRule}
      />
    </div>
  );
}
