/**
 * Wallet-level PnL analytics deck for Trader Analysis.
 *
 * Layout mirrors Console History / Position Summary:
 *   1. Hero KPI tiles + Open/Closed/Win/Loss toggles
 *   2. Focus chip strip
 *   3. ▾ Charts — Equity | Return · Hold vs PnL | Ranked · Timing (calendar + heat)
 *
 * Timing charts stay on the table-filter cohort (selection ring); other charts
 * fold the focused slice. Hold vs PnL keeps its own band zoom on the parent
 * (non-band) cohort and passes contextPoints so axes stay mounted.
 */

import { Suspense, lazy, useCallback, useMemo, useState, type ReactNode } from 'react';
import { LoadingState } from 'components/ui/LoadingState';
import { InfoTooltip } from 'components/ui/InfoTooltip';
import { PnlDistribution } from 'components/analytics/PnlDistribution';
import { HoldPnlScatter } from 'components/analytics/HoldPnlScatter';
import { PnlCalendar } from 'components/analytics/PnlCalendar';
import { PnlHeatmap } from 'components/analytics/PnlHeatmap';
import { RankedPnlBars } from 'components/analytics/RankedPnlBars';
import {
  EMPTY_PNL_DECK,
  dayKeyInTz,
  foldPnlDeck,
  isPnlDistBucket,
} from 'components/analytics/pnlSeries';
import type { HoldPnlDomain } from 'components/analytics/HoldPnlScatter';
import { usePnlDistDensity } from 'hooks/usePnlDistDensity';
import { cn } from 'lib/cn';
import { signedToneClass } from 'lib/signedTone';
import { formatDecimalTrim } from 'utils/format';
import {
  activeLensOfKind,
  isTimeLens,
  removePositionFocus,
  togglePositionFocus,
  type PositionFocusLens,
} from 'lib/strategy/positionFocus';
import { PositionFocusChips } from 'components/strategy/PositionFocusChips';
import { WalletPnlSummaryRow } from './WalletPnlSummary';
import {
  buildHoldScatter,
  computeWalletSummary,
  rankedPnlBarRows,
  toPnlPoints,
} from './walletPnlStats';
import { filterTraderRowsByFocus } from './walletFocus';
import type { TraderTokenRow } from 'types';

/** Shared body height for the Hold vs PnL | Ranked by PnL pair so the ranked
 *  list scrolls instead of stretching the row past the scatter. */
const PAIR_CHART_BODY_H = 280;

const EquityCurveChart = lazy(() =>
  import('components/analytics/EquityCurveChart').then((m) => ({ default: m.EquityCurveChart })),
);

const NO_ROWS: readonly TraderTokenRow[] = [];
const CALENDAR_WEEKS = 10;

interface WalletAnalyticsPanelProps {
  /** Table-filter cohort (column/search only — focus applied here). */
  rows: readonly TraderTokenRow[];
  timezone: string;
  focus: readonly PositionFocusLens[];
  onFocusChange: (next: PositionFocusLens[]) => void;
}

