/**
 * The Console's **History** section — the new center of gravity for a
 * review-first workflow: one cohort filter bar driving a charts deck and a
 * server-paged table of every position across every rule and run.
 *
 * It replaces the old "Recent closed" lane, which could only ever show the last
 * 50 rows of the session's SSE buffer — so "what happened on Tuesday, and is any
 * rule decaying" needed a different page (or wasn't answerable at all).
 */

import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useSearchParams } from 'react-router-dom';
import { useTimezone } from 'context/TimezoneContext';
import { connectStrategyPositionUpdate } from 'services/sse';
import { useGetStrategyRulesQuery } from 'store/sharedEndpoints';
import { useGetPortfolioClosesSeriesQuery } from '@live/store/liveEndpoints';
import { useHistoryCohort } from '@live/pages/console/historyCohort';
import { filterClosesForCohort } from '@live/pages/console/historyExitFilter';
import { HistoryChartsDeck } from './HistoryChartsDeck';
import { HistoryExitSummary } from './HistoryExitSummary';
import { HistoryFilterBar } from './HistoryFilterBar';
import { HistoryTable } from './HistoryTable';

/** A terminal frame means a row entered (or left) the History population. */
const TERMINAL_STATUSES = new Set(['End', 'EntryFailed']);
/** Coalesce a burst of closes into one refetch (mirrors `usePortfolioRealtime`). */
const COALESCE_MS = 500;

export const ConsoleHistorySection = memo(function ConsoleHistorySection({
  selectedKey,
  onSelect,
}: {
  selectedKey: string | null;
  onSelect: (positionId: string | null, mint?: string) => void;
}) {
  const { timezone } = useTimezone();
  // Frozen per mount: a preset window's `from` bound must not slide on every
  // render, or the table refetches continuously.
  const [nowMs] = useState(() => Date.now());
  const cohort = useHistoryCohort(nowMs);
  const [reloadNonce, setReloadNonce] = useState(0);

  const { data: rules = [] } = useGetStrategyRulesQuery();
  const ruleNameOf = useCallback(
    (id: string | null) => (id ? (rules.find((r) => r.id === id)?.rule_name ?? null) : null),
    [rules],
  );

  // The series drives every chart AND the filter bar's counts. `mode: 'all'`
  // has no server-side equivalent (the series endpoint takes one mode), so it
  // charts the real book — the table still pages both.
  const {
    data: series,
    isFetching,
    refetch,
  } = useGetPortfolioClosesSeriesQuery({
    range: cohort.seriesRange,
    mode: cohort.mode === 'paper' ? 'paper' : 'real',
    ruleId: cohort.ruleId,
  });

  // Client-side trim: custom window (endpoint serves presets only) + status +
  // exit-reason contains — same cohort the table pages. Series points carry
  // `exit_reason` so metric needles (`stall`) match `stall >= 300`.
  const closes = useMemo(
    () =>
      filterClosesForCohort(series?.closes ?? [], {
        fromIso: cohort.fromIso,
        toIso: cohort.toIso,
        status: cohort.status,
        exitReason: cohort.exitReason,
      }),
    [series, cohort.fromIso, cohort.toIso, cohort.status, cohort.exitReason],
  );
  // Entry-failed is only part of the cohort when status is unrestricted or
  // explicitly EntryFailed (series closes are always `End`).
  const entryFailedForCohort =
    !cohort.status || cohort.status === 'EntryFailed'
      ? (series?.entry_failed ?? null)
      : null;

  // Live terminal frames invalidate the cohort — coalesced, one refetch of both
  // the series and the current table page.
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => {
    const bump = () => {
      if (timer.current) return;
      timer.current = setTimeout(() => {
        timer.current = null;
        setReloadNonce((n) => n + 1);
        void refetch();
      }, COALESCE_MS);
    };
    const h = connectStrategyPositionUpdate((d) => {
      if (TERMINAL_STATUSES.has(d.status)) bump();
    });
    return () => {
      h.close();
      if (timer.current) {
        clearTimeout(timer.current);
        timer.current = null;
      }
    };
  }, [refetch]);

  // A Portfolio "History" deep-link lands mid-page: bring the section into view
  // once, then drop the marker so a later param edit doesn't re-scroll.
  const [params, setParams] = useSearchParams();
  const wantsScroll = params.get('scroll') === 'history';
  useEffect(() => {
    if (!wantsScroll) return;
    document.getElementById('console-history')?.scrollIntoView({ block: 'start' });
    const next = new URLSearchParams(params);
    next.delete('scroll');
    setParams(next, { replace: true });
    // Runs once per arrival — `params`/`setParams` change identity on every URL
    // write, and depending on them would re-scroll on each filter change.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [wantsScroll]);

  return (
    <section id="console-history" className="flex flex-col gap-2">
      <h2 className="text-xs font-bold uppercase tracking-wider text-text-dim">History</h2>

      <HistoryFilterBar
        cohort={cohort}
        closedCount={series ? closes.length : null}
        entryFailed={entryFailedForCohort}
        ruleNameOf={(id) => ruleNameOf(id)}
      />

      {series && (
        <HistoryExitSummary
          closes={closes}
          timezone={timezone}
          exitReason={cohort.exitReason}
          focus={cohort.focus}
          onCohortPatch={(patch) => cohort.set(patch)}
        />
      )}

      <HistoryChartsDeck
        closes={closes}
        timezone={timezone}
        ruleNameOf={ruleNameOf}
        loading={isFetching}
        focus={cohort.focus}
        onFocusChange={(focus) => cohort.set({ focus })}
      />

      <HistoryTable
        cohort={cohort}
        timezone={timezone}
        ruleNameOf={ruleNameOf}
        selectedKey={selectedKey}
        onSelect={onSelect}
        reloadNonce={reloadNonce}
      />
    </section>
  );
});
