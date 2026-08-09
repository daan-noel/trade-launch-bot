/**
 * Console History's **summary strip** — what the filtered positions add up to,
 * directly above the table that filters them.
 *
 * Renders through the shared `runSummarySections`, so a live book reports the
 * same metrics under the same labels as a simulate or a sweep, and the Exits
 * band (rendered separately by `HistoryExitSummary`) is dropped here rather than
 * duplicated.
 *
 * **Two sources, one shape.** Normally the numbers are the server aggregate over
 * the whole filtered cohort — exact past the page and past the chart-walk cap,
 * with no rows shipped. But a heat / hold-band / Metric± lens can't be expressed
 * in SQL, so under one of those the aggregate would count rows the surfaces have
 * lensed away; there the section hands down a client fold of the focused slice
 * and it wins. The two paths meet at `RunSummary`, so only the provenance line
 * and the (unmeasurable) Migrated tile differ.
 *
 * Tiles are lenses: Fired / Closed / Open write the status channel, Win% /
 * Worst% the outcome channel, Migrated its own — each composing with the chart
 * focus rather than replacing it.
 */

import { memo, useMemo } from 'react';
import { cn } from 'lib/cn';
import { SummaryStatsPanel, type SummaryStat } from 'components/strategy/SummaryStatsPanel';
import { toRunSummaryFromPositions } from 'components/strategy/SimSummaryCard';
import {
  runSummarySections,
  type RunSummary,
  type RunSummaryInteraction,
} from 'lib/strategy/runSummary';
import type { PositionsSummary } from 'types';
import type {
  HistoryCohortApi,
  HistoryLane,
  HistoryOutcome,
} from '@live/pages/console/historyCohort';

export const HistorySummaryStrip = memo(function HistorySummaryStrip({
  cohort,
  summary,
  clientRun,
  loading,
  scanCapped,
}: {
  cohort: HistoryCohortApi;
  /** Server aggregate over the filtered cohort; `null` until first load. */
  summary: PositionsSummary | null;
  /** Client fold of the focused slice — non-null exactly when a lens SQL can't
   *  express is active, and then it **wins** over the aggregate. */
  clientRun: RunSummary | null;
  loading: boolean;
  /** The cohort walk hit its row cap — a client fold is then a lower bound. */
  scanCapped: boolean;
}) {
  const run: RunSummary | null = useMemo(
    () => clientRun ?? (summary ? toRunSummaryFromPositions(summary) : null),
    [clientRun, summary],
  );

  // Omitted on the client path: a closes-only fold has no migration flag to
  // count, and a `0` there would read as "none migrated" rather than as "this
  // path doesn't measure it" — `runSummarySections` hides the tile instead.
  const migrated = clientRun ? undefined : (summary?.migrated ?? undefined);

  const interaction: RunSummaryInteraction = useMemo(
    () => ({
      status: cohort.lane,
      outcome: cohort.outcome,
      migrated: cohort.migrated,
      onToggleStatus: (lane: HistoryLane) =>
        cohort.set({ lane: cohort.lane === lane ? null : lane }),
      onToggleOutcome: (outcome: HistoryOutcome) =>
        cohort.set({ outcome: cohort.outcome === outcome ? null : outcome }),
      onToggleMigrated: (m: boolean) =>
        cohort.set({ migrated: cohort.migrated === m ? null : m }),
    }),
    [cohort],
  );

  const { hero, sections } = useMemo(() => {
    if (!run) return { hero: [] as SummaryStat[], sections: [] };
    return runSummarySections(run, { interaction, migrated });
  }, [run, interaction, migrated]);

  // Exits live in their own panel below (`HistoryExitSummary`) with the detailed
  // per-metric labels the wire counters can't carry — showing the collapsed
  // version here too would be two different answers to the same question.
  const bands = useMemo(() => sections.filter((s) => s.title !== 'Exits'), [sections]);

  if (!run) {
    return (
      <div className="rounded-lg border border-white/6 bg-bg-panel px-3 py-2.5 text-xs text-text-dim">
        {loading ? 'Loading summary…' : 'No summary for this cohort.'}
      </div>
    );
  }

  return (
    <div
      className={cn(
        'rounded-lg border border-white/6 bg-bg-panel px-3 pt-3',
        loading && 'opacity-60',
      )}
    >
      <SummaryStatsPanel
        title="Positions"
        heroStats={hero}
        sections={bands}
        className="mb-3"
      />
      {clientRun && (
        <p className="pb-2 text-[10px] leading-snug text-text-dim">
          Closed positions only, folded from the loaded rows — this focus can&apos;t be
          expressed as a server filter
          {scanCapped ? ', and the scan hit its row cap, so these are a lower bound' : ''}.
        </p>
      )}
    </div>
  );
});
