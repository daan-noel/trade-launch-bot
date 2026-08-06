/**
 * Shared position-summary shell for Evidence / Simulate / Sweep.
 *
 * Layout:
 *   1. Hero + exit mix + focus chips
 *   2. Details bands (always visible)
 *   3. ▾ Charts — one toggle collapsing every chart below it
 *   4. Equity | Return shape (`xl:grid-cols-2`)
 *   5. Hold mix | Wall clock (`xl:grid-cols-2`)
 *   6. Timing: daily calendar (1/3) | when-it-trades heatmap (2/3)
 *   7. Hold vs PnL (full width)
 *
 * The timing charts stay on the table-filter cohort (selection ring); other
 * charts fold the focused slice — same timing-vs-lens split as Console History.
 * Hold vs PnL also keeps its own band lens off its paint cohort (domain zooms
 * instead of emptying the plot), and keeps axes mounted via contextPoints when
 * a cross-chart focus has no scatter rows.
 */

import { Suspense, lazy, useCallback, useMemo, useState, type ReactNode } from 'react';
import { LoadingState } from 'components/ui/LoadingState';
import { InfoTooltip } from 'components/ui/InfoTooltip';
import { PnlDistribution } from 'components/analytics/PnlDistribution';
import { HoldPnlScatter } from 'components/analytics/HoldPnlScatter';
import { PnlCalendar } from 'components/analytics/PnlCalendar';
import { PnlHeatmap } from 'components/analytics/PnlHeatmap';
import {
  EMPTY_PNL_DECK,
  dayKeyInTz,
  foldPnlDeck,
  isPnlDistBucket,
} from 'components/analytics/pnlSeries';
import type { HoldPnlDomain } from 'components/analytics/HoldPnlScatter';
import { usePnlDistDensity } from 'hooks/usePnlDistDensity';
import { useTimezone } from 'context/TimezoneContext';
import { formatDecimalTrim } from 'utils/format';
import { signedToneClass } from 'lib/signedTone';
import { cn } from 'lib/cn';
import {
  activeLensOfKind,
  exitTileToFocusReason,
  filterRowsByFocus,
  isTimeLens,
  removePositionFocus,
  togglePositionFocus,
  type PositionFocusLens,
  type PositionFocusRow,
} from 'lib/strategy/positionFocus';
import {
  runSummarySections,
  type RunSummary,
  type RunSummaryInteraction,
} from 'lib/strategy/runSummary';
import {
  toHoldScatterPoints,
  toPnlPoints,
  type PositionChartPoint,
} from 'lib/strategy/positionChartPoints';
import { SummaryStatsPanel, type SummarySection, type SummaryStat } from './SummaryStatsPanel';
import { PositionFocusChips } from './PositionFocusChips';
import {
  TemporalSummary,
  type TemporalSelection,
  type TemporalSummaryProps,
} from './TemporalSummary';

const EquityCurveChart = lazy(() =>
  import('components/analytics/EquityCurveChart').then((m) => ({ default: m.EquityCurveChart })),
);

/** Stable empties so a collapsed deck never allocates a new cohort per render. */
const NO_POINTS: readonly PositionChartPoint[] = [];

/** One third of the timing row fits ~10 calendar columns before the in-cell SOL
 *  labels truncate to nothing — same budget as Console History. */
const CALENDAR_WEEKS = 10;

export interface PositionSummarySectionProps {
  title: string;
  subtitle?: string;
  onClose?: () => void;
  accentClass?: string;
  /** Shared run summary wire (or client fold). */
  summary: RunSummary;
  /** Extra tiles for `runSummarySections` (matched / migrated / …). */
  summaryExtras?: {
    matched?: number;
    tokensEntered?: number;
    migrated?: number;
    extended?: boolean;
  };
  /** Extra detail sections appended under Details (e.g. Capital — display-only). */
  extraDetailSections?: SummarySection[];
  /** Full-cohort chart points (table filters applied; focus applied here). */
  chartPoints: readonly PositionChartPoint[];
  focus: readonly PositionFocusLens[];
  onFocusChange: (next: PositionFocusLens[]) => void;
  /** Temporal — hold bars + wall timeline. */
  temporal?: Omit<TemporalSummaryProps, 'selection' | 'onSelect'> & {
    /** When temporal is driven by focus hold/wall lenses. */
    enabled?: boolean;
  };
  /** Optional hero override (rare); default from `runSummarySections`. */
  heroOverride?: SummaryStat[];
  className?: string;
  /** Empty-state under charts when cohort has no plottable points. */
  chartsEmptyMessage?: string;
}

