import { useEffect, useMemo, useState } from 'react';
import { DataTable } from 'components/table/DataTable';
import { InlineAlert } from 'components/ui/Modal';
import { Badge } from 'components/ui/Badge';
import { Button } from 'components/ui/Button';
import { useBackgroundJobActions, useBackgroundJobsState } from 'context/BackgroundJobsContext';
import { buildSweepColumns } from 'components/sweep/sweepColumns';
import { buildGroupColumns } from 'components/sweep/groupColumns';
import { SweepConfigForm } from 'components/sweep/SweepConfigForm';
import {
  TPSL1_AXES,
  TPSL2_AXES,
  type AxisDef,
  type GroupedSweepStartArgs,
} from 'components/sweep/groupedTypes';
import { STORAGE_KEYS } from 'lib/storage';
import {
  apiErrorMessage,
  useGetGroupedSweepRunsQuery,
  useGetGroupedSweepGroupsQuery,
  useGetGroupedSweepResultsQuery,
  useStartGroupedSweepMutation,
  useDeleteGroupedSweepRunMutation,
  usePruneGroupedSweepsMutation,
} from 'store/apiSlice';

/** The grouped-sweep page is strategy-agnostic — the API/data layer and column
 *  builders are all driven by `strategyId` + a swept-param-key list. Each
 *  strategy supplies its own keys + axes via a thin wrapper at the bottom. */

/** TPSL2 swept knobs — this array IS the param column order in the combo table
 *  (`buildSweepColumns` renders them in this order). Kept stable (not
 *  data-derived) so the columns exist on first render (colToggle persistence).
 *  Ordered to match the rule modal: TP/SL lead, entry gates next, then the
 *  trailing/time/stall exit knobs. MUST match the backend `params_json` keys. */
const TPSL2_PARAM_KEYS = [
  'exit_take_profit',
  'exit_stop_loss',
  'entry_min_age_secs',
  'entry_min_alive_sol',
  'entry_min_organic_sol',
  'entry_pullback_pct',
  'entry_higher_low_secs',
  'entry_max_cohort_held',
  'entry_min_liquidity_sol',
  'entry_min_organic_liq',
  'exit_trailing_stop_pct',
  'exit_time_stop_secs',
  'exit_stall_secs',
  'exit_liquidity_drop_pct',
  'exit_cohort_ratio',
];

/** TPSL1 swept knobs — the exit ladder only (no scalp entry gates, no cohort
 *  exit). MUST match the backend tpsl1 `params_json` keys. */
const TPSL1_PARAM_KEYS = [
  'exit_take_profit',
  'exit_stop_loss',
  'exit_trailing_stop_pct',
  'exit_time_stop_secs',
  'exit_stall_secs',
  'exit_liquidity_drop_pct',
];

function SectionDivider() {
  return <div role="separator" className="my-6 border-t border-white/6" />;
}

interface GroupedSweepViewProps {
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
function GroupedSweepView({ strategyId, paramKeys, axes, storageKey, title }: GroupedSweepViewProps) {
  const runsQuery = useGetGroupedSweepRunsQuery({ strategyId });
  const runs = runsQuery.data ?? [];

  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);
  const activeRunId = selectedRunId ?? runs[0]?.id ?? null;

  const [activeGroupId, setActiveGroupId] = useState<string | null>(null);
  // A new run invalidates the drilled-in group.
  useEffect(() => {
    setActiveGroupId(null);
  }, [activeRunId]);

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
      if ('cancelled' in created) return; // user cancelled — no run persisted
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

  const resultsQuery = useGetGroupedSweepResultsQuery(
    { strategyId, runId: activeRunId ?? '', groupId: activeGroupId ?? '' },
    { skip: !activeRunId || !activeGroupId },
  );
  const results = resultsQuery.data ?? [];

  const groupColumns = useMemo(() => buildGroupColumns(paramKeys), [paramKeys]);
  const comboColumns = useMemo(() => buildSweepColumns(paramKeys), [paramKeys]);

  const runsErr = apiErrorMessage(runsQuery.error, 'Failed to load sweep runs');
  const groupsErr = apiErrorMessage(groupsQuery.error, 'Failed to load groups');
  const resultsErr = apiErrorMessage(resultsQuery.error, 'Failed to load combo results');

  return (
    <div>
      <div className="mb-3.5 flex flex-wrap items-center gap-2.5">
        <h2 className="text-base font-bold text-primary">{title}</h2>
        <Badge variant="primary" className="font-mono">
          {runs.length} runs · {groups.length} groups
        </Badge>
      </div>

      <SweepConfigForm
        strategyId={strategyId}
        axes={axes}
        storageKey={storageKey}
        running={sweepRunning}
        onRun={run}
      />

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
          <div className="mb-4 flex flex-wrap items-center gap-2.5">
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
                  {new Date(r.created_at).toLocaleString()} · {r.method} ·{' '}
                  {r.grouping_spec.length ? r.grouping_spec.join('+') : 'ALL'} ·{' '}
                  {r.token_count} tokens · {r.group_count} groups × {r.combo_count} combos
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
          </div>

          {deleteErr && <InlineAlert variant="error">{deleteErr}</InlineAlert>}
          {groupsErr && <InlineAlert variant="error">{groupsErr}</InlineAlert>}

          <DataTable
            columns={groupColumns}
            rows={groups}
            rowKey={(g) => g.id}
            groupLabels={{ metrics: 'Metrics', entry: 'Entry', exit: 'Exit' }}
            defaultSort={{ col: 'best_expectancy_sol', dir: 'desc' }}
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
            <div className="mt-5">
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
                defaultSort={{ col: 'score', dir: 'desc' }}
                searchable={false}
                colFilters
                colToggle
                selectable={false}
                defaultPageSize={25}
                pageSizeOptions={[25, 50, 100]}
                tableId={`${strategyId}_sweep_combos`}
                resetKey={activeGroupId}
                loading={resultsQuery.isFetching}
                emptyMessage="No combo results for this group."
              />
            </div>
          )}
        </>
      )}
    </div>
  );
}

/** TPSL2 grouped sweep — the full entry-gate + exit-ladder param space. */
export function GroupedSweepPage() {
  return (
    <GroupedSweepView
      strategyId="tpsl2"
      paramKeys={TPSL2_PARAM_KEYS}
      axes={TPSL2_AXES}
      storageKey={STORAGE_KEYS.sweepConfig}
      title="Grouped Param Sweep · TPSL2"
    />
  );
}

/** TPSL1 grouped sweep — the exit-ladder-only param space (no scalp entry gates,
 *  no cohort exit). Reuses the same generic view, API layer, and columns. */
export function Tpsl1GroupedSweepPage() {
  return (
    <GroupedSweepView
      strategyId="tpsl1"
      paramKeys={TPSL1_PARAM_KEYS}
      axes={TPSL1_AXES}
      storageKey={`${STORAGE_KEYS.sweepConfig}.tpsl1`}
      title="Grouped Param Sweep · TPSL1"
    />
  );
}