export function WalletAnalyticsPanel({
  rows,
  timezone,
  focus,
  onFocusChange,
}: WalletAnalyticsPanelProps) {
  const [chartsOpen, setChartsOpen] = useState(true);
  const [density, setDensity] = usePnlDistDensity();

  const toggleLens = useCallback(
    (lens: PositionFocusLens) => {
      onFocusChange(togglePositionFocus(focus, lens));
    },
    [focus, onFocusChange],
  );

  const matchOpts = useMemo(() => ({ timeZone: timezone }), [timezone]);

  // Summary tracks the focused slice so KPIs match the table under a lens.
  const summaryRows = useMemo(
    () => (focus.length === 0 ? rows : filterTraderRowsByFocus(rows, focus, matchOpts)),
    [rows, focus, matchOpts],
  );
  const summary = useMemo(() => computeWalletSummary(summaryRows), [summaryRows]);

  const statusLens = activeLensOfKind(focus, 'status');
  const outcomeLens = activeLensOfKind(focus, 'outcome');
  const pctLens = activeLensOfKind(focus, 'pct');
  const posLens = activeLensOfKind(focus, 'pos');
  const heatLens = activeLensOfKind(focus, 'heat');
  const dayLens = activeLensOfKind(focus, 'day');
  const weekLens = activeLensOfKind(focus, 'week');
  const bandLens = activeLensOfKind(focus, 'band');

  // Timing (calendar + heatmap): non-timing lenses narrow the grid; the active
  // time lens is a selection ring — same split as Console / Position Summary.
  const timingRows = useMemo(() => {
    if (!chartsOpen) return NO_ROWS;
    const nonTiming = focus.filter((l) => !isTimeLens(l));
    if (nonTiming.length === 0) return rows;
    return filterTraderRowsByFocus(rows, nonTiming, matchOpts);
  }, [chartsOpen, rows, focus, matchOpts]);

  const focusedRows = useMemo(() => {
    if (!chartsOpen) return NO_ROWS;
    if (focus.length === 0) return rows;
    return filterTraderRowsByFocus(rows, focus, matchOpts);
  }, [chartsOpen, rows, focus, matchOpts]);

  // Scatter: band zoom keeps parent dots (domain clips); other lenses refold.
  const holdSourceRows = useMemo(() => {
    if (!chartsOpen) return NO_ROWS;
    const nonBand = focus.filter((l) => l.kind !== 'band');
    if (nonBand.length === 0) return rows;
    return filterTraderRowsByFocus(rows, nonBand, matchOpts);
  }, [chartsOpen, rows, focus, matchOpts]);

  const lensDeck = useMemo(
    () =>
      chartsOpen
        ? foldPnlDeck(toPnlPoints(focusedRows), {
            timeZone: timezone,
            density,
            labelOf: () => '',
            only: ['curve', 'buckets'],
          })
        : EMPTY_PNL_DECK,
    [chartsOpen, focusedRows, timezone, density],
  );

  const timingDeck = useMemo(
    () =>
      chartsOpen
        ? foldPnlDeck(toPnlPoints(timingRows), {
            timeZone: timezone,
            density,
            labelOf: () => '',
            only: ['heat', 'days'],
          })
        : EMPTY_PNL_DECK,
    [chartsOpen, timingRows, timezone, density],
  );

  const holdPoints = useMemo(
    () => (chartsOpen ? buildHoldScatter(holdSourceRows) : []),
    [chartsOpen, holdSourceRows],
  );
  const holdContextPoints = useMemo(
    () => (chartsOpen ? buildHoldScatter(rows) : []),
    [chartsOpen, rows],
  );
  const rankedBars = useMemo(
    () => (chartsOpen ? rankedPnlBarRows(focusedRows) : []),
    [chartsOpen, focusedRows],
  );

  const todayKey = useMemo(() => dayKeyInTz(Date.now(), timezone), [timezone]);
  const bandDomain: HoldPnlDomain | null = bandLens
    ? {
        holdLo: bandLens.holdLo,
        holdHi: bandLens.holdHi,
        pctLo: bandLens.pctLo,
        pctHi: bandLens.pctHi,
      }
    : null;

  const onDensityChange = (next: typeof density) => {
    setDensity(next);
    if (pctLens && !isPnlDistBucket(pctLens.lo, pctLens.hi, next)) {
      onFocusChange(focus.filter((l) => l.kind !== 'pct'));
    }
  };

  if (rows.length === 0) return null;

  const emptyLens = 'No tokens in this focus.';
  const emptyCohort = 'No tokens with PnL to plot in this cohort.';
  const chartsEmpty = focus.length > 0 ? emptyLens : emptyCohort;

  return (
    <div className="mb-4 flex flex-col gap-3">
      <WalletPnlSummaryRow
        summary={summary}
        status={statusLens?.status === 'open' || statusLens?.status === 'closed' ? statusLens.status : null}
        outcome={outcomeLens?.outcome ?? null}
        onToggleStatus={(s) => toggleLens({ kind: 'status', status: s })}
        onToggleOutcome={(o) => toggleLens({ kind: 'outcome', outcome: o })}
      />

      <PositionFocusChips
        lenses={focus}
        onRemove={(lens) => onFocusChange(removePositionFocus(focus, lens))}
        onClearAll={() => onFocusChange([])}
        className="mb-0"
      />

      <button
        type="button"
        className="self-start text-[11px] font-semibold uppercase tracking-wider text-text-dim hover:text-text"
        onClick={() => setChartsOpen((v) => !v)}
      >
        {chartsOpen ? '▾' : '▸'} Charts
      </button>

      {chartsOpen && (
        <div className="grid grid-cols-1 gap-3 xl:grid-cols-2">
          <ChartCard
            title="Equity path"
            tip={{
              title: 'Cumulative PnL',
              body:
                'Running sum of mark-to-market PnL per mint (ordered by each mint\'s most-recent trade). ' +
                'Max DD is the deepest peak-to-trough drop. Per-mint grain — re-entries on one mint collapse to one step.',
            }}
            hint={
              lensDeck.curve.length > 0 ? (
                <span className={cn('font-mono', signedToneClass(-lensDeck.drawdownSol))}>
                  max DD ◎{formatDecimalTrim(lensDeck.drawdownSol, 3)}
                </span>
              ) : null
            }
          >
            {lensDeck.curve.length === 0 ? (
              <p className="text-xs text-text-dim">{chartsEmpty}</p>
            ) : (
              <Suspense fallback={<LoadingState variant="inline" label="Loading chart…" />}>
                <EquityCurveChart points={lensDeck.curve} timezone={timezone} height={220} />
              </Suspense>
            )}
          </ChartCard>

          <ChartCard
            title="Return shape"
            tip={{
              title: 'PnL % distribution',
              body: 'Histogram of realized round-trip returns. Open-only bags (no matched cost basis) are excluded. Click a bar to focus that bucket.',
            }}
            hint={`${focusedRows.length} token${focusedRows.length === 1 ? '' : 's'}`}
          >
            <PnlDistribution
              buckets={lensDeck.buckets}
              height={220}
              emptyMessage={chartsEmpty}
              density={density}
              onDensityChange={onDensityChange}
              selected={pctLens ? { lo: pctLens.lo, hi: pctLens.hi } : null}
              onSelectBucket={({ lo, hi }) => {
                if (!isPnlDistBucket(lo, hi)) return;
                toggleLens({ kind: 'pct', lo, hi });
              }}
            />
          </ChartCard>

          <div className="grid grid-cols-1 items-start gap-3 xl:col-span-2 xl:grid-cols-2">
            <ChartCard
              title="Hold vs PnL"
              tip={{
                title: 'Hold vs PnL scatter',
                body:
                  'Each point is one mint: X = first→last trade span in the window (not a single episode), Y = realized PnL%. ' +
                  'Drag to zoom a band; click a point to focus that mint. Reset scale / the focus chip clears.',
              }}
              hint={`${holdPoints.length} mint${holdPoints.length === 1 ? '' : 's'} · click to focus`}
            >
              <HoldPnlScatter
                points={holdPoints}
                contextPoints={holdContextPoints}
                height={PAIR_CHART_BODY_H}
                emptyMessage={chartsEmpty}
                selectedKey={posLens?.positionId ?? null}
                onSelectPoint={(key) => toggleLens({ kind: 'pos', positionId: key })}
                domain={bandDomain}
                onDomainChange={(d) => {
                  if (!d) {
                    onFocusChange(focus.filter((l) => l.kind !== 'band'));
                    return;
                  }
                  toggleLens({
                    kind: 'band',
                    holdLo: d.holdLo,
                    holdHi: d.holdHi,
                    pctLo: d.pctLo,
                    pctHi: d.pctHi,
                  });
                }}
              />
            </ChartCard>

            <ChartCard
              title="Ranked by PnL"
              tip={{
                title: 'Best → worst mint',
                body: 'Ranked on mark-to-market total PnL, not win rate. Click a row to focus that mint across charts + table.',
              }}
              hint="click a row to focus"
            >
              <RankedPnlBars
                rows={rankedBars}
                maxEachSide={15}
                maxHeight={PAIR_CHART_BODY_H}
                emptyMessage={chartsEmpty}
                selectedKey={posLens?.positionId ?? null}
                onSelectRow={(key) => toggleLens({ kind: 'pos', positionId: key })}
              />
            </ChartCard>
          </div>

          <div className="xl:col-span-2">
            <ChartCard
              title="Timing"
              hint={<span className="text-[10px] text-text-dim/70">click a day or slot to focus</span>}
            >
              <div className="grid grid-cols-1 items-start gap-4 xl:grid-cols-3">
                <ChartPanel
                  title="Daily PnL"
                  tip={{
                    title: 'Daily PnL calendar',
                    body:
                      'One square per calendar day in your timezone (bucketed by each mint\'s most-recent trade).\n\n' +
                      'Green = the day netted profit, red = it bled. Brighter fill = larger |PnL|; brighter border = more mints that day. ' +
                      'Click a day — or a month label for that whole week — to focus. Click again to clear.',
                  }}
                >
                  <PnlCalendar
                    days={timingDeck.days}
                    weeks={CALENDAR_WEEKS}
                    todayKey={todayKey}
                    timeZone={timezone}
                    emptyMessage={chartsEmpty}
                    selectedDay={dayLens?.day ?? null}
                    onSelectDay={(day) => toggleLens({ kind: 'day', day })}
                    selectedWeek={weekLens?.weekStart ?? null}
                    onSelectWeek={(weekStart) => toggleLens({ kind: 'week', weekStart })}
                  />
                </ChartPanel>

                <ChartPanel
                  title="When it trades"
                  className="xl:col-span-2 xl:border-l xl:border-white/6 xl:pl-4"
                  tip={{
                    title: 'Dow × hour heatmap',
                    body:
                      'Day-of-week × hour-of-day net SOL in your timezone (per mint\'s most-recent trade).\n\n' +
                      'Green = net profit in that slot; red = net loss. A cell counts mints decided then, not individual trades. ' +
                      'Click to focus — equity, return, scatter, ranked, and the table follow.',
                  }}
                >
                  <PnlHeatmap
                    cells={timingDeck.heatCells}
                    unitLabel="token"
                    emptyMessage={chartsEmpty}
                    selected={heatLens ? { dow: heatLens.dow, hour: heatLens.hour } : null}
                    onSelectCell={(c) => toggleLens({ kind: 'heat', dow: c.dow, hour: c.hour })}
                  />
                </ChartPanel>
              </div>
            </ChartCard>
          </div>
        </div>
      )}
    </div>
  );
}

function ChartCard({
  title,
  tip,
  hint,
  children,
}: {
  title: string;
  tip?: { title: string; body: string };
  hint?: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className="flex min-w-0 flex-col gap-2 rounded-lg border border-white/6 bg-bg-panel p-3">
      <div className="flex items-baseline justify-between gap-2">
        <h3 className="inline-flex items-center gap-1 text-[10px] font-bold uppercase tracking-wider text-text-dim">
          {title}
          {tip && <InfoTooltip title={tip.title} body={tip.body} />}
        </h3>
        {hint}
      </div>
      {children}
    </div>
  );
}

function ChartPanel({
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
