import { useEffect, useMemo, useState } from 'react';
import { DataTable } from 'components/table/DataTable';
import { InlineAlert } from 'components/ui/Modal';
import { Badge } from 'components/ui/Badge';
import { ProgressBar } from 'components/ui/ProgressBar';
import { connectSweepProgress } from 'services/sse';
import { cancelGroupedSweep } from 'services/api';
import { buildSweepColumns } from 'components/sweep/sweepColumns';
import { buildGroupColumns } from 'components/sweep/groupColumns';
import { SweepConfigForm } from 'components/sweep/SweepConfigForm';
import type { GroupedSweepStartArgs } from 'components/sweep/groupedTypes';
import {
  apiErrorMessage,
  useGetGroupedSweepRunsQuery,
  useGetGroupedSweepGroupsQuery,
  useGetGroupedSweepResultsQuery,
  useStartGroupedSweepMutation,
} from 'store/apiSlice';

/** This page is wired for TPSL2 today; a future strategy passes its own id (and
 *  its own param-key list) — the backend tables/engine are already generic. */
const STRATEGY_ID = 'tpsl2';

/** TPSL2 swept knobs in serialization order — kept stable (not data-derived) so
 *  the drill-in table's columns exist on first render (colToggle persistence). */
const TPSL2_PARAM_KEYS = [
  'exit_take_profit',
  'exit_stop_loss',
  'exit_trailing_stop_pct',
  'exit_time_stop_secs',
  'exit_stall_secs',
  'exit_liquidity_drop_pct',
  'exit_cohort_ratio',
  'entry_min_age_secs',
  'entry_min_alive_sol',
  'entry_min_organic_sol',
  'entry_pullback_pct',
  'entry_higher_low_secs',
  'entry_max_cohort_held',
  'entry_min_liquidity_sol',
  'entry_min_organic_liq',
];

/**
 * Grouped param-sweep: select tokens by a created-at range, partition them by a
 * fingerprint key, sweep each group, and rank combos by expectancy per trade.
 * Flow: configure + Run → pick a run → group-summary table → click a group →
 * drill into its full ranked combo table (reuses the TPSL2 sweep columns).
 */
/** Honest progress bar for the in-flight grouped sweep. Subscribes to the
 *  `sweep_progress` SSE stream (real `processed / total` tokens folded across
 *  groups) and offers a Cancel button — the backend cancel is cooperative, so
 *  the bar stays mounted (showing "Cancelling…") until the run actually ends. */
function SweepProgressBar({ strategyId }: { strategyId: string }) {
  const [progress, setProgress] = useState<{ processed: number; total: number } | null>(null);
  const [cancelling, setCancelling] = useState(false);
  useEffect(() => {
    setProgress(null);
    setCancelling(false);
    const h = connectSweepProgress(strategyId, (processed, total) =>
      setProgress({ processed, total }),
    );
    return () => h.close();
  }, [strategyId]);
  return (
    <ProgressBar
      label="Running Grouped Sweep"
      processed={progress?.processed ?? null}
      total={progress?.total ?? null}
      cancelling={cancelling}
      onCancel={() => {
        setCancelling(true);
        cancelGroupedSweep().catch(() => setCancelling(false));
      }}
    />
  );
}

export function GroupedSweepPage() {
  const runsQuery = useGetGroupedSweepRunsQuery({ strategyId: STRATEGY_ID });
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

  async function run(args: GroupedSweepStartArgs) {
    try {
      const created = await startSweep(args).unwrap();
      if ('cancelled' in created) return; // user cancelled — no run persisted
      setSelectedRunId(created.id); // jump to the fresh run
    } catch {
      // Surfaced via startState.error (e.g. 409 = one already running).
    }
  }

  const groupsQuery = useGetGroupedSweepGroupsQuery(
    { strategyId: STRATEGY_ID, runId: activeRunId ?? '' },
    { skip: !activeRunId },
  );
  const groups = groupsQuery.data ?? [];
  const activeGroup = groups.find((g) => g.id === activeGroupId) ?? null;

  const resultsQuery = useGetGroupedSweepResultsQuery(
    { strategyId: STRATEGY_ID, runId: activeRunId ?? '', groupId: activeGroupId ?? '' },
    { skip: !activeRunId || !activeGroupId },
  );
  const results = resultsQuery.data ?? [];

  const groupColumns = useMemo(() => buildGroupColumns(), []);
  const comboColumns = useMemo(() => buildSweepColumns(TPSL2_PARAM_KEYS), []);

  const runsErr = apiErrorMessage(runsQuery.error, 'Failed to load sweep runs');
  const groupsErr = apiErrorMessage(groupsQuery.error, 'Failed to load groups');
  const resultsErr = apiErrorMessage(resultsQuery.error, 'Failed to load combo results');

  return (
    <div>
      <div className="mb-3.5 flex flex-wrap items-center gap-2.5">
        <h2 className="text-base font-bold text-primary">Grouped Param Sweep</h2>
        <Badge variant="primary" className="font-mono">
          {runs.length} runs · {groups.length} groups
        </Badge>
      </div>

      <SweepConfigForm strategyId={STRATEGY_ID} running={startState.isLoading} onRun={run} />

      {startState.isLoading && <SweepProgressBar strategyId={STRATEGY_ID} />}

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
          </div>

          {groupsErr && <InlineAlert variant="error">{groupsErr}</InlineAlert>}

          <DataTable
            columns={groupColumns}
            rows={groups}
            rowKey={(g) => g.id}
            defaultSort={{ col: 'best_expectancy_sol', dir: 'desc' }}
            searchable
            colFilters
            selectable
            selectedKey={activeGroupId}
            onSelect={setActiveGroupId}
            defaultPageSize={25}
            pageSizeOptions={[25, 50, 100]}
            storageKey="grouped_sweep_group_cols"
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
                defaultSort={{ col: 'score', dir: 'desc' }}
                searchable={false}
                colFilters
                colToggle
                selectable={false}
                defaultPageSize={25}
                pageSizeOptions={[25, 50, 100]}
                storageKey="grouped_sweep_combo_cols"
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
