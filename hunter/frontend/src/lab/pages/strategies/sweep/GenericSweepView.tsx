import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { useSearchParams } from 'react-router-dom';
import { useLocalStorage } from 'hooks/useLocalStorage';
import { STORAGE_KEYS } from 'lib/storage';
import { DataTable } from 'components/table/DataTable';
import { TokenTable } from 'components/tokens/TokenTable';
import type { ColumnDef, SortEntry, TableQuery } from 'components/table/types';
import { InlineAlert } from 'components/ui/Modal';
import { SectionDivider } from 'components/ui/SectionDivider';
import { Badge } from 'components/ui/Badge';
import { Button } from 'components/ui/Button';
import { Accordion } from 'components/ui/Accordion';
import { VisibilityToggleButton } from 'components/ui/VisibilityToggleButton';
import { markerRowOverlay, type InspectTarget } from 'components/strategy/inspectTarget';
import {
  SummaryStatsPanel,
  type SummaryStat,
  type SummarySection,
} from 'components/strategy/SummaryStatsPanel';
import { runSummaryFromRows, runSummarySections } from 'lib/strategy/runSummary';
import { useBackgroundJobActions, useBackgroundJobsState } from '@lab/context/BackgroundJobsContext';
import { apiErrorMessage } from 'store/apiSlice';
import {
  useGetGroupedSweepRunsQuery,
  useGetGroupedSweepGroupsQuery,
  useGetComboTokenResultsQuery,
  useStartGroupedSweepMutation,
  useDeleteGroupedSweepRunMutation,
  usePruneGroupedSweepsMutation,
  usePromoteSweepGroupMutation,
} from '@lab/store/labEndpoints';
import { useStreamedSweepResults, COMBO_PAGE_SIZE } from '@lab/hooks/useStreamedSweepResults';
import { SelectedSweepHistory } from '@lab/components/sweep/SelectedSweepHistory';
import { GenericSweepConfigForm, GENERIC_STRATEGY_ID } from '@lab/components/sweep/GenericSweepConfigForm';
import { PromoteRuleModal } from '@lab/components/sweep/PromoteRuleModal';
import { LabTokenInspectModal } from '@lab/components/strategy/LabTokenInspectModal';
import {
  buildGenericComboColumns,
  buildGenericGroupColumns,
} from '@lab/components/sweep/genericSweepColumns';
import type {
  GroupedSweepGroupRecord,
  GroupedSweepRunRecord,
  GroupedSweepResultRecord,
  ComboTokenResult,
} from '@lab/components/sweep/groupedTypes';
import type { GroupedSweepStartArgs } from '@lab/components/sweep/groupedTypes';
import type { PromotedRuleDraft } from 'lib/strategy/types';

