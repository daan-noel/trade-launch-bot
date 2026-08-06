/**
 * The Console History charts deck — several views of ONE cohort.
 *
 * Every chart folds the same `/api/portfolio/closes-series` payload (B2) that
 * the filter bar's counts come from, so they cannot disagree with each other or
 * with the table below. Only the equity curve pulls `lightweight-charts` (it is
 * the one with a real time axis worth panning), and it is lazy-loaded so the
 * rest of the deck costs nothing extra.
 *
 * Clicking a calendar day / heat cell / distribution bar / rule row sets a
 * URL-backed `focus` lens: charts stay on the parent cohort (selection ring),
 * the table below narrows to that slice.
 */

import { Suspense, lazy, memo, useMemo, useState, type ReactNode } from 'react';
import { LoadingState } from 'components/ui/LoadingState';
import { InfoTooltip } from 'components/ui/InfoTooltip';
import { HoldPnlScatter } from 'components/analytics/HoldPnlScatter';
import { PnlCalendar } from 'components/analytics/PnlCalendar';
import { PnlDistribution } from 'components/analytics/PnlDistribution';
import { PnlHeatmap } from 'components/analytics/PnlHeatmap';
import { PnlSparkline } from 'components/analytics/PnlSparkline';
import {
  buildDailyPnl,
  buildEquityCurve,
  buildHoldScatterPoints,
  buildPnlHeatCells,
  dayKeyInTz,
  groupDailyPnl,
  groupTrends,
  isPnlDistBucket,
  maxDrawdownSol,
  pnlDistributionBuckets,
  type PnlPoint,
} from 'components/analytics/pnlSeries';
import {
  formatSigned,
  formatSignedPct,
  signedToneClass,
  winRateGradeClass,
} from 'lib/signedTone';
import { usePnlDistDensity } from 'hooks/usePnlDistDensity';
import { formatDecimalTrim } from 'utils/format';
import { cn } from 'lib/cn';
import type { ClosedTradePoint } from '@live/store/liveEndpoints';
import {
  toggleHistoryFocus,
  type HistoryFocus,
} from '@live/pages/console/historyFocus';

const EquityCurveChart = lazy(() =>
  import('components/analytics/EquityCurveChart').then((m) => ({ default: m.EquityCurveChart })),
);

/** ⓘ copy for each card — plain language, same cohort as the table below. */
const CHART_HELP = {
  equity: {
    title: 'Equity curve',
    body:
      'Running total of realized SOL profit/loss from closed trades, oldest → newest.\n\n' +
      '• Goes up when you close winners, down when you close losers.\n' +
      '• Max DD (drawdown) is the deepest drop from a previous peak — how far you fell from your best point.\n\n' +
      'Only includes closes that match the History filters above (date, rule, mode, …).',
  },
  distribution: {
    title: 'PnL distribution',
    body:
      'How many closed trades landed in each return-size bucket.\n\n' +
      '• Return = PnL ÷ entry size (percent of SOL you put in).\n' +
      '• Left = losers, right = winners (open tail: 50 / 100 / 200 / ≥500%). Bar height = trade count.\n' +
      '• Color follows magnitude grade: deep red for big losses, blue for small wins, green / gold / orange for larger wins.\n' +
      '• Sparse / Default / Dense changes how finely the zone around 0% is sliced (preference is remembered).\n\n' +
      'Click a bar to show only those positions in the table. Click again to clear.',
  },
  calendar: {
    title: 'Daily PnL calendar',
    body:
      'One square per calendar day in your selected timezone — like a contribution graph.\n\n' +
      '• Green = that day made money overall; red = that day lost. Brighter fill = larger |PnL|.\n' +
      '• Brighter BORDER = more trades that day, so one lucky close and a long grind look different.\n' +
      '• Columns are weeks (newest right), labeled by month. Rows run Sun (top) → Sat (bottom); weekend cells are dashed. Today has a white outline.\n' +
      '• The line underneath is the part color cannot show: how often you finish green, your best and worst day, and the longest run of consecutive losing days.\n\n' +
      'Click a day to show only that day in the table; click the month label above a column to show that whole week. Click again to clear.',
  },
  heatmap: {
    title: 'When it trades',
    body:
      'Day-of-week × hour-of-day grid of net SOL (your timezone).\n\n' +
      '• Each cell sums PnL of closes that exited in that weekday + hour slot.\n' +
      '• Green cell = that slot is profitable on net; red = net loser.\n' +
      '• Empty / near-empty cells mean few or no closes then.\n\n' +
      'Click a cell to show only closes in that recurring slot (still inside the date filters). Click again to clear.',
  },
  rules: {
    title: 'Rule comparison',
    body:
      'Per-rule recent form vs the stretch before it.\n\n' +
      '• Sparkline = daily PnL shape for that rule.\n' +
      '• Number = net SOL on the last 20 closes.\n' +
      '• Win% = hit rate on those 20, with the change vs the prior 20 in parentheses.\n' +
      '• n = how many closes are in the recent window.\n\n' +
      '▼ means the rule is decaying: both win rate AND average SOL per trade fell vs the prior window.\n\n' +
      'Click a row to show only that rule in the table (charts stay on the full cohort). Click again to clear.',
  },
  hold: {
    title: 'Hold vs PnL',
    body:
      'Each closed trade as a point: how long you held (X, log scale) vs return % (Y).\n\n' +
      '• Green = winner, red = loser. Dot size = entry SOL.\n' +
      '• Cluster on the left = money from fast scalps; on the right = longer rides.\n' +
      '• Drag a rectangle to zoom that band and show only those positions in the table.\n' +
      '• Click a point for one position. Double-click or Reset scale to clear the zoom.',
  },
} as const;

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

