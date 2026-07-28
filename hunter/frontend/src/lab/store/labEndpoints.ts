import { baseApi } from 'store/baseApi';
import { tokensTableRequestBody, type TokensPageArgs } from 'store/sharedEndpoints';
import type {
  GroupedSweepRunRecord,
  GroupedSweepGroupRecord,
  GroupedSweepStartArgs,
  ComboTokenResultsResponse,
} from '@lab/components/sweep/groupedTypes';
import type {
  GroupedCreationArgs,
  GroupedCreationResponse,
  GroupedCreationTokensArgs,
  GroupedCreationTokensResponse,
} from '@lab/components/creation-stats/groupedCreationStats';
import type {
  SimulatedSummary,
  TraderTokenRow,
  FlowDiscoveryResult,
} from 'types';
import type { InspectRequest, InspectRun } from '@lab/services/replayInspect';
import type {
  EngineSimRequest,
  SimStartResponse,
  MetricSeriesResponse,
  PromotedRuleDraft,
  Fingerprint,
} from 'lib/strategy/types';
import type { GroupField } from '@lab/components/sweep/groupedTypes';
import type {
  MetricDiscoveryResult,
  MetricDiscoveryStartArgs,
} from '@lab/lib/metricDiscoveryTypes';

/** Body for `POST /api/strategies/flow-discovery`. */
export interface FlowDiscoveryStartArgs {
  created_after?: string;
  created_before?: string;
  curve_only?: boolean;
  group_by: GroupField[];
  bucket_width_sol?: number;
  ix_labels_filter?: string[];
  min_tokens?: number;
  token_cap?: number;
  field_filters?: Record<string, (number | boolean)[]>;
  /** When set, corpus is filtered by engine fingerprint match (bucket-aware). */
  fingerprint_id?: string;
}

/**
 * Lab-only RTK Query endpoints — bundled exclusively in the lab
 * (local) build: grouped param-sweeps, per-rule strategy simulate/paper reads,
 * grouped creation stats, and the creators/analysis page lists. The live
 * backend serves none of these.
 */
