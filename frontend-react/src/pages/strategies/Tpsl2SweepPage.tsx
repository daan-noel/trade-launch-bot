import { useMemo, useState } from 'react';
import { DataTable } from 'components/table/DataTable';
import { InlineAlert } from 'components/ui/Modal';
import { Badge } from 'components/ui/Badge';
import { Button } from 'components/ui/Button';
import { buildSweepColumns } from 'components/sweep/sweepColumns';
import {
  apiErrorMessage,
  useGetTpsl2SweepRunsQuery,
  useGetTpsl2SweepResultsQuery,
  useStartTpsl2SweepMutation,
} from 'store/apiSlice';

/**
 * Param-sweep results for the TPSL2 strategy: pick a sweep run, inspect the
 * ranked table of param pairs (sort/filter/paginate client-side).
 *
 * TPSL2-specific by design. Future strategies (TPSL1, …) get their own page
 * file and their own separate sweep endpoints — clone-and-tweak, like the
 * `tpsl_sniper_{1,2}` backend pattern.
 */
/**
 * The TPSL2 swept knobs, in the order the backend serializes them
 * (`Tpsl2Strategy::params_json`). Kept stable here — rather than derived
 * from the first loaded row — so the table's columns exist from first render.
 * That's what lets DataTable's `colToggle` persistence (keyed on column keys)
 * work like every other table; data-derived keys would be absent at mount and
 * get dropped from the saved set.
 */
const TPSL2_PARAM_KEYS = [
  'exit_take_profit',
  'exit_stop_loss',
  'exit_trailing_stop_pct',
  'exit_time_stop_secs',
  'exit_stall_secs',
  'entry_min_age_secs',
  'entry_pullback_pct',
  'entry_min_liquidity_sol',
];

export function Tpsl2SweepPage() {
  const runsQuery = useGetTpsl2SweepRunsQuery();
  const runs = runsQuery.data ?? [];

  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);
  const activeRunId = selectedRunId ?? runs[0]?.id ?? null;

  const [startSweep, startState] = useStartTpsl2SweepMutation();
  const startErr = apiErrorMessage(startState.error, 'Failed to start sweep');

  async function runOnLiveCache() {
    try {
      // Defaults: full grid over the live cache window (backend caps tokens).
      const run = await startSweep({}).unwrap();
      setSelectedRunId(run.id); // jump to the fresh run
    } catch {
      // Surfaced via startState.error (e.g. 409 = one already running).
    }
  }

  const resultsQuery = useGetTpsl2SweepResultsQuery(
    { runId: activeRunId ?? '' },
    { skip: !activeRunId },
  );
  const results = resultsQuery.data ?? [];

  // Stable column set (built once from the fixed TPSL2 param keys), so the
  // columns exist on first render — required for colToggle persistence to stick.
  const columns = useMemo(() => buildSweepColumns(TPSL2_PARAM_KEYS), []);

  const runsErr = apiErrorMessage(runsQuery.error, 'Failed to load sweep runs');
  const resultsErr = apiErrorMessage(resultsQuery.error, 'Failed to load sweep results');

  return (
    <div>
      <div className="mb-3.5 flex flex-wrap items-center gap-2.5">
        <h2 className="text-base font-bold text-primary">TPSL2 Param Sweep</h2>
        <Badge variant="primary" className="font-mono">
          {runs.length} runs · {results.length} param pairs
        </Badge>
        <div className="ml-auto flex items-center gap-2.5">
          {startState.isLoading && (
            <span className="text-sm text-text-dim">Running on live cache…</span>
          )}
          <Button
            variant="primary"
            onClick={runOnLiveCache}
            disabled={startState.isLoading}
            title="Run a sweep over the live in-memory token cache (no DB load)"
          >
            {startState.isLoading ? 'Sweeping…' : 'Run sweep (live cache)'}
          </Button>
        </div>
      </div>

      {startErr && <InlineAlert variant="error">{startErr}</InlineAlert>}
      {runsQuery.isLoading && <p className="text-text-dim">Loading sweep runs…</p>}
      {runsErr && <InlineAlert variant="error">{runsErr}</InlineAlert>}

      {!runsQuery.isLoading && !runsErr && runs.length === 0 && (
        <div className="rounded-md border border-white/10 bg-surface p-3 text-sm text-text-dim">
          No sweeps yet. Click <span className="text-primary">Run sweep (live cache)</span>{' '}
          above, or generate one offline with{' '}
          <code className="font-mono text-primary">
            cargo run -p backend -- tpsl2-sweep --source cache
          </code>
          .
        </div>
      )}

      {runs.length > 0 && (
        <>
          <div className="mb-4 flex flex-wrap items-center gap-2.5">
            <label className="text-sm text-text-dim" htmlFor="sweep-run">
              Run
            </label>
            <select
              id="sweep-run"
              className="rounded-md border border-white/10 bg-surface px-2.5 py-1.5 text-sm text-primary"
              value={activeRunId ?? ''}
              onChange={(e) => setSelectedRunId(e.target.value)}
            >
              {runs.map((r) => (
                <option key={r.id} value={r.id}>
                  {new Date(r.created_at).toLocaleString()} · {r.method} · {r.source} ·{' '}
                  {r.token_count} tokens × {r.combo_count} combos
                </option>
              ))}
            </select>
          </div>

          {resultsErr && <InlineAlert variant="error">{resultsErr}</InlineAlert>}

          <DataTable
            columns={columns}
            rows={results}
            rowKey={(r) => String(r.combo_id)}
            searchable={false}
            colFilters
            colToggle
            selectable={false}
            defaultPageSize={25}
            pageSizeOptions={[25, 50, 100]}
            storageKey="tpsl2_sweep_cols"
            resetKey={activeRunId ?? ''}
            loading={resultsQuery.isFetching}
            emptyMessage="No results for this run."
          />
        </>
      )}
    </div>
  );
}