/** Compact run-picker line (date · method · tokens · groups · combos). */
function runPickerLine(r: GroupedSweepRunRecord): string {
  const d = new Date(r.created_at);
  const p = (n: number) => String(n).padStart(2, '0');
  const date = `${p(d.getMonth() + 1)}/${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
  const groups =
    r.status === 'completed'
      ? `${r.group_count} grp`
      : `${r.status === 'running' ? 'run' : 'part'} ${r.groups_done}/${r.group_count} grp`;
  const suffix = r.label ? ` · ${r.label}` : '';
  return `${date} · ${r.method} · ${r.token_count.toLocaleString()} tok · ${groups} × ${r.combo_count.toLocaleString()} combos${suffix}`;
}

/** ComboTokenResult → inspect target (entry/exit markers). */
function comboTarget(r: ComboTokenResult): InspectTarget {
  return {
    mint_address: r.mint_address,
    symbol: r.symbol,
    entryTime: r.entry_time ?? null,
    entryPrice: r.entry_price ?? null,
    entryTx: r.entry_tx ?? null,
    exitTime: r.exit_time ?? null,
    exitPrice: r.exit_price ?? null,
    exitTx: r.exit_tx ?? null,
    exitLabel: r.fired ? r.exit : null,
  };
}

/** Per-row entry/exit markers for the token-results charts grid. */
const comboRowOverlay = markerRowOverlay(comboTarget);

/**
 * Aggregate the combo's per-token rows into the summary tiles, via the **shared**
 * run-summary renderer — the same builder the Simulate page and the live/paper
 * positions card use, so all three show the same metrics under the same labels in
 * the same format (parity plan F1-F8). It runs over whatever cohort it's handed
 * (the FULL results, or the table's current filtered subset), so the summary
 * tracks the table's search/column filters.
 *
 * The realized-vs-MTM two-band split and the metric arithmetic live in
 * `lib/strategy/runSummary`; this is now only the row adapter.
 */
function buildComboSummary(rows: ComboTokenResult[]): {
  hero: SummaryStat[];
  sections: SummarySection[];
} {
  const migrated = rows.filter((r) => r.fired && r.is_migrated).length;
  return runSummarySections(runSummaryFromRows(rows), { migrated });
}

/**
 * Generic-engine grouped sweep view (redesign FE5.2 + 5.3). One page replaces the
 * three per-strategy sweep pages: configure → run → pick a run → group-summary
 * table → drill into ranked combos → per-token results, with a one-click
 * **Promote** on any group/combo that opens the rule editor pre-filled. Reuses the
 * kept streaming/persistence infrastructure (`useStreamedSweepResults`, the sweep
 * RTK endpoints) with `strategy_id = "generic"`.
 */
export function GenericSweepView() {
  const strategyId = GENERIC_STRATEGY_ID;
  const [searchParams, setSearchParams] = useSearchParams();
  const runFromUrl = searchParams.get('run');
  const runsQuery = useGetGroupedSweepRunsQuery({ strategyId });
  const runs = useMemo(() => runsQuery.data ?? [], [runsQuery.data]);

  const [selectedRunId, setSelectedRunId] = useLocalStorage<string | null>(
    `${STORAGE_KEYS.sweepSel}.generic`,
    null,
  );

  // Home / deep-link `?run=` wins over localStorage — but ONLY on the first load
  // of the runs list. Without the one-shot guard this effect fights the URL-sync
  // effect below: picking a run in the dropdown sets `selectedRunId` while
  // `?run=` still names the *previous* run, so this would immediately write the
  // old id straight back and the page would snap to the old run's results.
  const urlHydratedRef = useRef(false);
  useEffect(() => {
    if (urlHydratedRef.current || runs.length === 0) return;
    urlHydratedRef.current = true;
    if (!runFromUrl) return;
    if (!runs.some((r) => r.id === runFromUrl)) return;
    if (selectedRunId === runFromUrl) return;
    setSelectedRunId(runFromUrl);
  }, [runFromUrl, runs, selectedRunId, setSelectedRunId]);

  const activeRunId = selectedRunId && runs.some((r) => r.id === selectedRunId)
    ? selectedRunId
    : (runs[0]?.id ?? null);
  const activeRun = runs.find((r) => r.id === activeRunId) ?? null;

  // Keep URL in sync so resume links and refresh stay on the same run.
  useEffect(() => {
    if (!activeRunId) return;
    setSearchParams(
      (prev) => {
        if (prev.get('run') === activeRunId) return prev;
        const next = new URLSearchParams(prev);
        next.set('run', activeRunId);
        return next;
      },
      { replace: true },
    );
  }, [activeRunId, setSearchParams]);

  const [activeGroupId, setActiveGroupId] = useState<string | null>(null);
  const [activeComboId, setActiveComboId] = useState<number | null>(null);
  useEffect(() => setActiveComboId(null), [activeGroupId]);

  const selectRun = useCallback(
    (id: string) => {
      setSelectedRunId(id);
      setActiveGroupId(null);
      setActiveComboId(null);
    },
    [setSelectedRunId],
  );

  // --- run lifecycle (start / delete / prune) ---
  const [startSweep, startState] = useStartGroupedSweepMutation();
  const startErr = apiErrorMessage(startState.error, 'Failed to start sweep');
  const { markStarting } = useBackgroundJobActions();
  const { isRunning, jobs } = useBackgroundJobsState();
  const sweepRunning = isRunning('sweep', 'sweep') || startState.isLoading;
  // Live persisted-group counters from the `sweep_group_done` SSE stream — the
  // run row's `groups_done` only refreshes on the terminal frame, so the
  // in-progress banner prefers these when the running job matches this run.
  const sweepJob = jobs.find((j) => j.kind === 'sweep') ?? null;

  const [deleteRun, deleteState] = useDeleteGroupedSweepRunMutation();
  const [pruneRuns, pruneState] = usePruneGroupedSweepsMutation();
  const deleteErr = apiErrorMessage(deleteState.error ?? pruneState.error, 'Failed to delete sweep history');
  const [pruneBefore, setPruneBefore] = useState('');

  const [reuseNonce, setReuseNonce] = useState(0);
  const formRef = useRef<HTMLDivElement>(null);

  async function run(args: GroupedSweepStartArgs) {
    markStarting('sweep', 'sweep', 'Grouped sweep');
    try {
      const { run_id } = await startSweep(args).unwrap();
      setSelectedRunId(run_id);
    } catch {
      // Surfaced via startState.error (e.g. 409 = one already running).
    }
  }

  async function onDeleteRun() {
    if (!activeRunId) return;
    if (!window.confirm('Delete this sweep run and all its groups/results?')) return;
    try {
      await deleteRun({ strategyId, runId: activeRunId }).unwrap();
      setSelectedRunId(null);
    } catch { /* surfaced via deleteErr */ }
  }
  async function onPrune() {
    if (!pruneBefore) return;
    if (!window.confirm(`Delete all sweep runs created before ${pruneBefore}?`)) return;
    try {
      await pruneRuns({ strategyId, before: new Date(pruneBefore).toISOString() }).unwrap();
      setSelectedRunId(null);
    } catch { /* surfaced via deleteErr */ }
  }

  // --- groups ---
  const groupsQuery = useGetGroupedSweepGroupsQuery(
    { strategyId, runId: activeRunId ?? '' },
    { skip: !activeRunId },
  );
  const groups = groupsQuery.data ?? [];
  const activeGroup = groups.find((g) => g.id === activeGroupId) ?? null;
  const tokensDone = useMemo(
    () => (groups.length ? groups.reduce((s, g) => s + g.token_count, 0) : null),
    [groups],
  );

  // --- combos (streamed) ---
  const [comboPage, setComboPage] = useState(0);
  const [comboPageSize, setComboPageSize] = useState(COMBO_PAGE_SIZE);
  const [comboSortKeys, setComboSortKeys] = useState<SortEntry[]>([]);
  const onComboQueryChange = useCallback((q: TableQuery) => {
    setComboPage(q.page - 1);
    setComboPageSize(q.pageSize);
    setComboSortKeys(q.sortKeys);
  }, []);
  useEffect(() => setComboPage(0), [activeGroupId]);

  // Drop the previous run's drill-down DURING render, not in an effect. An
  // effect-based reset lands one commit too late: the results stream would
  // already have fired once for (new run, *old* group) → 404 + error flash.
  const [runScope, setRunScope] = useState(activeRunId);
  if (runScope !== activeRunId) {
    setRunScope(activeRunId);
    setActiveGroupId(null);
    setActiveComboId(null);
    setComboPage(0);
  }

  const { rows: results, total: resultsTotal, loading: resultsLoading, error: resultsErr } =
    useStreamedSweepResults(strategyId, activeRunId, activeGroupId, comboPage, comboPageSize, comboSortKeys);

  // --- promote ---
  const [promote, promoteState] = usePromoteSweepGroupMutation();
  const [promoteDraft, setPromoteDraft] = useState<PromotedRuleDraft | null>(null);
  const promoteErr = apiErrorMessage(promoteState.error, 'Promote failed');
  const doPromote = useCallback(
    async (groupId: string, comboId?: number) => {
      if (!activeRunId) return;
      try {
        setPromoteDraft(await promote({ runId: activeRunId, groupId, comboId }).unwrap());
      } catch { /* surfaced via promoteErr */ }
    },
    [activeRunId, promote],
  );

  const groupColumns = useMemo(() => buildGenericGroupColumns(), []);
  const comboColumns = useMemo(() => buildGenericComboColumns(), []);

  const groupRowActions = useCallback(
    (g: GroupedSweepGroupRecord): ReactNode => (
      <Button
        variant="ghost"
        size="xs"
        disabled={promoteState.isLoading}
        onClick={(e) => {
          e.stopPropagation();
          void doPromote(g.id);
        }}
        title="Promote this group's best combo → new rule"
      >
        Promote
      </Button>
    ),
    [doPromote, promoteState.isLoading],
  );
  const comboRowActions = useCallback(
    (r: GroupedSweepResultRecord): ReactNode =>
      activeGroupId ? (
        <Button
          variant="ghost"
          size="xs"
          disabled={promoteState.isLoading}
          onClick={(e) => {
            e.stopPropagation();
            void doPromote(activeGroupId, r.combo_id);
          }}
          title="Promote this combo → new rule"
        >
          Promote
        </Button>
      ) : null,
    [activeGroupId, doPromote, promoteState.isLoading],
  );

  const runsErr = apiErrorMessage(runsQuery.error, 'Failed to load sweep runs');
  const groupsErr = apiErrorMessage(groupsQuery.error, 'Failed to load groups');
  return (
    <div>
      <div className="mb-3.5 flex flex-wrap items-center gap-2.5">
        <h1 className="text-lg font-extrabold text-text">Grouped Sweep</h1>
        <span className="text-sm text-text-mid">
          Configure → run → promote · drill into combos/tokens
        </span>
        <Badge variant="primary" className="font-mono">
          {runs.length} runs · {groups.length} groups
        </Badge>
      </div>

      {activeRunId && (
        <nav
          aria-label="Sweep drill path"
          className="sticky top-14 z-40 mb-3 flex flex-wrap items-center gap-1.5 rounded-md border border-white/8 bg-bg/90 px-3 py-2 text-[12px] backdrop-blur-md"
        >
          <button
            type="button"
            className="font-semibold text-primary hover:underline"
            onClick={() => {
              setActiveGroupId(null);
              setActiveComboId(null);
            }}
          >
            Run
          </button>
          <span className="text-text-dim">›</span>
          {activeGroup ? (
            <button
              type="button"
              className="max-w-[28rem] truncate font-medium text-text hover:underline"
              title={groupBreadcrumb(activeGroup)}
              onClick={() => setActiveComboId(null)}
            >
              {groupBreadcrumb(activeGroup)}
            </button>
          ) : (
            <span className="text-text-dim">pick a group</span>
          )}
          {activeComboId !== null && (
            <>
              <span className="text-text-dim">›</span>
              <span className="font-mono text-secondary">combo #{activeComboId}</span>
            </>
          )}
        </nav>
      )}

      <Accordion title="Configure sweep" defaultOpen={runs.length === 0}>
        <div ref={formRef}>
          <GenericSweepConfigForm
            storageKey={`${STORAGE_KEYS.sweepConfig}.generic`}
            running={sweepRunning}
            onRun={run}
            reuseNonce={reuseNonce}
            reuseRun={activeRun}
          />
        </div>
      </Accordion>

      {startErr && <InlineAlert variant="error">{startErr}</InlineAlert>}
      {promoteErr && <InlineAlert variant="error">{promoteErr}</InlineAlert>}
      {runsQuery.isLoading && <p className="text-text-dim">Loading sweep runs…</p>}
      {runsErr && <InlineAlert variant="error">{runsErr}</InlineAlert>}

      {!runsQuery.isLoading && !runsErr && runs.length === 0 && (
        <div className="rounded-md border border-white/10 bg-surface p-3 text-sm text-text-dim">
          No grouped sweeps yet. Set a date range + grouping + axes above and click{' '}
          <span className="text-primary">Run grouped sweep</span>.
        </div>
      )}

      {runs.length > 0 && (
        <>
          <SectionDivider />
          {deleteErr && <InlineAlert variant="error">{deleteErr}</InlineAlert>}
          {groupsErr && <InlineAlert variant="error">{groupsErr}</InlineAlert>}

          {activeRun && (
            <Accordion
              header={
                <div className="flex flex-1 flex-wrap items-center gap-2.5">
                  <label className="text-sm text-text-dim" htmlFor="generic-sweep-run">Run</label>
                  <select
                    id="generic-sweep-run"
                    className="rounded-md border border-white/10 bg-surface px-2.5 py-1.5 text-sm text-primary font-mono tabular-nums"
                    value={activeRunId ?? ''}
                    onChange={(e) => selectRun(e.target.value)}
                  >
                    {runs.map((r) => (
                      <option key={r.id} value={r.id}>{runPickerLine(r)}</option>
                    ))}
                  </select>
                  <Button variant="danger" size="sm" disabled={!activeRunId || deleteState.isLoading} onClick={onDeleteRun}>
                    {deleteState.isLoading ? 'Deleting…' : 'Delete Run'}
                  </Button>
                  <span className="ml-auto flex items-center gap-2">
                    <label className="text-sm text-text-dim" htmlFor="generic-sweep-prune">Clear runs before</label>
                    <input
                      id="generic-sweep-prune"
                      type="date"
                      className="rounded-md border border-white/10 bg-surface px-2.5 py-1.5 text-sm text-primary"
                      value={pruneBefore}
                      onChange={(e) => setPruneBefore(e.target.value)}
                    />
                    <Button variant="danger" size="sm" disabled={!pruneBefore || pruneState.isLoading} onClick={onPrune}>
                      {pruneState.isLoading ? 'Clearing…' : 'Clear All OLD'}
                    </Button>
                  </span>
                </div>
              }
              defaultOpen={false}
              className="mb-3"
            >
              <SelectedSweepHistory
                strategyId={strategyId}
                run={activeRun}
                tokensDone={tokensDone}
                onReuse={() => {
                  setReuseNonce((n) => n + 1);
                  formRef.current?.scrollIntoView({ behavior: 'smooth', block: 'start' });
                }}
              />
            </Accordion>
          )}

          {activeRun && activeRun.status !== 'completed' && (
            <InlineAlert variant="warning">
              {activeRun.status === 'running' ? 'In-progress' : 'Partial'} run —{' '}
              {(sweepJob && sweepJob.runId === activeRun.id ? sweepJob.groupsDone : null) ??
                activeRun.groups_done}{' '}
              of{' '}
              {(sweepJob && sweepJob.runId === activeRun.id ? sweepJob.groupCount : null) ??
                activeRun.group_count}{' '}
              groups
              {activeRun.status === 'running'
                ? ' persisted so far — each is browsable below as soon as it lands.'
                : activeRun.status === 'partial'
                  ? ' persisted before a write error ended the run — this is not a full sweep.'
                  : ' completed before the run was cancelled — this is not a full sweep.'}
            </InlineAlert>
          )}

          <DataTable
            columns={groupColumns}
            rows={groups}
            rowKey={(g) => g.id}
            rowActions={groupRowActions}
            groupLabels={{
              group: 'Group',
              counts: 'Counts',
              pnl: 'PnL (best combo)',
              holding: 'Holding',
              params: 'Best rule',
            }}
            defaultSort={{ col: 'best_total_pnl_sol', dir: 'desc' }}
            searchable
            colFilters
            colToggle
            selectable
            selectedKey={activeGroupId}
            onSelect={setActiveGroupId}
            sameValueTints
            tableId="generic_sweep_groups"
            resetKey={activeRunId ?? ''}
            loading={groupsQuery.isFetching}
            emptyMessage="No groups cleared the min-tokens threshold for this run."
          />

          {activeGroupId && (
            <div className="mt-4">
              <div className="mb-2 flex flex-wrap items-center gap-2">
                <h3 className="text-sm font-bold text-secondary">Combos for group</h3>
                {activeGroup && (
                  <span className="font-mono text-xs text-text-dim">
                    {Object.keys(activeGroup.group_key).length
                      ? Object.entries(activeGroup.group_key).map(([k, v]) => `${k}=${v}`).join(' · ')
                      : 'ALL tokens'}{' '}
                    · {activeGroup.token_count} tokens
                  </span>
                )}
              </div>

              {resultsErr && <InlineAlert variant="error">{resultsErr}</InlineAlert>}

              <DataTable
                columns={comboColumns}
                rows={results}
                rowKey={(r) => String(r.combo_id)}
                rowActions={comboRowActions}
                groupLabels={{ params: 'Rule', counts: 'Counts', pnl: 'PnL', holding: 'Holding', exits: 'Exit reasons' }}
                searchable={false}
                selectable
                selectedKey={activeComboId !== null ? String(activeComboId) : null}
                onSelect={(key) => setActiveComboId(key !== null ? Number(key) : null)}
                sameValueTints
                serverSide
                serverTotal={resultsTotal}
                onQueryChange={onComboQueryChange}
                tableId="generic_sweep_combos"
                resetKey={activeGroupId ?? ''}
                loading={resultsLoading}
                emptyMessage="No combo results for this group."
              />

              {activeComboId !== null && activeGroupId && (
                <ComboTokenResults
                  strategyId={strategyId}
                  runId={activeRunId ?? ''}
                  groupId={activeGroupId}
                  comboId={activeComboId}
                  comboParams={
                    results.find((r) => r.combo_id === activeComboId)?.params ?? null
                  }
                  onClose={() => setActiveComboId(null)}
                />
              )}
            </div>
          )}
        </>
      )}

      <PromoteRuleModal draft={promoteDraft} onClose={() => setPromoteDraft(null)} />
    </div>
  );
}

// --- per-token drill-in -----------------------------------------------------

/** The re-simulated per-token results for one combo (fired / PnL / exit) + a
 *  click-through to the token chart with entry/exit markers. */
function ComboTokenResults({
  strategyId,
  runId,
  groupId,
  comboId,
  comboParams,
  onClose,
}: {
  strategyId: string;
  runId: string;
  groupId: string;
  comboId: number;
  /** The combo's swept `RuleParams` blob — pins the inspect's metric panes to the
   *  exact params that produced these rows. Null when the row paged out of view. */
  comboParams: Record<string, unknown> | null;
  onClose: () => void;
}) {
  const query = useGetComboTokenResultsQuery({ strategyId, runId, groupId, comboId });
  // Stable identities: RTK keeps `query.data` referentially equal across renders, so
  // memoizing keeps `rows`/`visible` from churning — otherwise the filtered-rows
  // callback below (which sets state) would re-fire every render into a loop.
  const rows = useMemo(() => query.data?.rows ?? [], [query.data]);
  const err = apiErrorMessage(query.error, 'Failed to load token results');
  const [showNotFired, setShowNotFired] = useLocalStorage(
    STORAGE_KEYS.sweepShowNotFired,
    true,
  );
  const visible = useMemo(
    () => (showNotFired ? rows : rows.filter((r) => r.fired)),
    [showNotFired, rows],
  );
  const [selected, setSelected] = useState<string | null>(null);
  useEffect(() => setSelected(null), [comboId]);

  // Summary cohort = the table's current filtered rows (null until the table first
  // reports them → fall back to the full set so the card shows immediately). Reset
  // on combo change so a stale cohort can't linger while the new rows load.
  const [filteredRows, setFilteredRows] = useState<ComboTokenResult[] | null>(null);
  useEffect(() => setFilteredRows(null), [comboId]);
  const onFilteredRowsChange = useCallback((r: ComboTokenResult[]) => setFilteredRows(r), []);
  const summary = useMemo(
    () => buildComboSummary(filteredRows ?? visible),
    [filteredRows, visible],
  );
  const selectedRow = selected ? rows.find((r) => r.mint_address === selected) ?? null : null;

  const columns = useMemo<ColumnDef<ComboTokenResult>[]>(
    () => [
      {
        key: 'symbol',
        label: 'Symbol',
        render: (r) => <span className="font-mono text-xs">{r.symbol || '—'}</span>,
        searchValue: (r) => r.symbol,
        sortValue: (r) => r.symbol,
        sortable: true,
      },
      {
        key: 'mint_address',
        label: 'Mint',
        render: (r) => (
          <span className="font-mono text-xs text-text-dim" title={r.mint_address}>
            {r.mint_address.slice(0, 8)}…{r.mint_address.slice(-4)}
          </span>
        ),
        searchValue: (r) => r.mint_address,
      },
      {
        key: 'fired',
        label: 'Fired',
        render: (r) => <span className={r.fired ? 'text-success' : 'text-text-dim'}>{r.fired ? 'Yes' : 'No'}</span>,
        searchValue: (r) => (r.fired ? 'yes' : 'no'),
        sortValue: (r) => (r.fired ? 1 : 0),
        sortable: true,
      },
      {
        key: 'pnl_sol',
        label: 'PnL (SOL)',
        render: (r) => (
          <span className={r.pnl_sol > 0 ? 'text-success' : r.pnl_sol < 0 ? 'text-danger' : 'text-text-dim'}>
            {r.fired && r.pnl_sol != null ? r.pnl_sol.toFixed(4) : '—'}
          </span>
        ),
        searchValue: () => '',
        filterNumber: (r) => (r.fired ? r.pnl_sol : null),
        sortValue: (r) => r.pnl_sol,
        sortable: true,
      },
      {
        key: 'pnl_pct',
        label: 'PnL %',
        render: (r) => (
          <span className={r.pnl_pct > 0 ? 'text-success' : r.pnl_pct < 0 ? 'text-danger' : 'text-text-dim'}>
            {r.fired && r.pnl_pct != null ? `${r.pnl_pct.toFixed(1)}%` : '—'}
          </span>
        ),
        searchValue: () => '',
        sortValue: (r) => r.pnl_pct,
        sortable: true,
      },
      {
        key: 'holding_secs',
        label: 'Hold (s)',
        render: (r) => <span className="text-text-dim">{r.fired ? r.holding_secs : '—'}</span>,
        searchValue: () => '',
        sortValue: (r) => r.holding_secs,
        sortable: true,
      },
      {
        key: 'exit',
        label: 'Exit',
        render: (r) => <span className="font-mono text-xs">{r.exit}</span>,
        searchValue: (r) => r.exit,
        sortValue: (r) => r.exit,
        sortable: true,
      },
    ],
    [],
  );
  // The combo's own column keys — everything else in the shared token-info set is
  // appended by TokenTable so the drill-in carries the full enrichment columns.
  const existingKeys = useMemo(() => new Set(columns.map((c) => c.key)), [columns]);

  return (
    <div className="mt-4">
      <div className="mb-2 flex flex-wrap items-center gap-2">
        <h3 className="text-sm font-bold text-secondary">Tokens for combo #{comboId}</h3>
        <span className="text-xs text-text-dim">
          {query.isFetching ? 'Simulating…' : `${visible.length}${!showNotFired ? ` / ${rows.length}` : ''} tokens`}
        </span>
        <div className="grow" />
        <VisibilityToggleButton visible={showNotFired} onToggle={() => setShowNotFired((v) => !v)} label="not-fired tokens">
          {showNotFired ? 'Hide not fired' : 'Show not fired'}
        </VisibilityToggleButton>
        <button className="text-xs text-text-dim hover:text-primary" onClick={onClose}>✕ Close</button>
      </div>

      {err && <InlineAlert variant="error">{err}</InlineAlert>}

      {!err && rows.length > 0 && (
        <SummaryStatsPanel
          title="Combo results summary"
          subtitle="Tracks the table's filters"
          heroStats={summary.hero}
          sections={summary.sections}
          accentClass="bg-secondary"
        />
      )}

      <TokenTable
        columns={columns}
        existingKeys={existingKeys}
        charts
        useRowOverlay={comboRowOverlay}
        rows={visible}
        rowKey={(r) => r.mint_address}
        searchable
        colFilters
        selectable
        selectedKey={selected}
        onSelect={setSelected}
        onFilteredRowsChange={onFilteredRowsChange}
        sameValueTints
        defaultSort={{ col: 'pnl_sol', dir: 'desc' }}
        tableId="generic_combo_tokens"
        resetKey={`${comboId}_${showNotFired}`}
        loading={query.isFetching}
        emptyMessage="No token results for this combo."
      />

      {selectedRow && (
        <LabTokenInspectModal
          target={comboTarget(selectedRow)}
          titleSuffix="Sweep inspect"
          ruleOverride={
            comboParams
              ? { paramsJson: comboParams, fingerprintId: null, label: `combo #${comboId}` }
              : null
          }
          onClose={() => setSelected(null)}
        />
      )}
    </div>
  );
}

function groupBreadcrumb(g: GroupedSweepGroupRecord): string {
  const keys = Object.keys(g.group_key);
  if (keys.length === 0) return 'ALL tokens';
  return Object.entries(g.group_key)
    .map(([k, v]) => `${k}=${v}`)
    .join(' · ');
}
