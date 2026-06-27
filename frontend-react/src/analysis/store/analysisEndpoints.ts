import { baseApi } from 'store/baseApi';
import type {
  GroupedSweepRunRecord,
  GroupedSweepGroupRecord,
  GroupedSweepResultRecord,
  GroupedSweepStartArgs,
  ComboTokenResult,
} from '@analysis/components/sweep/groupedTypes';
import type {
  GroupedCreationArgs,
  GroupedCreationResponse,
} from 'components/dashboard/groupedCreationStats';
import type {
  AnalysisRecord,
  CreatorRecord,
  MatchedTokensResponse,
  PaperResultResponse,
  SimulatedTokenResult,
} from 'types';

/** Args for the per-rule strategy result reads (matched / simulate / paper),
 *  shared by both strategy pages so tpsl1 and tpsl2 keep distinct cache keys. */
export interface StrategyRuleArg {
  strategy: 'tpsl1' | 'tpsl2';
  ruleId: string;
  /**
   * Optional transient creation-time window for `matched` / `simulate` only
   * (ISO strings; empty = all-time). Not persisted on the rule â€” it scopes the
   * backend's full-`tokens`-table scan. Part of the arg, so different ranges
   * cache separately while a rule edit still invalidates the whole pair.
   * Ignored by `paper-result`.
   */
  from?: string;
  to?: string;
}

/** Append `?from=&to=` (only the bounds that are set) to an analysis-endpoint URL. */
const withAnalysisRange = (url: string, { from, to }: StrategyRuleArg): string => {
  const qs = new URLSearchParams();
  if (from) qs.set('from', from);
  if (to) qs.set('to', to);
  const s = qs.toString();
  return s ? `${url}?${s}` : url;
};

/** Cache tag for a rule's `matched` + `simulate` results â€” both derive from the
 *  rule's entry criteria, so editing the rule invalidates the pair. */
const strategyResultTag = (a: StrategyRuleArg) =>
  ({ type: 'StrategyResult', id: `${a.strategy}:${a.ruleId}` }) as const;

/** Offset-paginated list args shared by the simple `{ total, items }`
 *  endpoints (creators / analysis) that page server-side via limit+offset. */
export interface OffsetPageArgs {
  limit: number;
  offset: number;
}

export interface CreatorsResponse {
  total: number;
  items: CreatorRecord[];
}

export interface AnalysisResponse {
  total: number;
  items: AnalysisRecord[];
}

/**
 * Analysis-only RTK Query endpoints â€” bundled exclusively in the analysis
 * (local) build: grouped param-sweeps, per-rule strategy simulate/paper reads,
 * grouped creation stats, and the creators/analysis page lists. The deploy
 * backend serves none of these.
 */
