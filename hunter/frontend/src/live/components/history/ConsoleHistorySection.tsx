/**
 * The Console's **History** section — the center of gravity for a review-first
 * workflow: one cohort driving a summary strip, an exit breakdown, a charts
 * deck, and a server-paged table of every position across every rule and run.
 *
 * It replaces the old "Recent closed" lane, which could only ever show the last
 * 50 rows of the session's SSE buffer — so "what happened on Tuesday, and is any
 * rule decaying" needed a different page (or wasn't answerable at all).
 *
 * **One population, three readings.** The cohort bar, the chart focus, and the
 * table's own search + per-column filters compose into a single request body
 * (`historyRequest`), which is then read three ways: paged for the table,
 * aggregated server-side for the strip, and walked in full for the charts. That
 * is what makes filtering the table narrow everything above it — previously the
 * deck folded a separate closes-series endpoint that was closed-only,
 * single-mode, and blind to the table's filters, so the numbers on screen could
 * describe two different cohorts at once.
 */

import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useSearchParams } from 'react-router-dom';
import { useTimezone } from 'context/TimezoneContext';
import { connectStrategyPositionUpdate } from 'services/sse';
import { useGetStrategyRulesQuery } from 'store/sharedEndpoints';
import {
  fetchPortfolioPositionsPage,
  fetchPortfolioPositionsSummary,
} from 'services/api';
import { numericColKeys } from 'services/tableRequest';
import { fetchAllTablePages } from 'lib/strategy/fetchPositionChartSeries';
import { DEFAULT_POSITIONS_QUERY } from 'hooks/useServerTable';
import type { TableQuery } from 'components/table/types';
import type { PositionsSummary, RulePositionRecord } from 'types';
import { useHistoryCohort } from '@live/pages/console/historyCohort';
import {
  historyNeedsClientScan,
  historyPopulationKey,
  historySummaryBody,
} from '@live/pages/console/historyRequest';
import {
  applyHistoryCohortNeedle,
  countEntryFailed,
  positionsToClosePoints,
} from '@live/pages/console/historyPositions';
import { filterClosesForFocus } from '@live/pages/console/historyFocus';
import { historyRunSummaryFromCloses } from '@live/pages/console/historyExitSummary';
import { HistoryChartsDeck } from './HistoryChartsDeck';
import { HistoryExitSummary } from './HistoryExitSummary';
import { HistoryFilterBar } from './HistoryFilterBar';
import { HistorySummaryStrip } from './HistorySummaryStrip';
import { HistoryTable, historyColumns } from './HistoryTable';

/** A terminal frame means a row entered (or left) the History population. */
const TERMINAL_STATUSES = new Set(['End', 'EntryFailed']);
/** Coalesce a burst of closes into one refetch (mirrors `usePortfolioRealtime`). */
const COALESCE_MS = 500;
/**
 * Row ceiling for the charts walk. The strip is unaffected (it aggregates in
 * Postgres), so a cohort past this cap gets exact totals over partial-but-honest
 * charts — and says so, rather than drawing a curve that silently stops.
 */
const CHART_SCAN_MAX = 20_000;

