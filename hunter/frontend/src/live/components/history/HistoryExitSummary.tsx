/**
 * Exit mix + per-reason counts for Console History — the research detail the
 * charts deck never had. Folds the same B2 closes-series the deck uses, so the
 * counts cannot disagree with the cohort strip above the charts.
 *
 * Chart `hfocus` (day / heat / pct / …) narrows the strip the same way it
 * narrows equity / distribution — one cohort. Exit-tile clicks write `hexit`
 * and leave `hfocus` alone so the two compose.
 */

import { memo, useCallback, useMemo } from 'react';
import { cn } from 'lib/cn';
import {
  runSummarySections,
  type RunSummaryInteraction,
} from 'lib/strategy/runSummary';
import type { SummaryStat } from 'components/strategy/SummaryStatsPanel';
import type { ClosedTradePoint } from '@live/store/liveEndpoints';
import {
  filterClosesForFocus,
  type HistoryFocus,
} from '@live/pages/console/historyFocus';
import {
  activeExitTileLabel,
  exitTileToHistoryNeedle,
  historyRunSummaryFromCloses,
} from '@live/pages/console/historyExitSummary';

export const HistoryExitSummary = memo(function HistoryExitSummary({
  closes,
  timezone,
  exitReason,
  focus,
  onCohortPatch,
}: {
  closes: readonly ClosedTradePoint[];
  timezone: string;
  exitReason: string | null;
  focus: HistoryFocus | null;
  onCohortPatch: (patch: {
    exitReason?: string | null;
    focus?: HistoryFocus | null;
  }) => void;
}) {
  // Legacy exit focus stays on the parent mix (tile highlight only). Every other
  // chart lens refolds the strip onto the same slice equity/table use.
  const foldCloses = useMemo(() => {
    if (!focus || focus.kind === 'exit') return closes;
    return filterClosesForFocus(closes, focus, timezone);
  }, [closes, focus, timezone]);

  const summary = useMemo(
    () => historyRunSummaryFromCloses(foldCloses),
    [foldCloses],
  );
  const activeLabel = activeExitTileLabel(exitReason, focus);

  const onToggleExit = useCallback(
    (tileLabel: string) => {
      const needle = exitTileToHistoryNeedle(tileLabel);
      if (!needle) return;
      const next = exitReason === needle ? null : needle;
      // Keep chart focus — exit filter composes with day/heat/pct/….
      // Drop a legacy `hfocus=exit:…` so the needle owns the channel.
      if (focus?.kind === 'exit') {
        onCohortPatch({ exitReason: next, focus: null });
        return;
      }
      onCohortPatch({ exitReason: next });
    },
    [exitReason, focus, onCohortPatch],
  );

  const interaction: RunSummaryInteraction = useMemo(
    () => ({
      exitLabel: activeLabel,
      onToggleExit,
    }),
    [activeLabel, onToggleExit],
  );

  const { exitMix, sections } = useMemo(
    () => runSummarySections(summary, { interaction }),
    [summary, interaction],
  );

  const exits = sections.find((s) => s.title === 'Exits');
  if (!exits) return null;

  return (
    <div className="rounded-lg border border-white/6 bg-bg-panel px-3 py-2.5">
      {exitMix && <div className="mb-2 max-w-xl">{exitMix}</div>}
      <div className="flex flex-col gap-2 sm:flex-row sm:items-start sm:gap-6">
        <div className="shrink-0 sm:w-44">
          <div className="text-[10px] font-bold uppercase tracking-wider text-text-mid">
            {exits.title}
          </div>
          {exits.hint && (
            <div className="mt-0.5 text-[10px] leading-snug text-text-dim">{exits.hint}</div>
          )}
        </div>
        <div className="flex flex-1 flex-wrap gap-x-8 gap-y-3">
          {exits.stats.map((s) => (
            <ExitTile key={s.label} stat={s} />
          ))}
        </div>
      </div>
    </div>
  );
});

function ExitTile({ stat }: { stat: SummaryStat }) {
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