function temporalSelectionFromFocus(focus: readonly PositionFocusLens[]): TemporalSelection {
  const hold = activeLensOfKind(focus, 'hold');
  if (hold) return { kind: 'hold', binId: hold.binId, mints: hold.mints };
  const wall = activeLensOfKind(focus, 'wall');
  if (wall) return { kind: 'wall', cellId: wall.cellId, mints: wall.mints };
  return null;
}

function toFocusRow(p: PositionChartPoint): PositionFocusRow & { _p: PositionChartPoint } {
  return {
    id: p.key,
    mint_address: p.mint_address,
    fired: true,
    isOpen: p.isOpen,
    isClosed: !p.isOpen,
    exit_reason: p.exit_reason ?? null,
    pnl_sol: p.pnlSol,
    pnl_pct: p.pnlPct,
    hold_secs: p.holdSeconds,
    is_migrated: p.is_migrated ?? null,
    timeMs: p.timeMs,
    _p: p,
  };
}

/**
 * The one summary chrome above a positions table. Surfaces pass summary metrics
 * + full-cohort chart points + focus state; this owns chip UI and chart → focus.
 */
export function PositionSummarySection({
  title,
  subtitle,
  onClose,
  accentClass,
  summary,
  summaryExtras,
  extraDetailSections = [],
  chartPoints,
  focus,
  onFocusChange,
  temporal,
  heroOverride,
  className,
  chartsEmptyMessage = 'No positions with PnL% in this cohort to plot.',
}: PositionSummarySectionProps) {
  const { timezone } = useTimezone();
  const [density, setDensity] = usePnlDistDensity();
  const [chartsOpen, setChartsOpen] = useState(true);

  const toggleLens = useCallback(
    (lens: PositionFocusLens) => {
      onFocusChange(togglePositionFocus(focus, lens));
    },
    [focus, onFocusChange],
  );

  const interaction: RunSummaryInteraction = useMemo(() => {
    const status = activeLensOfKind(focus, 'status');
    const exit = activeLensOfKind(focus, 'exit');
    const outcome = activeLensOfKind(focus, 'outcome');
    const migrated = activeLensOfKind(focus, 'migrated');
    return {
      status: status?.status ?? null,
      exitLabel: exit
        ? exit.reason === 'TakeProfit'
          ? 'Take profit'
          : exit.reason === 'StopLoss'
            ? 'Stop loss'
            : exit.reason === 'TrailingStop'
              ? 'Trailing'
              : exit.reason === 'TimeStop'
                ? 'Time'
                : exit.reason === 'LiquidityExit'
                  ? 'Liquidity'
                  : exit.reason === 'Metrics'
                    ? 'Metric'
                    : exit.reason
        : null,
      outcome: outcome?.outcome ?? null,
      migrated: migrated?.migrated ?? null,
      onToggleStatus: (s) => toggleLens({ kind: 'status', status: s }),
      onToggleExit: (label) =>
        toggleLens({ kind: 'exit', reason: exitTileToFocusReason(label) }),
      onToggleOutcome: (o) => toggleLens({ kind: 'outcome', outcome: o }),
      onToggleMigrated: (m) => toggleLens({ kind: 'migrated', migrated: m }),
    };
  }, [focus, toggleLens]);

  const { hero, exitMix, sections } = useMemo(
    () =>
      runSummarySections(summary, {
        ...summaryExtras,
        interaction,
      }),
    [summary, summaryExtras, interaction],
  );

  const detailSections = useMemo(() => {
    const cleaned = sections.map((sec) =>
      sec.title === 'Exits' ? { ...sec, lead: undefined } : sec,
    );
    return [...cleaned, ...extraDetailSections];
  }, [sections, extraDetailSections]);

  const matchOpts = useMemo(() => ({ timeZone: timezone }), [timezone]);

  // Calendar + heatmap (timing) stay on the table-filter cohort — only non-timing
  // lenses apply, so an exit/pct stack still narrows the grid while a clicked day
  // or slot is drawn as a selection ring instead of emptying its own chart.
  // A collapsed deck skips the cohort walk entirely (hooks still run).
  const timingPoints = useMemo(() => {
    if (!chartsOpen) return NO_POINTS;
    const nonTiming = focus.filter((l) => !isTimeLens(l));
    if (nonTiming.length === 0) return chartPoints;
    return filterRowsByFocus(chartPoints.map(toFocusRow), nonTiming, matchOpts).map((r) => r._p);
  }, [chartsOpen, chartPoints, focus, matchOpts]);

  const focusedPoints = useMemo(() => {
    if (!chartsOpen) return NO_POINTS;
    if (focus.length === 0) return chartPoints;
    return filterRowsByFocus(chartPoints.map(toFocusRow), focus, matchOpts).map((r) => r._p);
  }, [chartsOpen, chartPoints, focus, matchOpts]);

  // Scatter does not empty itself on a band brush — domain zooms the parent
  // (non-band) cohort; other lenses still narrow the dots.
  const holdSourcePoints = useMemo(() => {
    if (!chartsOpen) return NO_POINTS;
    const nonBand = focus.filter((l) => l.kind !== 'band');
    if (nonBand.length === 0) return chartPoints;
    return filterRowsByFocus(chartPoints.map(toFocusRow), nonBand, matchOpts).map((r) => r._p);
  }, [chartsOpen, chartPoints, focus, matchOpts]);

  const pnlPoints = useMemo(() => toPnlPoints(focusedPoints), [focusedPoints]);
  const holdPoints = useMemo(() => toHoldScatterPoints(holdSourcePoints), [holdSourcePoints]);
  const holdContextPoints = useMemo(
    () => toHoldScatterPoints(chartsOpen ? chartPoints : NO_POINTS),
    [chartsOpen, chartPoints],
  );
  const timingPnlPoints = useMemo(() => toPnlPoints(timingPoints), [timingPoints]);

  const deck = useMemo(
    () =>
      chartsOpen
        ? foldPnlDeck(pnlPoints, {
            timeZone: timezone,
            density,
            labelOf: () => '',
            only: ['curve', 'buckets'],
          })
        : EMPTY_PNL_DECK,
    [chartsOpen, pnlPoints, timezone, density],
  );

  const timingDeck = useMemo(
    () =>
      chartsOpen
        ? foldPnlDeck(timingPnlPoints, {
            timeZone: timezone,
            density,
            labelOf: () => '',
            only: ['heat', 'days'],
          })
        : EMPTY_PNL_DECK,
    [chartsOpen, timingPnlPoints, timezone, density],
  );

  const todayKey = useMemo(() => dayKeyInTz(Date.now(), timezone), [timezone]);

  const pctLens = activeLensOfKind(focus, 'pct');
  const posLens = activeLensOfKind(focus, 'pos');
  const heatLens = activeLensOfKind(focus, 'heat');
  const dayLens = activeLensOfKind(focus, 'day');
  const weekLens = activeLensOfKind(focus, 'week');
  const bandLens = activeLensOfKind(focus, 'band');
  const bandDomain: HoldPnlDomain | null = bandLens
    ? {
        holdLo: bandLens.holdLo,
        holdHi: bandLens.holdHi,
        pctLo: bandLens.pctLo,
        pctHi: bandLens.pctHi,
      }
    : null;

  const temporalSel = temporalSelectionFromFocus(focus);
  const onTemporalSelect = useCallback(
    (sel: TemporalSelection) => {
      if (!sel) {
        onFocusChange(focus.filter((l) => l.kind !== 'hold' && l.kind !== 'wall'));
        return;
      }
      if (sel.kind === 'hold') {
        toggleLens({ kind: 'hold', binId: sel.binId, mints: sel.mints });
      } else {
        toggleLens({ kind: 'wall', cellId: sel.cellId, mints: sel.mints });
      }
    },
    [focus, onFocusChange, toggleLens],
  );

  const temporalEnabled = temporal?.enabled !== false && temporal != null;

  return (
    <div className={className}>
      <SummaryStatsPanel
        title={title}
        subtitle={subtitle}
        onClose={onClose}
        heroStats={heroOverride ?? hero}
        accentClass={accentClass}
      />

      {exitMix && <div className="mb-3 max-w-xl">{exitMix}</div>}

      <PositionFocusChips
        lenses={focus}
        onRemove={(lens) => onFocusChange(removePositionFocus(focus, lens))}
        onClearAll={() => onFocusChange([])}
      />

      {detailSections.length > 0 && (
        <div className="mb-5">
          {detailSections.map((sec) => (
            <div
              key={sec.title}
              className="border-t border-white/6 pt-4 pb-1 first:border-t-0 first:pt-0"
            >
              <div className="flex flex-col gap-2 sm:flex-row sm:items-start sm:gap-6">
                <div className="shrink-0 sm:w-44">
                  <div
                    className={cn(
                      'text-[10px] font-bold uppercase tracking-wider text-text-mid',
                      sec.titleCls,
                    )}
                  >
                    {sec.title}
                  </div>
                  {sec.hint && (
                    <div className="mt-0.5 text-[10px] leading-snug text-text-dim">{sec.hint}</div>
                  )}
                </div>
                <div className="flex flex-1 flex-wrap gap-x-8 gap-y-3">
                  {sec.stats.map((s) => (
                    <DetailStat key={s.label} stat={s} />
                  ))}
                </div>
              </div>
            </div>
          ))}
        </div>
      )}

      <button
        type="button"
        className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-text-dim hover:text-text"
        onClick={() => setChartsOpen((v) => !v)}
      >
        {chartsOpen ? '▾' : '▸'} Charts
      </button>

      {chartsOpen && (
        <>
      {/* Equity | Return */}
      <div className="mb-5 grid grid-cols-1 gap-3 xl:grid-cols-2">
        <ChartCard
          title="Equity path"
          tip={{
            title: 'Cumulative PnL',
            body: 'Running sum of position PnL (open marks included as unrealized). Max DD is the deepest peak-to-trough drop.',
          }}
          hint={
            deck.curve.length > 0 ? (
              <span className={cn('font-mono', signedToneClass(-deck.drawdownSol))}>
                max DD ◎{formatDecimalTrim(deck.drawdownSol, 3)}
              </span>
            ) : null
          }
        >
          {deck.curve.length === 0 ? (
            <p className="text-xs text-text-dim">{chartsEmptyMessage}</p>
          ) : (
            <Suspense fallback={<LoadingState label="Loading equity…" />}>
              <EquityCurveChart points={deck.curve} timezone={timezone} height={220} />
            </Suspense>
          )}
        </ChartCard>

        <ChartCard
          title="Return shape"
          tip={{
            title: 'PnL % distribution',
            body: 'Histogram of return size (open = unrealized %). Click a bar to focus that bucket.',
          }}
        >
          <PnlDistribution
            buckets={deck.buckets}
            height={160}
            emptyMessage={chartsEmptyMessage}
            selected={pctLens ? { lo: pctLens.lo, hi: pctLens.hi } : null}
            onSelectBucket={({ lo, hi }) => {
              if (!isPnlDistBucket(lo, hi)) return;
              toggleLens({ kind: 'pct', lo, hi });
            }}
            density={density}
            onDensityChange={setDensity}
          />
        </ChartCard>
      </div>

      {/* Hold mix | Wall clock — same ChartCard chrome as Equity / Return */}
      {temporalEnabled && temporal ? (
        <TemporalSummary
          variant="deck"
          data={temporal.data}
          rows={temporal.rows}
          linkedData={temporal.linkedData}
          wallField={temporal.wallField}
          onWallFieldChange={temporal.onWallFieldChange}
          wallGrain={temporal.wallGrain}
          onWallGrainChange={temporal.onWallGrainChange}
          holdScheme={temporal.holdScheme}
          onHoldSchemeChange={temporal.onHoldSchemeChange}
          className={temporal.className}
          selection={temporalSel}
          onSelect={onTemporalSelect}
        />
      ) : (
        <div className="mb-5 grid grid-cols-1 gap-3 xl:grid-cols-2">
          <ChartCard
            title="Hold mix"
            tip={{
              title: 'Hold duration',
              body: 'Hold-duration × exit mix. Click a bin to focus that mint set.',
            }}
          >
            <p className="text-xs text-text-dim">No temporal cohort yet.</p>
          </ChartCard>
          <ChartCard
            title="Wall clock"
            tip={{
              title: 'Wall-clock timeline',
              body: 'Entry/create volume over time. Click a cell to focus that mint set.',
            }}
          >
            <p className="text-xs text-text-dim">No temporal cohort yet.</p>
          </ChartCard>
        </div>
      )}

      {/* Timing — the calendar (1/3) and the 24-hour heatmap (2/3) share one card as
          "when did it trade": the heatmap's hour columns need the wide two thirds,
          and below xl they stack so neither is squeezed. */}
      <div className="mb-5 flex flex-col gap-3">
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
                  'One square per calendar day in your timezone (decision instant: exit, or entry for open).\n\n' +
                  'Green = the day netted profit, red = it bled. Brighter fill = larger |PnL|; brighter border = more positions that day, so one lucky close and a long grind look different. Rows run Sun → Sat, columns are weeks (newest right).\n\n' +
                  'Click a day — or a month label for that whole week — to focus. Click again to clear.',
              }}
            >
              <PnlCalendar
                days={timingDeck.days}
                weeks={CALENDAR_WEEKS}
                todayKey={todayKey}
                timeZone={timezone}
                emptyMessage={chartsEmptyMessage}
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
                  'Day-of-week × hour-of-day net SOL in your timezone (decision instant: exit, or entry for open).\n\n' +
                  'Green = net profit in that slot; red = net loss. Click a cell to focus — equity, return, scatter, and the table follow. Click again to clear.',
              }}
            >
              <PnlHeatmap
                cells={timingDeck.heatCells}
                unitLabel="position"
                emptyMessage={chartsEmptyMessage}
                selected={heatLens ? { dow: heatLens.dow, hour: heatLens.hour } : null}
                onSelectCell={(c) => toggleLens({ kind: 'heat', dow: c.dow, hour: c.hour })}
              />
            </ChartPanel>
          </div>
        </ChartCard>

        <ChartCard
          title="Hold vs PnL"
          tip={{
            title: 'Hold vs PnL scatter',
            body:
              'Each point is one position (open uses unrealized %). Drag to zoom a band; click a point to focus it. ' +
              'The plot stays mounted on an empty band or empty cross-chart focus — use Reset scale / the focus chip to clear.',
          }}
        >
          <HoldPnlScatter
            points={holdPoints}
            contextPoints={holdContextPoints}
            height={220}
            emptyMessage={chartsEmptyMessage}
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
      </div>
        </>
      )}
    </div>
  );
}