const NO_ROWS: RulePositionRecord[] = [];

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

  // The table's own view state, owned here — this is what lets the strip and the
  // deck narrow with it instead of describing the unfiltered book.
  const [query, setQuery] = useState<TableQuery>(DEFAULT_POSITIONS_QUERY);
  const columns = useMemo(() => historyColumns(ruleNameOf), [ruleNameOf]);
  const numericCols = useMemo(() => numericColKeys(columns), [columns]);

  const input = useMemo(
    () => ({ cohort, query, numericCols, timezone }),
    [cohort, query, numericCols, timezone],
  );
  const clientScan = historyNeedsClientScan(cohort);
  // Page/sort deliberately excluded — paging the table must not re-run the
  // aggregate or re-walk the cohort. The aggregate follows the focus (it
  // describes the rows the table is showing); the walk does not (the deck owns
  // the lens), so clicking through chart cells re-fetches one aggregate row
  // instead of the whole cohort.
  const summaryKey = useMemo(
    () => `${historyPopulationKey({ cohort, query, timezone })}|${reloadNonce}`,
    [cohort, query, timezone, reloadNonce],
  );
  const parentKey = useMemo(
    () =>
      `${historyPopulationKey({ cohort, query, timezone }, { includeFocus: false })}|${reloadNonce}`,
    [cohort, query, timezone, reloadNonce],
  );

  const [summary, setSummary] = useState<PositionsSummary | null>(null);
  const [summaryLoading, setSummaryLoading] = useState(true);
  const [cohortRows, setCohortRows] = useState<RulePositionRecord[]>(NO_ROWS);
  const [chartsLoading, setChartsLoading] = useState(true);
  const [scanCapped, setScanCapped] = useState(false);

  // The aggregate — exact over the whole filtered population, no rows shipped.
  useEffect(() => {
    const ctrl = new AbortController();
    setSummaryLoading(true);
    fetchPortfolioPositionsSummary(historySummaryBody(input), ctrl.signal)
      .then((s) => {
        if (ctrl.signal.aborted) return;
        setSummary(s);
        setSummaryLoading(false);
      })
      .catch((e) => {
        if (ctrl.signal.aborted || (e instanceof DOMException && e.name === 'AbortError')) return;
        setSummary(null);
        setSummaryLoading(false);
      });
    return () => ctrl.abort();
    // `input` changes identity per render; the population key is the real dep.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [summaryKey]);

  // The **parent** cohort (no chart focus) for the deck — it lenses itself.
  useEffect(() => {
    const ctrl = new AbortController();
    setChartsLoading(true);
    fetchAllTablePages(
      (b, signal) => fetchPortfolioPositionsPage(b, signal),
      historySummaryBody(input, { includeFocus: false }),
      ctrl.signal,
      CHART_SCAN_MAX,
    )
      .then((rows) => {
        if (ctrl.signal.aborted) return;
        setCohortRows(rows);
        setScanCapped(rows.length >= CHART_SCAN_MAX);
        setChartsLoading(false);
      })
      .catch((e) => {
        if (ctrl.signal.aborted || (e instanceof DOMException && e.name === 'AbortError')) return;
        setCohortRows(NO_ROWS);
        setScanCapped(false);
        setChartsLoading(false);
      });
    return () => ctrl.abort();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [parentKey]);

  // The parent population, with only the cohort-level client needle applied —
  // the deck and the exit strip take it from here and lens it themselves.
  const parentRows = useMemo(
    () => applyHistoryCohortNeedle(cohortRows, cohort),
    [cohortRows, cohort],
  );
  const parentCloses = useMemo(() => positionsToClosePoints(parentRows), [parentRows]);
  const entryFailed = useMemo(() => countEntryFailed(parentRows), [parentRows]);

  // The strip's client-fold source: the focused slice, folded from closes only.
  // Closes-only is right rather than a shortcut — every lens that forces this
  // path (heat, hold band, exit focus, Metric±) keys on an exit, so an open
  // position matches none of them and belongs in no such slice.
  const focusedCloses = useMemo(() => {
    if (!clientScan) return parentCloses;
    if (!cohort.focus) return parentCloses;
    return filterClosesForFocus(parentCloses, cohort.focus, timezone);
  }, [clientScan, parentCloses, cohort.focus, timezone]);
  const clientRun = useMemo(
    () => (clientScan ? historyRunSummaryFromCloses(focusedCloses) : null),
    [clientScan, focusedCloses],
  );

  // Live terminal frames invalidate the cohort — coalesced, one refetch of the
  // aggregate, the chart walk, and the table's current page.
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => {
    const bump = () => {
      if (timer.current) return;
      timer.current = setTimeout(() => {
        timer.current = null;
        setReloadNonce((n) => n + 1);
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
  }, []);

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
        closedCount={chartsLoading ? null : parentCloses.length}
        entryFailed={chartsLoading ? null : entryFailed}
        ruleNameOf={(id) => ruleNameOf(id)}
      />

      <HistorySummaryStrip
        cohort={cohort}
        summary={summary}
        clientRun={clientRun}
        loading={summaryLoading || (clientScan && chartsLoading)}
        scanCapped={scanCapped}
      />

      <HistoryExitSummary
        closes={parentCloses}
        timezone={timezone}
        exitReason={cohort.exitReason}
        focus={cohort.focus}
        onCohortPatch={(patch) => cohort.set(patch)}
      />

      <HistoryChartsDeck
        closes={parentCloses}
        timezone={timezone}
        ruleNameOf={ruleNameOf}
        loading={chartsLoading}
        focus={cohort.focus}
        onFocusChange={(focus) => cohort.set({ focus })}
      />

      <HistoryTable
        cohort={cohort}
        columns={columns}
        numericCols={numericCols}
        timezone={timezone}
        query={query}
        onQueryChange={setQuery}
        ruleNameOf={ruleNameOf}
        selectedKey={selectedKey}
        onSelect={onSelect}
        reloadNonce={reloadNonce}
      />
    </section>
  );
});