function toHoldRows(
  closes: readonly ClosedTradePoint[],
  ruleNameOf: (id: string | null) => string | null,
) {
  return closes.map((c) => ({
    key: c.id,
    label:
      (c.rule_id ? ruleNameOf(c.rule_id) : null) ??
      c.mint_address.slice(0, 8) + '…',
    holdSeconds: c.hold_secs,
    pnlPct: c.entry_sol > 0 ? (c.pnl_sol / c.entry_sol) * 100 : null,
    sizeSol: c.entry_sol,
    pnlSol: c.pnl_sol,
    isWin: c.win,
  }));
}

function Card({
  title,
  hint,
  tip,
  children,
}: {
  title: string;
  hint?: string;
  tip?: { title: string; body: string };
  children: ReactNode;
}) {
  return (
    <div className="flex min-w-0 flex-col gap-2 rounded-lg border border-white/6 bg-bg-panel p-3">
      <div className="flex items-baseline justify-between gap-2">
        <h3 className="inline-flex items-center gap-1 text-[10px] font-bold uppercase tracking-wider text-text-dim">
          {title}
          {tip && <InfoTooltip title={tip.title} body={tip.body} />}
        </h3>
        {hint && <span className="text-[10px] text-text-dim/70">{hint}</span>}
      </div>
      {children}
    </div>
  );
}

/** A titled sub-section *inside* a Card — used when two charts share one card and
 *  each still needs its own label + ⓘ (the card title alone can't name both). */
function Panel({
  title,
  tip,
  className,
  children,
}: {
  title: string;
  tip?: { title: string; body: string };
  className?: string;
  children: ReactNode;
}) {
  return (
    <section className={cn('flex min-w-0 flex-col gap-2', className)}>
      <h4 className="inline-flex items-center gap-1 text-[10px] font-semibold uppercase tracking-wider text-text-dim/80">
        {title}
        {tip && <InfoTooltip title={tip.title} body={tip.body} />}
      </h4>
      {children}
    </section>
  );
}

