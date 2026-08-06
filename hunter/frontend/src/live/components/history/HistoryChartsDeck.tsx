/**
 * The Console History charts deck — four views of ONE cohort.
 *
 * Every chart folds the same `/api/portfolio/closes-series` payload (B2) that
 * the filter bar's counts come from, so they cannot disagree with each other or
 * with the table below. Only the equity curve pulls `lightweight-charts` (it is
 * the one with a real time axis worth panning), and it is lazy-loaded so the
 * rest of the deck costs nothing extra.
 */

import { Suspense, lazy, memo, useMemo, useState } from 'react';
import { LoadingState } from 'components/ui/LoadingState';
import { PnlCalendar } from 'components/analytics/PnlCalendar';
import { PnlDistribution } from 'components/analytics/PnlDistribution';
import { PnlHeatmap } from 'components/analytics/PnlHeatmap';
import { PnlSparkline } from 'components/analytics/PnlSparkline';
import {
  buildDailyPnl,
  buildEquityCurve,
  buildPnlHeatCells,
  dayKeyInTz,
  groupDailyPnl,
  groupTrends,
  maxDrawdownSol,
  pnlDistributionBuckets,
  type PnlPoint,
} from 'components/analytics/pnlSeries';
import { formatSigned, formatSignedPct, signedToneClass } from 'lib/signedTone';
import { formatDecimalTrim } from 'utils/format';
import { cn } from 'lib/cn';
import type { ClosedTradePoint } from '@live/store/liveEndpoints';

const EquityCurveChart = lazy(() =>
  import('components/analytics/EquityCurveChart').then((m) => ({ default: m.EquityCurveChart })),
);

/** A close from the wire → the deck's neutral point. `pnlPct` is the SOL-basis
 *  percent (`pnl_sol / entry_sol`), matching `pnlPctFromSol`; a zero-cost row
 *  has no percent (not a 0% trade). */
function toPoints(closes: readonly ClosedTradePoint[]): PnlPoint[] {
  return closes.map((c, i) => ({
    key: `${c.exit_time}:${i}`,
    timeMs: Date.parse(c.exit_time),
    pnlSol: c.pnl_sol,
    pnlPct: c.entry_sol > 0 ? (c.pnl_sol / c.entry_sol) * 100 : null,
    label: c.rule_id ?? 'unknown',
    groupId: c.rule_id,
  }));
}

function Card({
  title,
  hint,
  children,
}: {
  title: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex min-w-0 flex-col gap-2 rounded-lg border border-white/6 bg-bg-panel p-3">
      <div className="flex items-baseline justify-between gap-2">
        <h3 className="text-[10px] font-bold uppercase tracking-wider text-text-dim">{title}</h3>
        {hint && <span className="text-[10px] text-text-dim/70">{hint}</span>}
      </div>
      {children}
    </div>
  );
}

export const HistoryChartsDeck = memo(function HistoryChartsDeck({
  closes,
  timezone,
  ruleNameOf,
  loading,
}: {
  closes: readonly ClosedTradePoint[];
  timezone: string;
  ruleNameOf: (id: string | null) => string | null;
  loading: boolean;
}) {
  const [open, setOpen] = useState(true);
  const points = useMemo(() => toPoints(closes), [closes]);

  const curve = useMemo(() => buildEquityCurve(points), [points]);
  const buckets = useMemo(() => pnlDistributionBuckets(points), [points]);
  const heatCells = useMemo(() => buildPnlHeatCells(points, timezone), [points, timezone]);
  const days = useMemo(() => buildDailyPnl(points, timezone), [points, timezone]);
  const perRuleDays = useMemo(() => groupDailyPnl(points, timezone), [points, timezone]);
  const trends = useMemo(
    () => groupTrends(points, (id) => ruleNameOf(id) ?? `${id.slice(0, 8)}…`),
    [points, ruleNameOf],
  );
  const todayKey = useMemo(() => dayKeyInTz(Date.now(), timezone), [timezone]);

  const totalPnl = curve.length ? curve[curve.length - 1]!.cumPnlSol : 0;
  const drawdown = useMemo(() => maxDrawdownSol(curve), [curve]);

  if (!open) {
    return (
      <button
        type="button"
        className="self-start text-[11px] font-semibold uppercase tracking-wider text-text-dim hover:text-text"
        onClick={() => setOpen(true)}
      >
        ▸ Charts
      </button>
    );
  }

  return (
    <div className="flex flex-col gap-2">
      <button
        type="button"
        className="self-start text-[11px] font-semibold uppercase tracking-wider text-text-dim hover:text-text"
        onClick={() => setOpen(false)}
      >
        ▾ Charts
      </button>

      {loading && points.length === 0 ? (
        <LoadingState variant="inline" />
      ) : (
        <div className="grid grid-cols-1 gap-2 xl:grid-cols-2">
          <Card
            title="Equity curve"
            hint={`${formatSigned(totalPnl, 3)} ◎ · max DD ${formatDecimalTrim(drawdown, 3)} ◎`}
          >
            {curve.length === 0 ? (
              <p className="text-xs text-text-dim">No closed trades in this cohort.</p>
            ) : (
              <Suspense fallback={<LoadingState variant="inline" />}>
                <EquityCurveChart points={curve} timezone={timezone} height={220} />
              </Suspense>
            )}
          </Card>

          <Card title="PnL distribution" hint={`${points.length} trades`}>
            <PnlDistribution buckets={buckets} height={220} />
          </Card>

          <Card title="Daily PnL calendar" hint="green = up day">
            <PnlCalendar days={days} todayKey={todayKey} />
          </Card>

          <Card title="When it trades" hint="day × hour, net ◎">
            <PnlHeatmap cells={heatCells} />
          </Card>

          <Card
            title="Rule comparison"
            hint="last 20 vs prior 20 — ▼ = win rate AND expectancy both down"
          >
            {trends.length === 0 ? (
              <p className="text-xs text-text-dim">No per-rule closes in this cohort.</p>
            ) : (
              <div className="flex flex-col gap-1">
                {trends.map((t) => (
                  <div key={t.groupId} className="flex items-center gap-2 text-[11px]">
                    <span className="w-28 shrink-0 truncate text-text-mid" title={t.label}>
                      {t.decaying && (
                        <span className="mr-1 font-bold text-red" title="Decaying">
                          ▼
                        </span>
                      )}
                      {t.label}
                    </span>
                    <PnlSparkline
                      values={(perRuleDays.get(t.groupId) ?? []).map((d) => d.pnlSol)}
                      title={`${t.label} — daily cumulative PnL`}
                    />
                    <span
                      className={cn(
                        'w-16 shrink-0 text-right font-mono tabular-nums',
                        signedToneClass(t.recent.pnlSol),
                      )}
                    >
                      {formatSigned(t.recent.pnlSol, 2)}
                    </span>
                    <span className="w-24 shrink-0 text-right tabular-nums text-text-dim">
                      {t.recent.winRate != null ? `${t.recent.winRate.toFixed(0)}%` : '—'}
                      {t.winRateDeltaPp != null && (
                        <span className={cn('ml-1', signedToneClass(t.winRateDeltaPp))}>
                          {formatSignedPct(t.winRateDeltaPp, 0)}
                        </span>
                      )}
                    </span>
                    <span className="w-10 shrink-0 text-right tabular-nums text-text-dim">
                      n{t.recent.n}
                    </span>
                  </div>
                ))}
              </div>
            )}
          </Card>
        </div>
      )}
    </div>
  );
});