/** Console History-style chart card. */
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

/** A titled sub-section *inside* a ChartCard — used when two charts share one card
 *  and each still needs its own label + ⓘ (the card title alone can't name both). */
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

function DetailStat({ stat }: { stat: SummaryStat }) {
  const body = (
    <>
      <span className="text-[9px] font-semibold uppercase tracking-wider text-text-dim">
        {stat.label}
      </span>
      <span className={cn('font-mono text-sm font-bold text-text', stat.cls)}>
        {stat.node ?? stat.value}
      </span>
    </>
  );
  if (!stat.onClick) {
    return <div className="flex min-w-[84px] flex-col gap-0.5">{body}</div>;
  }
  return (
    <button
      type="button"
      onClick={stat.onClick}
      className={cn(
        'flex min-w-[84px] flex-col gap-0.5 rounded-md px-1 py-0.5 text-left transition hover:bg-white/5',
        stat.active && 'bg-primary/15 ring-1 ring-primary/50',
      )}
    >
      {body}
    </button>
  );
}

/** Build capital section for Evidence (display-only — never clickable). */
export function capitalDetailSection(stats: SummaryStat[]): SummarySection {
  return {
    title: 'Capital',
    hint: 'SOL deployed by this review (display only)',
    stats,
  };
}

export type { PositionChartPoint, PositionFocusLens, ReactNode };