export const analysisApi = baseApi.injectEndpoints({
  endpoints: (builder) => ({
    // Per-rule strategy result reads. Driven imperatively from the strategy
    // pages via `endpoints.X.initiate` (the pages keep their own open/toggle
    // state), so folding them into RTK Query buys dedupe + structural sharing +
    // a short retention: re-opening a view or switching rules and back reuses
    // the cache instead of re-hitting the backend. `matched`/`simulate` are
    // tagged by rule so a rule edit can invalidate them; `paper-result` is
    // force-refetched on the paper-finished SSE event.
    getStrategyMatched: builder.query<MatchedTokensResponse, StrategyRuleArg>({
      query: (a) =>
        withAnalysisRange(
          `/api/strategies/${a.strategy}/rules/${encodeURIComponent(a.ruleId)}/matched`,
          a,
        ),
      providesTags: (_r, _e, a) => [strategyResultTag(a)],
      keepUnusedDataFor: 60,
    }),
    // Collect a finished backtest's result. The simulation is started separately
    // (`startSimulation`, a detached job) and its result stored server-side, so
    // this endpoint just picks it up once the `simulation_finished` SSE fires â€”
    // no long-held connection that a minutes-long run could fail with a
    // `FETCH_ERROR`. Strategy-agnostic URL (keyed by `rule_id`), but the cache key
    // / tag stays per `strategy:ruleId` so a rule edit invalidates it and tpsl1 /
    // tpsl2 don't share an entry. A cancelled run resolves to `{ cancelled: true }`;
    // callers type-guard on the shape. Driven imperatively from
    // `fetchSimulateCached` â€” see `store/strategyResultCache.ts`.
    getStrategySimulateResult: builder.query<
      SimulatedTokenResult[] | { cancelled: true },
      StrategyRuleArg
    >({
      query: (a) => `/api/jobs/simulations/${encodeURIComponent(a.ruleId)}/result`,
      providesTags: (_r, _e, a) => [strategyResultTag(a)],
      keepUnusedDataFor: 60,
    }),
    getStrategyPaperResult: builder.query<PaperResultResponse, StrategyRuleArg>({
      query: ({ strategy, ruleId }) =>
        `/api/strategies/${strategy}/rules/${encodeURIComponent(ruleId)}/paper-result`,
      providesTags: (_r, _e, a) => [
        { type: 'StrategyPaper', id: `${a.strategy}:${a.ruleId}` },
      ],
    }),
    // Grouped param-sweeps (generic across strategies; `strategy_id` resolves the
    // per-strategy tables). A run partitions its corpus by a fingerprint key and
    // ranks combos PER group. Runs/groups are bounded, so the page pulls them
    // whole; a group's combo rows are fetched lazily on drill-in. Cached so
    // flipping between runs/groups reuses the data.
    getGroupedSweepRuns: builder.query<
      GroupedSweepRunRecord[],
      { strategyId: string; limit?: number }
    >({
      query: ({ strategyId, limit }) =>
        `/api/strategies/sweeps?strategy_id=${encodeURIComponent(strategyId)}&limit=${limit ?? 50}`,
      providesTags: ['GroupedSweep'],
      keepUnusedDataFor: 120,
    }),
    getGroupedSweepGroups: builder.query<
      GroupedSweepGroupRecord[],
      { strategyId: string; runId: string }
    >({
      query: ({ strategyId, runId }) =>
        `/api/strategies/sweeps/${encodeURIComponent(runId)}/groups?strategy_id=${encodeURIComponent(strategyId)}`,
      keepUnusedDataFor: 120,
    }),
    getGroupedSweepResults: builder.query<
      GroupedSweepResultRecord[],
      { strategyId: string; runId: string; groupId: string }
    >({
      query: ({ strategyId, runId, groupId }) =>
        `/api/strategies/sweeps/${encodeURIComponent(runId)}/groups/${encodeURIComponent(groupId)}/results?strategy_id=${encodeURIComponent(strategyId)}`,
      keepUnusedDataFor: 120,
    }),
    getComboTokenResults: builder.query<
      ComboTokenResult[],
      { strategyId: string; runId: string; groupId: string; comboId: number }
    >({
      query: ({ strategyId, runId, groupId, comboId }) =>
        `/api/strategies/sweeps/${encodeURIComponent(runId)}/groups/${encodeURIComponent(groupId)}/token-results?strategy_id=${encodeURIComponent(strategyId)}&combo_id=${comboId}`,
      keepUnusedDataFor: 60,
    }),
    // Trigger a grouped DB-range sweep (single-flight on the backend â€” a 409 means
    // a sweep is already running). Invalidating `GroupedSweep` refetches the runs.
    // Returns AS SOON AS the run is admitted (`202 { run_id }`) rather than holding
    // the request open for the whole sweep â€” that prevented a concurrent cancel POST
    // from queueing behind it on the browser's per-host connection cap. The run then
    // fills in live via per-group writes + SSE; its terminal state (done / cancelled)
    // arrives over the `SweepFinished` SSE frame, which refetches `GroupedSweep`.
    startGroupedSweep: builder.mutation<
      { run_id: string; status: string },
      GroupedSweepStartArgs
    >({
      query: (body) => ({
        url: '/api/strategies/sweeps',
        method: 'POST',
        body,
      }),
      invalidatesTags: ['GroupedSweep'],
    }),
    // Delete one run by id; refetches the (now shorter) runs list.
    deleteGroupedSweepRun: builder.mutation<
      { deleted: number },
      { strategyId: string; runId: string }
    >({
      query: ({ strategyId, runId }) => ({
        url: `/api/strategies/sweeps/${encodeURIComponent(runId)}?strategy_id=${encodeURIComponent(strategyId)}`,
        method: 'DELETE',
      }),
      invalidatesTags: ['GroupedSweep'],
    }),
    // Rename one run (set/clear its label). A blank label clears the name.
    // Refetches the runs list so the picker + history panel show the new name.
    renameGroupedSweepRun: builder.mutation<
      { label: string | null },
      { strategyId: string; runId: string; label: string }
    >({
      query: ({ strategyId, runId, label }) => ({
        url: `/api/strategies/sweeps/${encodeURIComponent(runId)}?strategy_id=${encodeURIComponent(strategyId)}`,
        method: 'PATCH',
        body: { label },
      }),
      invalidatesTags: ['GroupedSweep'],
    }),
    // Prune all runs created strictly before `before` (ISO timestamp). `before`
    // is required server-side so this can't wipe the whole history by accident.
    pruneGroupedSweeps: builder.mutation<
      { deleted: number },
      { strategyId: string; before: string }
    >({
      query: ({ strategyId, before }) => ({
        url: `/api/strategies/sweeps?strategy_id=${encodeURIComponent(strategyId)}&before=${encodeURIComponent(before)}`,
        method: 'DELETE',
      }),
      invalidatesTags: ['GroupedSweep'],
    }),
    // Per-fingerprint creation activity (dashboard "Creation by token group").
    // Server-side partition by a compound fingerprint key + top-N by volume;
    // returns each group's dayÃ—hour fold + calendar trend (count only). Cached
    // 120s; the page floors `from` to the hour so the cache key stays stable.
    getGroupedCreationStats: builder.query<
      GroupedCreationResponse,
      GroupedCreationArgs
    >({
      query: ({ bucket, tz, from, segment, groupBy, top, fieldFilters, ixLabelsFilter }) => {
        const p = new URLSearchParams();
        p.set('bucket', bucket);
        p.set('tz', tz);
        p.set('segment', segment);
        p.set('group_by', groupBy.join(','));
        p.set('top', String(top));
        if (from) p.set('from', from);
        // Only attach filter params when non-empty so the cache key stays stable.
        if (fieldFilters && Object.keys(fieldFilters).length > 0) {
          p.set('field_filters', JSON.stringify(fieldFilters));
        }
        if (ixLabelsFilter && ixLabelsFilter.length > 0) {
          p.set('ix_labels_filter', JSON.stringify(ixLabelsFilter));
        }
        return `/api/tokens/creation-stats/grouped?${p.toString()}`;
      },
      keepUnusedDataFor: 120,
    }),
    // Creator profiles + analysis results â€” paged server-side (limit/offset).
    // On RTK Query (like the Tokens list) so revisiting the Analysis page reuses
    // the cache instead of re-fetching, and structural sharing keeps unchanged
    // rows referentially stable across polls (no whole-table re-render on a tick).
    getCreatorsPage: builder.query<CreatorsResponse, OffsetPageArgs>({
      query: ({ limit, offset }) => `/api/creators?limit=${limit}&offset=${offset}`,
      keepUnusedDataFor: 60,
    }),
    getAnalysisPage: builder.query<AnalysisResponse, OffsetPageArgs>({
      query: ({ limit, offset }) => `/api/analysis?limit=${limit}&offset=${offset}`,
      keepUnusedDataFor: 60,
    }),
  }),
});

export const {
  useGetGroupedCreationStatsQuery,
  useGetCreatorsPageQuery,
  useGetAnalysisPageQuery,
  useGetGroupedSweepRunsQuery,
  useGetGroupedSweepGroupsQuery,
  useGetGroupedSweepResultsQuery,
  useGetComboTokenResultsQuery,
  useStartGroupedSweepMutation,
  useDeleteGroupedSweepRunMutation,
  useRenameGroupedSweepRunMutation,
  usePruneGroupedSweepsMutation,
} = analysisApi;
