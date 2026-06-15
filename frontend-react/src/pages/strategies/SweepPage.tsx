import { useMemo, useState } from 'react';
import { DataTable } from 'components/table/DataTable';
import { InlineAlert } from 'components/ui/Modal';
import { Badge } from 'components/ui/Badge';
import { buildSweepColumns } from 'components/sweep/sweepColumns';
import {
  apiErrorMessage,
  useGetSweepRunsQuery,
  useGetSweepResultsQuery,
} from 'store/apiSlice';

/**
 * Param-sweep results for one strategy: pick a sweep run, inspect the ranked
 * table of param pairs (sort/filter/paginate client-side). Strategy-agnostic —
 * a new strategy's sweep page is just another `<SweepPage strategy=… />` route.
 */
export function SweepPage({
  strategy,
  title,
}: {
  strategy: 'tpsl1' | 'tpsl2';
  title: string;
}) {
  const runsQuery = useGetSweepRunsQuery({ strategy });
  const runs = runsQuery.data ?? [];

  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);
  const activeRunId = selectedRunId ?? runs[0]?.id ?? null;

  const resultsQuery = useGetSweepResultsQuery(
    { strategy, runId: activeRunId ?? '' },
    { skip: !activeRunId },
  );
  const results = resultsQuery.data ?? [];

  // Param columns are derived from a run's param keys, so the table adapts to
  // whatever knobs the strategy swept.
  const columns = useMemo(
    () => buildSweepColumns(results.length ? Object.keys(results[0].params) : []),
    [results],
  );

  const runsErr = apiErrorMessage(runsQuery.error, 'Failed to load sweep runs');
  const resultsErr = apiErrorMessage(resultsQuery.error, 'Failed to load sweep results');

  return (
    <div>
      <div className="mb-3.5 flex flex-wrap items-center gap-2.5">
        <h2 className="text-base font-bold text-primary">{title}</h2>
        <Badge variant="primary" className="font-mono">
          {runs.length} runs · {results.length} param pairs
        </Badge>
      </div>

      {runsQuery.isLoading && <p className="text-text-dim">Loading sweep runs…</p>}
      {runsErr && <InlineAlert variant="error">{runsErr}</InlineAlert>}

      {!runsQuery.isLoading && !runsErr && runs.length === 0 && (
        <div className="rounded-md border border-white/10 bg-surface p-3 text-sm text-text-dim">
          No sweeps yet. Generate one with{' '}
          <code className="font-mono text-primary">
            cargo run -p backend -- sweep --strategy {strategy} --source cache
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
            storageKey={`${strategy}_sweep_cols`}
            resetKey={activeRunId ?? ''}
            loading={resultsQuery.isFetching}
            emptyMessage="No results for this run."
          />
        </>
      )}
    </div>
  );
}