export const HistoryChartsDeck = memo(function HistoryChartsDeck({
  closes,
  timezone,
  ruleNameOf,
  loading,
  focus,
  onFocusChange,
}: {
  closes: readonly ClosedTradePoint[];
  timezone: string;
  ruleNameOf: (id: string | null) => string | null;
  loading: boolean;
  focus: HistoryFocus | null;
  onFocusChange: (focus: HistoryFocus | null) => void;
}) {
  const [open, setOpen] = useState(true);
  const [density, setDensity] = usePnlDistDensity();
  const points = useMemo(() => toPoints(closes), [closes]);

  const curve = useMemo(() => buildEquityCurve(points), [points]);
  const buckets = useMemo(() => pnlDistributionBuckets(points, density), [points, density]);
  const heatCells = useMemo(() => buildPnlHeatCells(points, timezone), [points, timezone]);
  const days = useMemo(() => buildDailyPnl(points, timezone), [points, timezone]);
  const perRuleDays = useMemo(() => groupDailyPnl(points, timezone), [points, timezone]);
  const trends = useMemo(
    () => groupTrends(points, (id) => ruleNameOf(id) ?? `${id.slice(0, 8)}…`),
    [points, ruleNameOf],
  );
  const holdPoints = useMemo(
    () => buildHoldScatterPoints(toHoldRows(closes, ruleNameOf)),
    [closes, ruleNameOf],
  );
  const todayKey = useMemo(() => dayKeyInTz(Date.now(), timezone), [timezone]);

  const totalPnl = curve.length ? curve[curve.length - 1]!.cumPnlSol : 0;
  const drawdown = useMemo(() => maxDrawdownSol(curve), [curve]);

  const setFocus = (next: HistoryFocus) => onFocusChange(toggleHistoryFocus(focus, next));

  const onDensityChange = (next: typeof density) => {
    setDensity(next);
    // Drop pct focus only when the active bar isn't on the new grid (table lens
    // would otherwise point at a bucket the histogram no longer draws).
    if (focus?.kind === 'pct' && !isPnlDistBucket(focus.lo, focus.hi, next)) {
      onFocusChange(null);
    }
  };

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
        <div className="grid grid-cols-1 gap-3 xl:grid-cols-2">
          <Card
            title="Equity curve"
            tip={CHART_HELP.equity}
            hint={`${formatSigned(totalPnl, 3)} ◎ · max DD ${formatDecimalTrim(drawdown, 3)} ◎`}
          >
            {curve.length === 0 ? (
              <p className="text-xs text-text-dim">No closed trades in this cohort.</p>
            ) : (
              <Suspense fallback={<LoadingState variant="inline" />}>
                <EquityCurveChart points={curve} timezone={timezone} height={280} />
              </Suspense>
            )}
          </Card>

          <Card title="PnL distribution" tip={CHART_HELP.distribution} hint={`${points.length} trades`}>
            <PnlDistribution
              buckets={buckets}
              height={280}
              density={density}
              onDensityChange={onDensityChange}
              selected={
                focus?.kind === 'pct' && isPnlDistBucket(focus.lo, focus.hi, density)
                  ? { lo: focus.lo, hi: focus.hi }
                  : null
              }
              onSelectBucket={(b) => setFocus({ kind: 'pct', lo: b.lo, hi: b.hi })}
            />
          </Card>


          <Card
            title="Hold vs PnL"
            tip={CHART_HELP.hold}
            hint={`${holdPoints.length} closes · click a point to focus`}
          >
            <HoldPnlScatter
              points={holdPoints}
              height={300}
              selectedKey={focus?.kind === 'pos' ? focus.positionId : null}
              onSelectPoint={(positionId) => setFocus({ kind: 'pos', positionId })}
              domain={
                focus?.kind === 'holdBand'
                  ? {
                    holdLo: focus.holdLo,
                    holdHi: focus.holdHi,
                    pctLo: focus.pctLo,
                    pctHi: focus.pctHi,
                  }
                  : null
              }
              onDomainChange={(d) => {
                if (!d) onFocusChange(null);
                else
                  onFocusChange({
                    kind: 'holdBand',
                    holdLo: d.holdLo,
                    holdHi: d.holdHi,
                    pctLo: d.pctLo,
                    pctHi: d.pctHi,
                  });
              }}
            />
          </Card>

          <Card title="Rule comparison" tip={CHART_HELP.rules} hint="click a row to focus">
            {trends.length === 0 ? (
              <p className="text-xs text-text-dim">No per-rule closes in this cohort.</p>
            ) : (
              <div className="flex flex-col gap-1">
                {trends.map((t) => {
                  const selected = focus?.kind === 'rule' && focus.ruleId === t.groupId;
                  return (
                    <button
                      key={t.groupId}
                      type="button"
                      title="Click to focus table on this rule"
                      onClick={() => setFocus({ kind: 'rule', ruleId: t.groupId })}
                      className={cn(
                        'flex items-center gap-2 rounded-sm border border-transparent px-1 py-0.5 text-left text-[11px] hover:border-white/15 hover:bg-white/3',
                        selected && 'border-primary/40 bg-primary/8 ring-1 ring-primary/50',
                      )}
                    >
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
                      <span className="w-24 shrink-0 text-right tabular-nums">
                        {t.recent.winRate != null ? (
                          <span className={winRateGradeClass(t.recent.winRate / 100)}>
                            {t.recent.winRate.toFixed(0)}%
                          </span>
                        ) : (
                          <span className="text-text-dim">—</span>
                        )}
                        {t.winRateDeltaPp != null && (
                          <span className={cn('ml-1', signedToneClass(t.winRateDeltaPp))}>
                            {formatSignedPct(t.winRateDeltaPp, 0)}
                          </span>
                        )}
                      </span>
                      <span className="w-10 shrink-0 text-right tabular-nums text-text-dim">
                        n{t.recent.n}
                      </span>
                    </button>
                  );
                })}
              </div>
            )}
          </Card>

          {/* Full-width: the calendar (1/3) and the 24-hour heatmap (2/3) share one
              card as "when did it trade" — the heatmap's hour columns need the wide
              two-thirds, and below xl they stack so neither is squeezed. */}
          <div className="xl:col-span-2">
            <Card title="Timing" hint="click a day or slot to focus">
              <div className="grid grid-cols-1 items-start gap-4 xl:grid-cols-3">
                <Panel title="Daily PnL calendar" tip={CHART_HELP.calendar}>
                  <PnlCalendar
                    days={days}
                    /* One third of the row fits ~10 columns before the in-cell SOL
                       labels truncate to nothing. */
                    weeks={10}
                    todayKey={todayKey}
                    timeZone={timezone}
                    selectedDay={focus?.kind === 'day' ? focus.day : null}
                    onSelectDay={(day) => setFocus({ kind: 'day', day })}
                    selectedWeek={focus?.kind === 'week' ? focus.weekStart : null}
                    onSelectWeek={(weekStart) => setFocus({ kind: 'week', weekStart })}
                  />
                </Panel>
                <Panel
                  title="When it trades"
                  tip={CHART_HELP.heatmap}
                  className="xl:col-span-2 xl:border-l xl:border-white/6 xl:pl-4"
                >
                  <PnlHeatmap
                    cells={heatCells}
                    selected={focus?.kind === 'heat' ? { dow: focus.dow, hour: focus.hour } : null}
                    onSelectCell={(c) => setFocus({ kind: 'heat', dow: c.dow, hour: c.hour })}
                  />
                </Panel>
              </div>
            </Card>
          </div>
        </div>
      )}
    </div>
  );
});