export const labApi = baseApi.injectEndpoints({
  endpoints: (builder) => ({
    // Generic-engine simulate (redesign 5.2). Start a run for a saved rule or an
    // inline dry-run draft; the run is detached and its result stored server-side,
    // collected once the `simulation_finished` SSE fires (same pattern as the
    // legacy per-strategy simulate). The summary aggregates over the whole run.
    startEngineSimulation: builder.mutation<SimStartResponse, EngineSimRequest>({
      query: (body) => ({ url: '/api/strategies/simulate', method: 'POST', body }),
    }),
    getEngineSimSummary: builder.mutation<SimulatedSummary, string>({
      query: (runId) => ({
        url: `/api/strategies/simulate/${encodeURIComponent(runId)}/result/summary`,
        method: 'POST',
        // Aggregate over the full run (summary ignores pagination).
        body: { pagination: { page: 1, pageSize: 1 }, sorting: [], search: '', filters: {} },
      }),
    }),
    /** Unfiltered rollups for many rules in one round-trip (Simulate page hydrate).
     *  Rules with no resident result are omitted from the map. */
    getEngineSimSummaries: builder.mutation<Record<string, SimulatedSummary>, string[]>({
      query: (ruleIds) => ({
        url: '/api/strategies/simulate/summaries',
        method: 'POST',
        body: { rule_ids: ruleIds },
      }),
      transformResponse: (r: { summaries: Record<string, SimulatedSummary> }) => r.summaries ?? {},
    }),
    // On-demand metric series for a token's chart panes (redesign 5.7) — every
    // metric's value at every EVENT (trades + engine `TICK_MS` grid ticks),
    // recomputed from the lake + PG tail with the SAME engine compute the
    // live/sweep paths use (never persisted).
    getMetricSeries: builder.query<
      MetricSeriesResponse,
      {
        mint: string;
        windows?: number[];
        fingerprintId?: string | null;
        /** Inspected run's entry fill — supplies the `m_position` (retrace/bounce/pnl/held)
         *  columns, which are position-scoped and omitted without it. */
        entryTime?: string | null;
        entryPrice?: number | null;
        /** Largest `time` / `stall` condition value the caller will evaluate over the
         *  series (secs). These size the backend's sparse tick grid: the two clocks are
         *  monotone, so it only needs dense ticks up to the last instant they could
         *  cross. Omit when the rule constrains neither — see `metricClockHorizons`. */
        timeHorizonSec?: number | null;
        stallHorizonSec?: number | null;
      }
    >({
      query: ({
        mint,
        windows,
        fingerprintId,
        entryTime,
        entryPrice,
        timeHorizonSec,
        stallHorizonSec,
      }) => {
        const params = new URLSearchParams();
        if (windows && windows.length) params.set('windows', windows.join(','));
        if (fingerprintId) params.set('fingerprint_id', fingerprintId);
        if (entryTime && entryPrice != null && Number.isFinite(entryPrice)) {
          params.set('entry_time', entryTime);
          params.set('entry_price', String(entryPrice));
        }
        if (timeHorizonSec != null && timeHorizonSec > 0) {
          params.set('time_horizon_sec', String(timeHorizonSec));
        }
        if (stallHorizonSec != null && stallHorizonSec > 0) {
          params.set('stall_horizon_sec', String(stallHorizonSec));
        }
        const q = params.toString();
        return `/api/tokens/${encodeURIComponent(mint)}/metric-series${q ? `?${q}` : ''}`;
      },
      keepUnusedDataFor: 60,
    }),
    // Just the matched `mint_address` set for the current Tokens filter — no rows.
    // "Swing Detection All" fans out over the full filtered set, so it reads the
    // mints from here instead of pulling ~20k full token rows via `getTokensPage`
    // only to `.map` them down to mints. Reuses the SAME `tokensTableRequestBody`
    // builder, so the set matches the visible table exactly. One-shot imperative
    // fetch (no cache retention) driven from the page's Run handler.
    getTokenMints: builder.query<string[], TokensPageArgs>({
      query: (a) => ({ url: '/api/tokens/mints', method: 'POST', body: tokensTableRequestBody(a) }),
      transformResponse: (r: { mints: string[] }) => r.mints,
      keepUnusedDataFor: 0,
    }),
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
      // Per-run id tag: the background-jobs registry invalidates it (throttled)
      // on each `sweep_group_done` SSE frame, so a viewed in-progress run's
      // groups table fills in live as groups persist. Also provides the broad
      // 'GroupedSweep' tag so the terminal `SweepFinished` refresh (and
      // delete/prune) refetches the final state.
      providesTags: (_result, _err, { runId }) => [
        { type: 'GroupedSweepGroups' as const, id: runId },
        'GroupedSweep',
      ],
      keepUnusedDataFor: 120,
    }),
    // NOTE: per-group results are read via the NDJSON streaming reader in
    // `useStreamedSweepResults` (the backend `results` route streams
    // application/x-ndjson and needs page/limit/sort) — not an RTK query.
    getComboTokenResults: builder.query<
      ComboTokenResultsResponse,
      { strategyId: string; runId: string; groupId: string; comboId: number }
    >({
      query: ({ strategyId, runId, groupId, comboId }) =>
        `/api/strategies/sweeps/${encodeURIComponent(runId)}/groups/${encodeURIComponent(groupId)}/token-results?strategy_id=${encodeURIComponent(strategyId)}&combo_id=${comboId}`,
      keepUnusedDataFor: 60,
    }),
    // Trigger a grouped DB-range sweep (single-flight on the backend — a 409 means
    // a sweep is already running). Invalidating `GroupedSweep` refetches the runs.
    // Returns AS SOON AS the run is admitted (`202 { run_id }`) rather than holding
    // the request open for the whole sweep — that prevented a concurrent cancel POST
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
    // Promote a swept group's winning combo → find-or-created fingerprint + a
    // pre-filled rule draft the editor opens (sweep redesign 5.6). Omit `comboId`
    // to promote the group's crowned best combo. The fingerprint is find-or-created
    // server-side, so refresh the Fingerprint cache the editor's picker reads from.
    promoteSweepGroup: builder.mutation<
      PromotedRuleDraft,
      { runId: string; groupId: string; comboId?: number }
    >({
      query: ({ runId, groupId, comboId }) => {
        const combo = comboId != null ? `&combo_id=${comboId}` : '';
        return {
          url: `/api/strategies/sweeps/${encodeURIComponent(runId)}/groups/${encodeURIComponent(groupId)}/promote?strategy_id=generic${combo}`,
          method: 'POST',
        };
      },
      invalidatesTags: ['Fingerprint'],
    }),
    // Event-log replay inspector (redesign FE6 / backend Phase 6). Re-runs the pure
    // engine over a recorded live log and dumps every event→effects decision. A
    // one-shot POST (no cache retention — each run is a fresh inspection).
    inspectReplay: builder.mutation<InspectRun, InspectRequest>({
      query: (body) => ({ url: '/api/replay/inspect', method: 'POST', body }),
    }),
    // Per-fingerprint creation activity (dashboard "Creation by token group").
    // Server-side partition by a compound fingerprint key + top-N by volume;
    // returns each group's day×hour fold + calendar trend (count only). Cached
    // 120s; the page floors `from` to the hour so the cache key stays stable.
    getGroupedCreationStats: builder.query<
      GroupedCreationResponse,
      GroupedCreationArgs
    >({
      query: ({
        bucket,
        tz,
        from,
        segment,
        groupBy,
        top,
        bucketWidth,
        fieldFilters,
        ixLabelsFilter,
        rankBy,
        fingerprintId,
      }) => {
        const p = new URLSearchParams();
        p.set('bucket', bucket);
        p.set('tz', tz);
        p.set('segment', segment);
        if (from) p.set('from', from);
        // Scoped by a saved fingerprint ⇒ the backend ignores group_by/top/
        // field_filters/ix_labels_filter/bucket_width/rank_by entirely (same
        // contract as the sweep's/flow discovery's fingerprint_id) — don't
        // send them.
        if (fingerprintId) {
          p.set('fingerprint_id', fingerprintId);
          return `/api/tokens/creation-stats/grouped?${p.toString()}`;
        }
        p.set('group_by', groupBy.join(','));
        p.set('top', String(top));
        // Only attach a non-default width so the cache key stays stable for the
        // common 0.1 case; omitted ⇒ backend default.
        if (bucketWidth != null) p.set('bucket_width', String(bucketWidth));
        // Only attach filter params when non-empty so the cache key stays stable.
        if (fieldFilters && Object.keys(fieldFilters).length > 0) {
          p.set('field_filters', JSON.stringify(fieldFilters));
        }
        if (ixLabelsFilter && ixLabelsFilter.length > 0) {
          p.set('ix_labels_filter', JSON.stringify(ixLabelsFilter));
        }
        // Only attach a non-default rank so the cache key stays stable for the
        // common `count` case; omitted ⇒ backend default.
        if (rankBy && rankBy !== 'count') p.set('rank_by', rankBy);
        return `/api/tokens/creation-stats/grouped?${p.toString()}`;
      },
      keepUnusedDataFor: 120,
    }),
    // Drill-down tokens table behind one group card (or one of its heatmap
    // tiles): the exact rows `getGroupedCreationStats` folded into that
    // `group_key` (optionally narrowed to a recurring dow/hour slot), paged
    // through the SAME SQL projection as the Tokens page so `TokenTable`
    // renders identically. See `creation_stats.rs::get_grouped_creation_tokens`.
    getGroupedCreationTokens: builder.query<
      GroupedCreationTokensResponse,
      GroupedCreationTokensArgs
    >({
      query: (a) => ({
        url: '/api/tokens/creation-stats/grouped/tokens',
        method: 'POST',
        body: {
          pagination: { page: a.page, pageSize: a.pageSize },
          sorting: a.sortKeys.map((s) => ({ col: s.col, dir: s.dir })),
          search: a.search,
          // Per-column drill-in filters (numeric/amount/flag/identity grammar),
          // already lowered by `drillTokenFilters`; the backend layers them onto
          // the group's corpus scope. Empty ⇒ no per-column filter.
          filters: a.filters ?? {},
          tz: a.tz,
          from: a.from,
          segment: a.segment,
          // `group_key` is a required (non-defaulted) field on the backend
          // request struct, so it must always be present or serde 400s
          // ("missing field group_key") before the handler runs — even in the
          // fingerprint-scope branch where the handler then ignores it. Send the
          // caller's key (`{}` under fingerprint scope).
          group_key: a.groupKey,
          // Scoped by a saved fingerprint ⇒ group_by/field_filters/
          // ix_labels_filter/group_key are all ignored server-side; don't send
          // the rest (same contract as `getGroupedCreationStats`).
          ...(a.fingerprintId
            ? { fingerprint_id: a.fingerprintId }
            : {
                group_by: a.groupBy.join(','),
                ...(a.bucketWidth != null ? { bucket_width: a.bucketWidth } : {}),
                ...(a.fieldFilters && Object.keys(a.fieldFilters).length > 0
                  ? { field_filters: JSON.stringify(a.fieldFilters) }
                  : {}),
                ...(a.ixLabelsFilter && a.ixLabelsFilter.length > 0
                  ? { ix_labels_filter: JSON.stringify(a.ixLabelsFilter) }
                  : {}),
              }),
          ...(a.dow != null && a.hour != null ? { dow: a.dow, hour: a.hour } : {}),
        },
      }),
      transformResponse: (r: GroupedCreationTokensResponse) => ({
        total: r.total,
        items: r.items.map((t) => ({ ...t, created_at_ms: Date.parse(t.created_at) })),
      }),
      keepUnusedDataFor: 30,
    }),
    // Trader Analysis: full token rows (+ the wallet's per-mint stats) for every
    // mint a wallet traded in the last `days`, most-recent-trade first, capped at
    // `limit`. PG-backed on purpose — the 7-day default window includes today,
    // which the sealed-days lake lacks. The page renders these through the shared
    // token columns (client-side sort/filter) + a synced charts grid.
    getTraderTokens: builder.query<
      TraderTokenRow[],
      { wallet: string; days: number; limit: number }
    >({
      query: ({ wallet, days, limit }) =>
        `/api/wallets/${encodeURIComponent(wallet)}/tokens?days=${days}&limit=${limit}`,
      // Pre-parse created_at to epoch-ms so the shared AgeCell reads it directly
      // (mirrors the getTokensPage transform).
      transformResponse: (rows: TraderTokenRow[]) =>
        rows.map((r) => ({ ...r, created_at_ms: Date.parse(r.created_at) })),
      keepUnusedDataFor: 60,
    }),
    // Flow discovery — score trade ix-structures per fingerprint group (V4).
    // Returns immediately with `202 { run_id }`; collect via getFlowDiscovery
    // after `flow_discovery_finished` SSE.
    startFlowDiscovery: builder.mutation<
      { run_id: string; status: string },
      FlowDiscoveryStartArgs
    >({
      query: (body) => ({
        url: '/api/strategies/flow-discovery',
        method: 'POST',
        body,
      }),
    }),
    getFlowDiscovery: builder.query<FlowDiscoveryResult, string>({
      query: (runId) => `/api/strategies/flow-discovery/${encodeURIComponent(runId)}`,
    }),
    // Disk-cached last result, keyed by nothing — rehydrates the page after a
    // reload when there's no run_id in hand yet. 404 ⇒ no cache on disk.
    getLastFlowDiscovery: builder.query<FlowDiscoveryResult, void>({
      query: () => '/api/strategies/flow-discovery/last',
    }),
    // Promote-style bind: find-or-create fingerprint from group_key + patch patterns.
    bindFlowDiscovery: builder.mutation<
      Fingerprint,
      {
        group_key: Record<string, string>;
        bucket_width_sol: number;
        volume_ix_patterns: string[][];
        name?: string;
      }
    >({
      query: (body) => ({
        url: '/api/strategies/flow-discovery/bind',
        method: 'POST',
        body,
      }),
      invalidatesTags: ['Fingerprint'],
    }),
    // Metric-combo discovery pipeline (screen → family → validate). Returns
    // `202 { run_id }` and runs detached; collect via getMetricDiscovery once the
    // `metric_discovery_finished` SSE fires (same shape as flow discovery).
    startMetricDiscovery: builder.mutation<
      { run_id: string; status: string },
      MetricDiscoveryStartArgs
    >({
      query: (body) => ({
        url: '/api/strategies/metric-discovery',
        method: 'POST',
        body,
      }),
    }),
    getMetricDiscovery: builder.query<MetricDiscoveryResult, string>({
      query: (runId) => `/api/strategies/metric-discovery/${encodeURIComponent(runId)}`,
    }),
    // Last cached pipeline result — rehydrates the page after a reload. 404 ⇒ none.
    getLastMetricDiscovery: builder.query<MetricDiscoveryResult, void>({
      query: () => '/api/strategies/metric-discovery/last',
    }),
  }),
});

export const {
  useGetGroupedCreationStatsQuery,
  useGetGroupedCreationTokensQuery,
  useGetGroupedSweepRunsQuery,
  useGetGroupedSweepGroupsQuery,
  useGetComboTokenResultsQuery,
  useStartGroupedSweepMutation,
  useDeleteGroupedSweepRunMutation,
  useRenameGroupedSweepRunMutation,
  usePruneGroupedSweepsMutation,
  useGetTraderTokensQuery,
  usePromoteSweepGroupMutation,
  useInspectReplayMutation,
  useStartEngineSimulationMutation,
  useGetEngineSimSummaryMutation,
  useGetEngineSimSummariesMutation,
  useGetMetricSeriesQuery,
  useStartFlowDiscoveryMutation,
  useGetFlowDiscoveryQuery,
  useLazyGetFlowDiscoveryQuery,
  useGetLastFlowDiscoveryQuery,
  useBindFlowDiscoveryMutation,
  useStartMetricDiscoveryMutation,
  useLazyGetMetricDiscoveryQuery,
  useGetLastMetricDiscoveryQuery,
} = labApi;
