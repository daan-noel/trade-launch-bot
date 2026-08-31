import { baseApi, OVERRIDE_ENDPOINTS_ON_HMR } from 'store/baseApi';
import { formatWindowSpec, type WindowSpec } from 'lib/strategy/windowSpec';
import type { FieldFilterValue } from '@lab/components/sweep/fingerprintFilters';
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
import type { IxPatternSet, IxPatternSetDraft } from 'lib/flow/ixPatternSets';
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
import type {
  RuleSearchResult,
  RuleSearchStartArgs,
} from '@lab/lib/ruleSearchTypes';
import type { PartitionSpec } from '@lab/components/sweep/groupedTypes';
import type {
  FamilySearchResult,
  FamilySearchStartArgs,
} from '@lab/lib/familySearchTypes';

/** Body for `POST /api/strategies/flow-discovery`. */
export interface FlowDiscoveryStartArgs {
  created_after?: string;
  created_before?: string;
  curve_only?: boolean;
  group_by: GroupField[];
  /** How each grouped field is partitioned, keyed by field tag. A field not named
   *  here is `{"kind":"distinct"}` (one group per value). Explicit edges, so the
   *  windows a run scored over travel with the request instead of being re-derived
   *  from a width by three surfaces. */
  partition?: Record<string, PartitionSpec>;
  ix_labels_filter?: string[];
  min_tokens?: number;
  token_cap?: number;
  field_filters?: Record<string, FieldFilterValue[]>;
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
  overrideExisting: OVERRIDE_ENDPOINTS_ON_HMR,
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
        windows?: WindowSpec[];
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
        // Whole spans, in the `WindowSpec::parse` grammar the backend reads: `30`
        // stays 30 seconds, `30sl@1` and `20p` mean themselves.
        if (windows && windows.length)
          params.set('windows', windows.map(formatWindowSpec).join(','));
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
      // The flow columns are folded server-side from the fingerprint's saved
      // `ix_patterns`, so a pattern edit changes these numbers. Without the
      // tag the pane keeps serving the pre-edit series while the chart — which
      // re-derives its keys from the same invalidated fingerprint — already moved,
      // and the two disagree for the rest of the cache window.
      providesTags: ['Fingerprint'],
      keepUnusedDataFor: 60,
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
    // Event-log replay inspector . Re-runs the pure
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
        to,
        segment,
        groupBy,
        top,
        partition,
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
        // Omitted `to` = an open window ending at the server's `now` — only a
        // closed (custom / civil-day) window sends an upper bound.
        if (to) p.set('to', to);
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
        // Only attach a non-default partition so the cache key stays stable for the
        // common one-group-per-value case; omitted ⇒ distinct on every field.
        if (partition && Object.keys(partition).length > 0) {
          p.set('partition', JSON.stringify(partition));
        }
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
          to: a.to,
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
                // MUST be the partition that produced `group_key`, or this asks for a
                // window the card never produced and selects nothing.
                ...(a.partition && Object.keys(a.partition).length > 0
                  ? { partition: JSON.stringify(a.partition) }
                  : {}),
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
    // mint a wallet traded in the window, most-recent-trade first (`limit <= 0` ⇒
    // unbounded; positive ⇒ capped). The window is either rolling (`days`) or an
    // explicit `from`/`to` pair of UTC ISO instants — sending `from` makes the
    // backend ignore `days`, so only the active shape goes on the wire. PG-backed
    // on purpose — the 7-day default window includes today, which the sealed-days
    // lake lacks. The page renders these through the shared token columns
    // (client-side sort/filter) + a synced charts grid.
    getTraderTokens: builder.query<
      TraderTokenRow[],
      { wallet: string; days: number; limit: number; from?: string; to?: string; with?: string[] }
    >({
      query: ({ wallet, days, limit, from, to, with: comparison }) => {
        const qs = new URLSearchParams({ limit: String(limit) });
        // An explicit lower bound replaces the rolling window; `to` alone still
        // rides `days` (backend anchors the rolling span to that upper bound).
        if (from) qs.set('from', from);
        else qs.set('days', String(days));
        if (to) qs.set('to', to);
        // Comparison wallets for the co-trade columns. Omitted when empty so the
        // backend skips its second query entirely — the single-wallet page (the
        // common case) costs exactly what it did before.
        if (comparison?.length) qs.set('with', comparison.join(','));
        return `/api/wallets/${encodeURIComponent(wallet)}/tokens?${qs.toString()}`;
      },
      // Pre-parse created_at + the wallet trade timestamps to epoch-ms so the
      // shared AgeCell and the PnL analytics panel (heatmap/equity-curve/scatter
      // bucketing) never re-parse the ISO strings per render.
      transformResponse: (rows: TraderTokenRow[]) =>
        rows.map((r) => ({
          ...r,
          created_at_ms: Date.parse(r.created_at),
          wallet_first_trade_at_ms: Date.parse(r.wallet_first_trade_at),
          wallet_last_trade_at_ms: Date.parse(r.wallet_last_trade_at),
          // Entry/exit are per-SIDE and nullable (exit-only window / still
          // holding) — keep `null` rather than a NaN from parsing undefined, so
          // the age columns can tell "no such leg" from "unparseable".
          wallet_entry_at_ms: r.wallet_entry_at != null ? Date.parse(r.wallet_entry_at) : null,
          wallet_exit_at_ms: r.wallet_exit_at != null ? Date.parse(r.wallet_exit_at) : null,
          // Always an array so every consumer can test `.length` without a guard
          // — the field is absent from a response served before co-trade existed.
          co_traders: r.co_traders ?? [],
        })),
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
        /// The card's own key. It carries the window each axis selected, so bind is
        /// a copy — there is no precision to pass, and so no substituted precision
        /// that could arm the bound rule on a window the card never showed.
        group_key: Record<string, unknown>;
        ix_patterns: (string[] | Record<string, unknown>)[];
        /** Which list to write. Omit / `tagged` → `m_flow_ix`; `dump` → `m_dump_ix`. */
        list?: 'tagged' | 'dump';
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
    // Rule search — registry-role champion for one fingerprint + range. Returns
    // `202 { run_id }` and runs detached; collect via getRuleSearch once the
    // `rule_search_finished` SSE fires.
    startRuleSearch: builder.mutation<
      { run_id: string; status: string },
      RuleSearchStartArgs
    >({
      query: (body) => ({
        url: '/api/strategies/rule-search',
        method: 'POST',
        body,
      }),
    }),
    getRuleSearch: builder.query<RuleSearchResult, string>({
      query: (runId) => `/api/strategies/rule-search/${encodeURIComponent(runId)}`,
    }),
    getLastRuleSearch: builder.query<RuleSearchResult, void>({
      query: () => '/api/strategies/rule-search/last',
    }),
    // Family search — grades one fingerprint's sibling family, fitting the
    // ordering broad and taking the level from the held-out target cohort.
    // Same detached shape as rule search: `202 { run_id }`, then collect via
    // getFamilySearch once `family_search_finished` fires.
    startFamilySearch: builder.mutation<
      { run_id: string; status: string },
      FamilySearchStartArgs
    >({
      query: (body) => ({
        url: '/api/strategies/family-search',
        method: 'POST',
        body,
      }),
    }),
    getFamilySearch: builder.query<FamilySearchResult, string>({
      query: (runId) => `/api/strategies/family-search/${encodeURIComponent(runId)}`,
    }),
    getLastFamilySearch: builder.query<FamilySearchResult, void>({
      query: () => '/api/strategies/family-search/last',
    }),
    // ── Flow lens: analysis-owned ix_labels pattern sets ─────────────────────
    // The study twin of a fingerprint's `ix_patterns` — same classifier,
    // different owner, so a wallet study can split vol/non-vol on tokens that
    // belong to no cohort. Lab-only table; nothing the engine reads.
    getIxPatternSets: builder.query<IxPatternSet[], void>({
      query: () => '/api/ix-pattern-sets',
      providesTags: ['IxPatternSet'],
    }),
    createIxPatternSet: builder.mutation<IxPatternSet, IxPatternSetDraft>({
      query: (body) => ({ url: '/api/ix-pattern-sets', method: 'POST', body }),
      invalidatesTags: ['IxPatternSet'],
    }),
    updateIxPatternSet: builder.mutation<
      IxPatternSet,
      { id: string; body: IxPatternSetDraft }
    >({
      query: ({ id, body }) => ({
        url: `/api/ix-pattern-sets/${encodeURIComponent(id)}`,
        method: 'PUT',
        body,
      }),
      invalidatesTags: ['IxPatternSet'],
    }),
    deleteIxPatternSet: builder.mutation<void, string>({
      query: (id) => ({
        url: `/api/ix-pattern-sets/${encodeURIComponent(id)}`,
        method: 'DELETE',
      }),
      invalidatesTags: ['IxPatternSet'],
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
  useLazyGetFlowDiscoveryQuery,
  useGetLastFlowDiscoveryQuery,
  useBindFlowDiscoveryMutation,
  useStartMetricDiscoveryMutation,
  useLazyGetMetricDiscoveryQuery,
  useGetLastMetricDiscoveryQuery,
  useStartRuleSearchMutation,
  useLazyGetRuleSearchQuery,
  useGetLastRuleSearchQuery,
  useStartFamilySearchMutation,
  useLazyGetFamilySearchQuery,
  useGetLastFamilySearchQuery,
  useGetIxPatternSetsQuery,
  useCreateIxPatternSetMutation,
  useUpdateIxPatternSetMutation,
  useDeleteIxPatternSetMutation,
} = labApi;
