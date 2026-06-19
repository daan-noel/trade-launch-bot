import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useLocalStorage } from 'hooks/useLocalStorage';
import { STORAGE_KEYS } from 'lib/storage';
import { DataTable } from 'components/table/DataTable';
import { InlineAlert } from 'components/ui/Modal';
import { SectionDivider } from 'components/ui/SectionDivider';
import { Badge } from 'components/ui/Badge';
import { Button } from 'components/ui/Button';
import { Accordion } from 'components/ui/Accordion';
import { useBackgroundJobActions, useBackgroundJobsState } from 'context/BackgroundJobsContext';
import { buildSweepColumns } from 'components/sweep/sweepColumns';
import { buildGroupColumns } from 'components/sweep/groupColumns';
import { computeParamColumnColors } from 'lib/sweepParamColors';
import { SweepConfigForm } from 'components/sweep/SweepConfigForm';
import { SelectedSweepHistory } from 'components/sweep/SelectedSweepHistory';
import {
  type AxisDef,
  type GroupedSweepStartArgs,
  type GroupedSweepRunRecord,
  type ComboTokenResult,
} from 'components/sweep/groupedTypes';
import {
  apiErrorMessage,
  useGetGroupedSweepRunsQuery,
  useGetGroupedSweepGroupsQuery,
  useGetComboTokenResultsQuery,
  useStartGroupedSweepMutation,
  useDeleteGroupedSweepRunMutation,
  usePruneGroupedSweepsMutation,
} from 'store/apiSlice';
import { useStreamedSweepResults, COMBO_PAGE_SIZE } from 'hooks/useStreamedSweepResults';
import type { ColumnDef, TableQuery } from 'components/table/types';

/** The grouped-sweep view is strategy-agnostic — the API/data layer and column
 *  builders are all driven by `strategyId` + a swept-param-key list. Each
 *  strategy supplies its own keys + axes via a thin child page (see this
 *  folder's `Tpsl1GroupedSweepPage` / `Tpsl2GroupedSweepPage`). */


/** Run-picker groups label: a completed run shows its full group count; a running
 *  or cancelled (partial) run shows "done / total" so the picker reveals at a
 *  glance that it isn't a full sweep. */
function runGroupsLabel(r: GroupedSweepRunRecord): string {
  if (r.status === 'completed') return `${r.group_count} groups`;
  const tag = r.status === 'running' ? 'running' : 'partial';
  return `${tag} ${r.groups_done}/${r.group_count} groups`;
}

export interface GroupedSweepViewProps {
  /** Resolves the per-strategy backend tables + sweep entry point. */
  strategyId: string;
  /** Swept-param keys, in column order (matches the backend `params_json`). */
  paramKeys: string[];
  /** This strategy's editable param axes for the config form. */
  axes: AxisDef[];
  /** localStorage key for this strategy's persisted form config. */
  storageKey: string;
  /** Page heading. */
  title: string;
}

/**
 * Grouped param-sweep view: select tokens by a created-at range, partition them
 * by a fingerprint key, sweep each group, and rank combos by expectancy per
 * trade. Flow: configure + Run → pick a run → group-summary table → click a
 * group → drill into its full ranked combo table.
 */
