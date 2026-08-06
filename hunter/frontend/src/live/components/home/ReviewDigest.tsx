/**
 * The "I just came back" strip on Home.
 *
 * The Console lanes answer *what needs doing right now*; this answers *what
 * happened while I was away* — a week of realized PnL as a sparkline, today's
 * money, whether anything needs attention, and the one thing a reviewer must
 * not miss: a rule whose recent trades got worse than its prior ones.
 *
 * Everything here folds the same `/api/portfolio/closes-series` payload the
 * Console History deck and the Portfolio scoreboard use — one definition of
 * "decaying", three places it can surface.
 */

import { memo, useMemo } from 'react';
import { Link } from 'react-router-dom';
import { useSelector } from 'react-redux';
import { PnlSparkline } from 'components/analytics/PnlSparkline';
import {
  buildDailyPnl,
  groupTrends,
  type PnlPoint,
} from 'components/analytics/pnlSeries';
import { useTimezone } from 'context/TimezoneContext';
import { formatSigned, signedToneClass } from 'lib/signedTone';
import { consoleHref, consoleHistoryHref, rulesHref } from 'lib/strategy/nav';
import { useGetStrategyRulesQuery } from 'store/sharedEndpoints';
import { useGetPortfolioClosesSeriesQuery } from '@live/store/liveEndpoints';
import { ATTENTION_STATUSES, selectLiveOpen } from '@live/slices/liveStatusSlice';

/** Same window as the Portfolio decay column — one definition, one verdict. */
const DECAY_WINDOW = 20;

export const ReviewDigest = memo(function ReviewDigest() {
  const { timezone } = useTimezone();
  const { data: series } = useGetPortfolioClosesSeriesQuery({ range: '7d', mode: 'real' });
  const { data: rules = [] } = useGetStrategyRulesQuery();
  const openMap = useSelector(selectLiveOpen);

  const attention = useMemo(
    () =>
      Object.values(openMap).filter(
        (r) => ATTENTION_STATUSES.has(r.status) || (r.status === 'BuySubmitted' && r.needsReview),
      ).length,
    [openMap],
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

  const days = useMemo(() => buildDailyPnl(points, timezone), [points, timezone]);
  const weekPnl = useMemo(() => days.reduce((s, d) => s + d.pnlSol, 0), [days]);

  const decaying = useMemo(() => {
    const nameOf = (id: string) =>
      rules.find((r) => r.id === id)?.rule_name ?? `${id.slice(0, 8)}…`;
    return groupTrends(points, nameOf, DECAY_WINDOW).filter((t) => t.decaying);
  }, [points, rules]);

  return (
    <div className="flex flex-col gap-2 rounded-lg border border-white/6 bg-bg-panel p-3">
      <div className="flex flex-wrap items-center gap-x-5 gap-y-2">
        <div className="flex items-center gap-2">
          <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">
            Last 7 days
          </span>
          <PnlSparkline
            values={days.map((d) => d.pnlSol)}
            width={110}
            height={22}
            title="Cumulative realized PnL, last 7 days"
          />
          <span className={`tabular-nums text-sm font-semibold ${signedToneClass(weekPnl)}`}>
            {formatSigned(weekPnl, 3)}◎
          </span>
        </div>

        <Link
          to={consoleHistoryHref({ range: '7d', mode: 'real' })}
          className="text-[11px] font-semibold text-accent hover:underline"
        >
          Review trades →
        </Link>

        {attention > 0 && (
          <Link
            to={consoleHref({ mode: 'real' })}
            className="rounded-md bg-warning/15 px-2 py-1 text-[11px] font-bold text-warning hover:bg-warning/25"
          >
            ⚠ {attention} need{attention === 1 ? 's' : ''} attention
          </Link>
        )}

        {series != null && series.entry_failed > 0 && (
          <span
            className="text-[11px] text-text-dim"
            title="Buys that never filled in this window — no SOL deployed"
          >
            {series.entry_failed} entry-failed
          </span>
        )}
      </div>

      {decaying.length > 0 && (
        <div className="flex flex-col gap-1 border-t border-white/6 pt-2">
          <span className="text-[10px] font-bold uppercase tracking-wider text-red">
            Rule alerts
          </span>
          {decaying.map((t) => (
            <Link
              key={t.groupId}
              to={rulesHref(t.groupId)}
              className="text-[11px] text-text-mid hover:text-text hover:underline"
            >
              <span className="font-semibold text-red">▼ {t.label}</span>: win rate{' '}
              <span className="tabular-nums">{t.recent.winRate?.toFixed(0)}%</span> over the last{' '}
              {DECAY_WINDOW} vs{' '}
              <span className="tabular-nums">{t.prior.winRate?.toFixed(0)}%</span> before, and{' '}
              <span className={`tabular-nums ${signedToneClass(t.recent.expectancySol ?? 0)}`}>
                {formatSigned(t.recent.expectancySol ?? 0, 4)}◎
              </span>{' '}
              per trade vs{' '}
              <span className="tabular-nums">{formatSigned(t.prior.expectancySol ?? 0, 4)}◎</span>
            </Link>
          ))}
        </div>
      )}
    </div>
  );
});