export function GroupedSweepView({
  strategyId,
  paramKeys,
  axes,
  storageKey,
  title,
}: GroupedSweepViewProps) {
  const runsQuery = useGetGroupedSweepRunsQuery({ strategyId });
  const runs = runsQuery.data ?? [];

  const [selectedRunId, setSelectedRunId] = useLocalStorage<string | null>(
    `${STORAGE_KEYS.sweepSel}.${strategyId}`,
    null,
  );
  // Fall back to the newest run when the stored id is stale (run deleted) or
  // nothing has been selected yet.
  const activeRunId = (selectedRunId && runs.some((r) => r.id === selectedRunId))
    ? selectedRunId
    : (runs[0]?.id ?? null);

  const [activeGroupId, setActiveGroupId] = useState<string | null>(null);
  // A new run invalidates the drilled-in group.
  useEffect(() => {
    setActiveGroupId(null);
  }, [activeRunId]);

  const [activeComboId, setActiveComboId] = useState<number | null>(null);
  // A new group invalidates the drilled-in combo.
  useEffect(() => {
    setActiveComboId(null);
  }, [activeGroupId]);

  const [startSweep, startState] = useStartGroupedSweepMutation();
  const startErr = apiErrorMessage(startState.error, 'Failed to start sweep');
  // The sweep's running/progress state lives in the app-wide jobs registry so it
  // survives navigation (the run continues on the backend regardless); the global
  // indicator renders the progress bar + cancel. The page only needs "is it
  // running" to gate the form.
  const { markStarting } = useBackgroundJobActions();
  const { isRunning } = useBackgroundJobsState();
  const sweepRunning = isRunning('sweep', 'sweep') || startState.isLoading;

  const [deleteRun, deleteState] = useDeleteGroupedSweepRunMutation();
  const [pruneRuns, pruneState] = usePruneGroupedSweepsMutation();
  const deleteErr = apiErrorMessage(
    deleteState.error ?? pruneState.error,
    'Failed to delete sweep history',
  );
  // "older than" cutoff for the prune control (a yyyy-mm-dd date).
  const [pruneBefore, setPruneBefore] = useState('');

  // Re-run: bumping this nonce makes the config form adopt the selected run's
  // stored settings; `formRef` scrolls it back into view so the user can review
  // before clicking Run (re-run never auto-fires — a sweep is expensive).
  const [reuseNonce, setReuseNonce] = useState(0);
  const formRef = useRef<HTMLDivElement>(null);

  async function onDeleteRun() {
    if (!activeRunId) return;
    if (!window.confirm('Delete this sweep run and all its groups/results?')) return;
    try {
      await deleteRun({ strategyId, runId: activeRunId }).unwrap();
      setSelectedRunId(null); // fall back to the newest remaining run
    } catch {
      // Surfaced via deleteErr.
    }
  }

  async function onPrune() {
    if (!pruneBefore) return;
    const beforeIso = new Date(pruneBefore).toISOString();
    if (!window.confirm(`Delete all sweep runs created before ${pruneBefore}?`)) return;
    try {
      await pruneRuns({ strategyId, before: beforeIso }).unwrap();
      setSelectedRunId(null);
    } catch {
      // Surfaced via deleteErr.
    }
  }

  async function run(args: GroupedSweepStartArgs) {
    // Register the job immediately so the global indicator shows before the first
    // SSE frame (which only arrives once the backend finishes corpus selection).
    markStarting('sweep', 'sweep', 'Grouped sweep');
    try {
      const created = await startSweep(args).unwrap();
      if ('cancelled' in created) {
        // Cancelled mid-sweep — the groups that finished are kept as a partial
        // run; jump to it (if any persisted) so the user sees what completed.
        if (created.groups_done > 0) setSelectedRunId(created.run_id);
        return;
      }
      setSelectedRunId(created.id); // jump to the fresh run
    } catch {
      // Surfaced via startState.error (e.g. 409 = one already running).
    }
  }

  const groupsQuery = useGetGroupedSweepGroupsQuery(
    { strategyId, runId: activeRunId ?? '' },
    { skip: !activeRunId },
  );
  const groups = groupsQuery.data ?? [];
  const activeGroup = groups.find((g) => g.id === activeGroupId) ?? null;
  const activeRun = runs.find((r) => r.id === activeRunId) ?? null;

  // Server-side pagination state for the combo table. `onComboQueryChange` is
  // stable (useCallback) so DataTable's onQueryChange prop identity doesn't
  // churn on every parent re-render, which would reset the pager.
  const [comboPage, setComboPage] = useState(0);
  const [comboPageSize, setComboPageSize] = useState(COMBO_PAGE_SIZE);
  const onComboQueryChange = useCallback((q: TableQuery) => {
    setComboPage(q.page - 1); // DataTable pages are 1-based; hook is 0-based
    setComboPageSize(q.pageSize);
  }, []);

  // Reset to page 0 whenever the selected group changes.
  useEffect(() => {
    setComboPage(0);
  }, [activeGroupId]);

  const { rows: results, total: resultsTotal, loading: resultsLoading, error: resultsErr } =
    useStreamedSweepResults(strategyId, activeRunId, activeGroupId, comboPage, comboPageSize);

  const groupColumns = useMemo(() => buildGroupColumns(paramKeys), [paramKeys]);
  // Per-column tint plan for the drill-in combo table: constant knobs dim out,
  // varying knobs get a per-value cell band so near-identical combos read at a
  // glance. Recomputed per group (cheap, O(rows×params)).
  const paramColors = useMemo(
    () => computeParamColumnColors(results, paramKeys),
    [results, paramKeys],
  );
  const comboColumns = useMemo(
    () => buildSweepColumns(paramKeys, paramColors),
    [paramKeys, paramColors],
  );

  const tokenResultsQuery = useGetComboTokenResultsQuery(
    {
      strategyId,
      runId: activeRunId ?? '',
      groupId: activeGroupId ?? '',
      comboId: activeComboId ?? 0,
    },
    { skip: !activeRunId || !activeGroupId || activeComboId === null },
  );
  const tokenResults = tokenResultsQuery.data ?? [];
  const tokenResultsErr = apiErrorMessage(tokenResultsQuery.error, 'Failed to load token results');

  const tokenColumns = useMemo<ColumnDef<ComboTokenResult>[]>(
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
        key: 'mint',
        label: 'Mint',
        render: (r) => (
          <span className="font-mono text-xs text-text-dim" title={r.mint}>
            {r.mint.slice(0, 8)}…{r.mint.slice(-4)}
          </span>
        ),
        searchValue: (r) => r.mint,
        sortable: false,
      },
      {
        key: 'fired',
        label: 'Fired',
        render: (r) => (
          <span className={r.fired ? 'text-success' : 'text-text-dim'}>
            {r.fired ? 'Yes' : 'No'}
          </span>
        ),
        searchValue: (r) => (r.fired ? 'yes' : 'no'),
        sortValue: (r) => (r.fired ? 1 : 0),
        sortable: true,
      },
      {
        key: 'pnl_sol',
        label: 'PnL (SOL)',
        render: (r) => (
          <span className={r.pnl_sol > 0 ? 'text-success' : r.pnl_sol < 0 ? 'text-danger' : 'text-text-dim'}>
            {r.fired ? r.pnl_sol.toFixed(4) : '—'}
          </span>
        ),
        searchValue: () => '',
        sortValue: (r) => r.pnl_sol,
        sortable: true,
      },
      {
        key: 'pnl_pct',
        label: 'PnL %',
        render: (r) => (
          <span className={r.pnl_pct > 0 ? 'text-success' : r.pnl_pct < 0 ? 'text-danger' : 'text-text-dim'}>
            {r.fired ? `${r.pnl_pct.toFixed(1)}%` : '—'}
          </span>
        ),
        searchValue: () => '',
        sortValue: (r) => r.pnl_pct,
        sortable: true,
      },
      {
        key: 'holding_secs',
        label: 'Hold (s)',
        render: (r) => (
          <span className="text-text-dim">
            {r.fired ? r.holding_secs : '—'}
          </span>
        ),
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

  const runsErr = apiErrorMessage(runsQuery.error, 'Failed to load sweep runs');
  const groupsErr = apiErrorMessage(groupsQuery.error, 'Failed to load groups');

  return (
    <div>
      <div className="mb-3.5 flex flex-wrap items-center gap-2.5">
        <h2 className="text-base font-bold text-primary">{title}</h2>
        <Badge variant="primary" className="font-mono">
          {runs.length} runs · {groups.length} groups
        </Badge>
      </div>

      <Accordion title="Configure sweep">
        <div ref={formRef}>
          <SweepConfigForm
            strategyId={strategyId}
            axes={axes}
            storageKey={storageKey}
            running={sweepRunning}
            onRun={run}
            reuseNonce={reuseNonce}
            reuseRun={activeRun}
          />
        </div>
      </Accordion>

      {startErr && <InlineAlert variant="error">{startErr}</InlineAlert>}
      {runsQuery.isLoading && <p className="text-text-dim">Loading sweep runs…</p>}
      {runsErr && <InlineAlert variant="error">{runsErr}</InlineAlert>}

      {!runsQuery.isLoading && !runsErr && runs.length === 0 && (
        <div className="rounded-md border border-white/10 bg-surface p-3 text-sm text-text-dim">
          No grouped sweeps yet. Set a date range + grouping above and click{' '}
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
              header={<div className="flex flex-1 flex-wrap items-center gap-2.5">
                <label className="text-sm text-text-dim" htmlFor="grouped-sweep-run">
                  Run
                </label>
                <select
                  id="grouped-sweep-run"
                  className="rounded-md border border-white/10 bg-surface px-2.5 py-1.5 text-sm text-primary"
                  value={activeRunId ?? ''}
                  onChange={(e) => setSelectedRunId(e.target.value)}
                >
                  {runs.map((r) => (
                    <option key={r.id} value={r.id}>
                      {r.label ? `${r.label} · ` : ''}
                      {new Date(r.created_at).toLocaleString()} · {r.method} ·{' '}
                      {r.grouping_spec.length ? r.grouping_spec.join('+') : 'ALL'} ·{' '}
                      {r.token_count} tokens · {runGroupsLabel(r)} × {r.combo_count} combos
                    </option>
                  ))}
                </select>

                <Button
                  variant="danger"
                  size="sm"
                  disabled={!activeRunId || deleteState.isLoading}
                  onClick={onDeleteRun}
                >
                  {deleteState.isLoading ? 'Deleting…' : 'Delete Run'}
                </Button>

                <span className="ml-auto flex items-center gap-2">
                  <label className="text-sm text-text-dim" htmlFor="grouped-sweep-prune">
                    Clear runs before
                  </label>
                  <input
                    id="grouped-sweep-prune"
                    type="date"
                    className="rounded-md border border-white/10 bg-surface px-2.5 py-1.5 text-sm text-primary"
                    value={pruneBefore}
                    onChange={(e) => setPruneBefore(e.target.value)}
                  />
                  <Button
                    variant="danger"
                    size="sm"
                    disabled={!pruneBefore || pruneState.isLoading}
                    onClick={onPrune}
                  >
                    {pruneState.isLoading ? 'Clearing…' : 'Clear All OLD'}
                  </Button>
                </span>
              </div>}
              defaultOpen={false} className="mb-3">
              <SelectedSweepHistory
                strategyId={strategyId}
                run={activeRun}
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
              {activeRun.groups_done} of {activeRun.group_count} groups
              {activeRun.status === 'running'
                ? ' persisted so far. The sweep is still running; more groups will appear as they finish.'
                : ' completed before the run was cancelled. The remaining groups were not swept — this is not a full sweep.'}
            </InlineAlert>
          )}

          <DataTable
            columns={groupColumns}
            rows={groups}
            rowKey={(g) => g.id}
            groupLabels={{ metrics: 'Metrics', entry: 'Entry', exit: 'Exit' }}
            defaultSort={{ col: 'best_score', dir: 'desc' }}
            searchable
            colFilters
            colToggle
            selectable
            selectedKey={activeGroupId}
            onSelect={setActiveGroupId}
            defaultPageSize={5}
            pageSizeOptions={[5, 10, 25]}
            tableId={`${strategyId}_sweep_groups`}
            resetKey={activeRunId ?? ''}
            loading={groupsQuery.isFetching}
            emptyMessage="No groups cleared the min-tokens threshold for this run."
          />

          {activeGroupId && (
            <div className="mt-12">
              <div className="mb-2 flex flex-wrap items-center gap-2">
                <h3 className="text-sm font-bold text-secondary">Combos for group</h3>
                {activeGroup && (
                  <span className="font-mono text-xs text-text-dim">
                    {Object.keys(activeGroup.group_key).length
                      ? Object.entries(activeGroup.group_key)
                        .map(([k, v]) => `${k}=${v}`)
                        .join(' · ')
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
                groupLabels={{
                  params: 'Params',
                  counts: 'Counts',
                  pnl: 'PnL',
                  holding: 'Holding',
                  exits: 'Exit reasons',
                }}
                searchable={false}
                colFilters={false}
                colToggle
                selectable
                selectedKey={activeComboId !== null ? String(activeComboId) : null}
                onSelect={(key) => setActiveComboId(key !== null ? Number(key) : null)}
                serverSide
                serverTotal={resultsTotal}
                onQueryChange={onComboQueryChange}
                defaultPageSize={COMBO_PAGE_SIZE}
                pageSizeOptions={[100, 200, 500]}
                tableId={`${strategyId}_sweep_combos`}
                resetKey={activeGroupId ?? ''}
                loading={resultsLoading}
                emptyMessage="No combo results for this group."
              />

              {activeComboId !== null && (
                <div className="mt-10">
                  <div className="mb-2 flex flex-wrap items-center gap-2">
                    <h3 className="text-sm font-bold text-secondary">Tokens for combo #{activeComboId}</h3>
                    <span className="text-xs text-text-dim">
                      {tokenResultsQuery.isFetching
                        ? 'Simulating…'
                        : `${tokenResults.length} tokens`}
                    </span>
                    <button
                      className="ml-auto text-xs text-text-dim hover:text-primary"
                      onClick={() => setActiveComboId(null)}
                    >
                      ✕ Close
                    </button>
                  </div>

                  {tokenResultsErr && (
                    <InlineAlert variant="error">{tokenResultsErr}</InlineAlert>
                  )}

                  <DataTable
                    columns={tokenColumns}
                    rows={tokenResults}
                    rowKey={(r) => r.mint}
                    searchable
                    colFilters={false}
                    colToggle={false}
                    selectable={false}
                    defaultSort={{ col: 'pnl_sol', dir: 'desc' }}
                    defaultPageSize={25}
                    pageSizeOptions={[25, 50, 100]}
                    tableId={`${strategyId}_combo_tokens`}
                    resetKey={String(activeComboId)}
                    loading={tokenResultsQuery.isFetching}
                    emptyMessage="No token results for this combo."
                  />
                </div>
              )}
            </div>
          )}

        </>
      )}
    </div>
  );
}
