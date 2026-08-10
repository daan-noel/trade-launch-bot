import { baseApi } from './baseApi';
import type { AppSettings } from 'services/api';
import type { SortEntry } from 'components/table/types';
import type { FilterSpec } from 'components/table/numericFilter';
import { toTableRequest } from 'services/tableRequest';
import { TOKEN_INFO_AMOUNT_COLS } from 'components/tokens/sharedTokenColumns';
import type {
  CreationStatsArgs,
  CreationStatsResponse,
} from 'components/creation-stats/creationStats';
import type {
  RulePositionRecord,
  TokenDetailRecord,
  TokenRecord,
  TradeRecord,
  WalletProfile,
} from 'types';
import type { StrategyRegistry } from 'lib/strategy/registry';
import type {
  Fingerprint,
  FingerprintDraft,
  StrategyRule,
  CreateRuleBody,
  UpdateRuleBody,
  TradeMode,
} from 'lib/strategy/types';

/**
 * How the Rules scoreboard scores each rule. A bare string is the legacy scope-only
 * form; the object form adds `mode`, which pins every rule to ONE trade mode's ledger
 * instead of its own `trade_mode`. Omitting `mode` is not the same as picking one —
 * it is the per-rule "own mode" board.
 */
export type RuleScoreArg =
  | 'current'
  | 'all'
  | { scope?: 'current' | 'all'; mode?: TradeMode }
  | void;

/**
 * Args for the server-side paginated Tokens view: the backend filters/sorts/pages so
 * only one page crosses the wire. Mirrors the DataTable view-state; page-owned
 * quick filters (Created / Dead / Migrated) and mint-set arrive already folded
 * into `structuredFilters` by the caller.
 */
export interface TokensPageArgs {
  page: number; // 1-based
  pageSize: number;
  sortKeys: SortEntry[];
  search: string;
  colFilters: Record<string, string>;
  /** Wrapper-/page-injected structured filters (mint-set `in`, Tokens quick
   *  bar, etc.); merged into the request `filters` map (structured wins on
   *  key collision with per-column text). */
  structuredFilters?: Record<string, FilterSpec>;
  /** When true, restrict results to the live cache-tracked subset only. */
  trackedOnly?: boolean;
}

/**
 * Page size used by the full-list consumers (Tokens + Swing detection) that
 * pull the whole token set client-side for filtering/analysis. Kept as one
 * shared constant so both pages request identical args and share the RTK cache
 * entry, and so the value stays at or below the backend's list ceiling (50k) —
 * a mismatch there would silently truncate the list again.
 */
export const TOKENS_LIST_LIMIT = 20_000;

/**
 * Serialize `TokensPageArgs` into the unified `TableRequest` POST body — the ONE
 * place DataTable view-state (`toTableRequest`) becomes a wire body, plus the
 * Tokens-only `trackedOnly` rider. Kept as a named function (not inlined into
 * `getTokensPage`) so any future filtered-token read builds a byte-identical
 * body and can't drift from the visible table.
 */
export function tokensTableRequestBody(a: TokensPageArgs): ReturnType<typeof toTableRequest> {
  // Numeric columns needn't be enumerated for ordinary ops: the backend re-parses
  // each raw per-column predicate string (`lower_filter`). We still pass
  // `amountCols` so PriceUnit-converted amount filters rewrite display→storage
  // before that re-parse sees the operand.
  const body = toTableRequest(
    {
      page: a.page,
      pageSize: a.pageSize,
      sortKeys: a.sortKeys,
      search: a.search,
      colFilters: a.colFilters,
      structuredFilters: a.structuredFilters,
    },
    new Set(),
    { amountCols: TOKEN_INFO_AMOUNT_COLS },
  );
  if (a.trackedOnly) body.trackedOnly = true;
  return body;
}

export interface TokensResponse {
  total: number;
  /** Filtered count restricted to the live, cache-tracked subset (≤ `total`).
   *  Defensively defaulted to `total` if a response ever omits it. */
  tracked: number;
  items: TokenRecord[];
}

/**
 * Parse each row's `created_at` to epoch-ms exactly once, here, instead of in
 * every age cell on every render/tick. RTK's structural sharing runs *after*
 * this, so the derived field is deterministic and unchanged rows keep their
 * object identity across polls (the row memoization still holds).
 */
function withCreatedMs(r: TokensResponse): TokensResponse {
  return {
    total: r.total,
    // `getTokens` (the simple list) omits `tracked`; fall back to `total` so the
    // field is always a number for consumers.
    tracked: r.tracked ?? r.total,
    items: r.items.map((t) => ({ ...t, created_at_ms: Date.parse(t.created_at) })),
  };
}

/**
 * Shared RTK Query endpoints — read by both the live and lab builds:
 * tokens (list / paged / batch / detail / trades), profiles, settings, SOL
 * price, and the creation-stats heatmap. Injected onto `baseApi`
 * so each build's store registers them for side-effect.
 */
export const sharedApi = baseApi.injectEndpoints({
  endpoints: (builder) => ({
    // Server-side paginated/filtered/sorted token list over the **unified**
    // `POST /api/tokens` [`TableRequest`] body — the SAME contract the strategy
    // tables use. DataTable view-state + page/wrapper `structuredFilters`
    // (quick bar, mint-set) fold into ONE `filters` map; the Tokens-only
    // `trackedOnly` rides alongside. Backend execution differs by build (same
    // wire contract): the LIVE bin pages this straight from Postgres over the
    // whole token universe (no cap) — its in-RAM cache holds only tracking
    // tokens; the LAB bin runs it over a full in-RAM snapshot. A short retention
    // keeps abandoned filter/sort/page permutations from accumulating.
    getTokensPage: builder.query<TokensResponse, TokensPageArgs>({
      query: (a) => ({ url: '/api/tokens', method: 'POST', body: tokensTableRequestBody(a) }),
      transformResponse: withCreatedMs,
      keepUnusedDataFor: 30,
    }),
    // Token-creation-time bias aggregate (dashboard). Server-side GROUP BY over
    // tokens ⋈ tokens_info; all three color metrics ship together so the metric
    // toggle is a pure client re-color. Cached 120s; the page floors `from` to the
    // hour so the cache key stays stable across renders within the hour.
    getCreationStats: builder.query<CreationStatsResponse, CreationStatsArgs>({
      query: ({ view, bucket, tz, from, segment }) => {
        const p = new URLSearchParams();
        p.set('view', view);
        p.set('bucket', bucket);
        p.set('tz', tz);
        p.set('segment', segment);
        if (from) p.set('from', from);
        return `/api/tokens/creation-stats?${p.toString()}`;
      },
      keepUnusedDataFor: 120,
    }),
    getTokenDetail: builder.query<TokenDetailRecord, string>({
      query: (mint) => `/api/tokens/${encodeURIComponent(mint)}`,
    }),
    getTokenTrades: builder.query<TradeRecord[], string>({
      // Full history (no `limit` ⇒ backend returns every trade): the inspect charts
      // resolve their entry/exit markers and swing legs against this trade set, so a
      // first-N cap left the tail of a high-volume token off the chart and mis-snapped
      // the exit / later swing legs. This is a cold, deliberately-opened path.
      query: (mint) => `/api/tokens/${encodeURIComponent(mint)}/trades`,
    }),
    // Wallet profiles — read by the chart-marker consumers (Swing detection,
    // Sync token) to overlay tracked wallets. Folded into the cache so those two
    // pages dedupe a shared fetch and reuse it across navigation instead of each
    // re-fetching on mount. The OtherProfiles editor owns the authoritative CRUD
    // state imperatively; a bounded retention keeps these cosmetic markers from
    // lagging an edit for long.
    getProfiles: builder.query<WalletProfile[], void>({
      query: () => '/api/profiles',
      providesTags: ['Profiles'],
      keepUnusedDataFor: 120,
    }),
    // System reads shared app-wide (header + price toggle). Folding them into
    // RTK Query collapses the StrictMode double-fire and the multiple
    // independent callers into a single deduped request per cache key.
    getSolPrice: builder.query<number | null, void>({
      query: () => '/api/system/price',
      transformResponse: (r: { usd_rate: number | null }) => r.usd_rate,
    }),
    // Global app settings — one shared object read by both root contexts
    // (timezone / price-unit), the Settings page, and the Sync page. A single
    // cache entry, in place of 4+ independent fetches on startup.
    getSettings: builder.query<AppSettings, void>({
      query: () => '/api/system/settings',
      providesTags: ['Settings'],
    }),
    // The strategy metric registry (`hunter_engine::metrics::registry_json`) — the
    // self-describing vocabulary the whole rule-authoring UI renders from. Static
    // for the backend process lifetime, so cache it for an hour and never refetch
    // on remount: one request per session drives every group picker / metric row /
    // sweep axis / chart pane / validation message.
    getStrategyRegistry: builder.query<StrategyRegistry, void>({
      query: () => '/api/meta/strategy-registry',
      keepUnusedDataFor: 3600,
    }),

    // ── Fingerprints (generic engine, live bin) ──────────────────────────────
    // Shared match specs many rules reference. Amounts are lamports on the wire;
    // the form components convert to/from SOL. The list carries a folded-in
    // `used_by` rule count for the library + delete guard.
    getFingerprints: builder.query<Fingerprint[], void>({
      query: () => '/api/fingerprints',
      providesTags: ['Fingerprint'],
    }),
    createFingerprint: builder.mutation<Fingerprint, FingerprintDraft>({
      query: (body) => ({ url: '/api/fingerprints', method: 'POST', body }),
      invalidatesTags: ['Fingerprint'],
    }),
    updateFingerprint: builder.mutation<Fingerprint, { id: string; body: FingerprintDraft }>({
      query: ({ id, body }) => ({ url: `/api/fingerprints/${id}`, method: 'PUT', body }),
      // A fingerprint edit can change which tokens rules match — refresh rules too.
      invalidatesTags: ['Fingerprint', 'StrategyRule'],
    }),
    deleteFingerprint: builder.mutation<void, string>({
      query: (id) => ({ url: `/api/fingerprints/${id}`, method: 'DELETE' }),
      invalidatesTags: ['Fingerprint'],
    }),

    // ── Strategy rules (generic engine, live bin) ────────────────────────────
    // `scope`: `current` = latest-run counters (Control keep/kill);
    // omit/`all` = legacy (real all-time, paper latest run).
    // `mode`: score EVERY rule in that one trade mode instead of its own — the only
    // way to read a rule's paper record while it trades real. Omit for the rule's own
    // mode. With a mode pinned, `all` is all-time on both sides (no paper/real span
    // asymmetry). The bare-string arg is the legacy scope-only form, kept because
    // most callers want the plain list.
    getStrategyRules: builder.query<StrategyRule[], RuleScoreArg>({
      query: (arg) => {
        const { scope, mode } = typeof arg === 'string' ? { scope: arg, mode: undefined } : arg ?? {};
        const q = new URLSearchParams();
        if (scope === 'current') q.set('score_scope', 'current');
        if (mode) q.set('score_mode', mode);
        const qs = q.toString();
        return qs ? `/api/strategy-rules?${qs}` : '/api/strategy-rules';
      },
      providesTags: ['StrategyRule'],
    }),
    /**
     * Every **entered** episode on one mint, oldest first, with `exit_legs` — the
     * token chart's re-entry overlay, so a modal opened on one position still draws
     * the mint's whole trade history (`Entry 1..N` / every leg of every exit).
     *
     * Shared because BOTH bins serve this URL: live off its own `strategy_positions`,
     * lab off the synced mirror. Mode-scoped — paper fills are modeled and real ones
     * are money, so they never share a chart.
     */
    getMintEpisodes: builder.query<
      RulePositionRecord[],
      { mint: string; mode?: string | null }
    >({
      query: ({ mint, mode }) =>
        `/api/strategies/generic/positions/mint/${encodeURIComponent(mint)}/episodes?mode=${
          mode === 'paper' ? 'paper' : 'real'
        }`,
      providesTags: (_result, _error, { mint }) => [{ type: 'MintEpisodes' as const, id: mint }],
    }),
    /** Run navigator — live-only route; Evidence pane is the sole consumer. */
    getStrategyRuleRuns: builder.query<
      import('lib/strategy/types').StrategyRuleRun[],
      string
    >({
      query: (id) => `/api/strategy-rules/${encodeURIComponent(id)}/runs`,
      providesTags: ['StrategyRule'],
    }),
    createStrategyRule: builder.mutation<StrategyRule, CreateRuleBody>({
      query: (body) => ({ url: '/api/strategy-rules', method: 'POST', body }),
      // A new rule bumps its fingerprint's used-by count.
      invalidatesTags: ['StrategyRule', 'Fingerprint'],
    }),
    updateStrategyRule: builder.mutation<StrategyRule, { id: string; body: UpdateRuleBody }>({
      query: ({ id, body }) => ({ url: `/api/strategy-rules/${id}`, method: 'PUT', body }),
      invalidatesTags: ['StrategyRule'],
    }),
    deleteStrategyRule: builder.mutation<void, string>({
      query: (id) => ({ url: `/api/strategy-rules/${id}`, method: 'DELETE' }),
      invalidatesTags: ['StrategyRule', 'Fingerprint'],
    }),
    activateStrategyRule: builder.mutation<StrategyRule, string>({
      query: (id) => ({ url: `/api/strategy-rules/${id}/activate`, method: 'POST' }),
      invalidatesTags: ['StrategyRule'],
    }),
    // Soft-unarchive — orthogonal to Active/Idle.
    enableStrategyRule: builder.mutation<StrategyRule, string>({
      query: (id) => ({ url: `/api/strategy-rules/${id}/enable`, method: 'POST' }),
      async onQueryStarted(id, { dispatch, queryFulfilled }) {
        const undo = patchAllRuleLists(dispatch, (draft) => {
          const row = draft.find((r) => r.id === id);
          if (row) row.is_enabled = true;
        });
        try {
          await queryFulfilled;
        } catch {
          undo.undo();
        }
      },
      invalidatesTags: ['StrategyRule'],
    }),
    // Soft-archive (+ pause if Active). Optimistic Disabled + Idle patch.
    disableStrategyRule: builder.mutation<StrategyRule, string>({
      query: (id) => ({ url: `/api/strategy-rules/${id}/disable`, method: 'POST' }),
      async onQueryStarted(id, { dispatch, queryFulfilled }) {
        const undo = patchAllRuleLists(dispatch, (draft) => {
          const row = draft.find((r) => r.id === id);
          if (row) {
            row.is_enabled = false;
            row.is_active = false;
          }
        });
        try {
          await queryFulfilled;
        } catch {
          undo.undo();
        }
      },
      invalidatesTags: ['StrategyRule'],
    }),
    // Instant flag flip — server ack + refetch; no optimistic is_active patch.
    pauseStrategyRule: builder.mutation<StrategyRule, string>({
      query: (id) => ({ url: `/api/strategy-rules/${id}/pause`, method: 'POST' }),
      async onQueryStarted(_id, { dispatch, queryFulfilled }) {
        try {
          await queryFulfilled;
          refetchAllRuleLists(dispatch);
        } catch {
          /* RulesView surfaces the error */
        }
      },
      invalidatesTags: ['StrategyRule'],
    }),
    // Stop = deactivate AND force-close open positions. Returns 202 + action_id;
    // position closes stream over `action_progress` / `strategy_position_update`.
    stopStrategyRule: builder.mutation<
      { action_id: string; total: number; closing: boolean; rule_id: string },
      string
    >({
      query: (id) => ({ url: `/api/strategy-rules/${id}/stop`, method: 'POST' }),
      async onQueryStarted(_id, { dispatch, queryFulfilled }) {
        try {
          await queryFulfilled;
          refetchAllRuleLists(dispatch);
        } catch {
          /* RulesView surfaces the error */
        }
      },
      invalidatesTags: ['StrategyRule'],
    }),
    // Bulk lifecycle scoped to one trade mode — mirror the per-row Pause / Stop.
    pauseAllStrategyRules: builder.mutation<{ paused: number }, TradeMode>({
      query: (mode) => ({ url: `/api/strategy-rules/pause-all?mode=${mode}`, method: 'POST' }),
      async onQueryStarted(_mode, { dispatch, queryFulfilled }) {
        try {
          await queryFulfilled;
          refetchAllRuleLists(dispatch);
        } catch {
          /* RulesView surfaces the error */
        }
      },
      invalidatesTags: ['StrategyRule'],
    }),
    stopAllStrategyRules: builder.mutation<
      { action_id: string; total: number; paused: number; closing: boolean; mode: string },
      TradeMode
    >({
      query: (mode) => ({ url: `/api/strategy-rules/stop-all?mode=${mode}`, method: 'POST' }),
      async onQueryStarted(_mode, { dispatch, queryFulfilled }) {
        try {
          await queryFulfilled;
          refetchAllRuleLists(dispatch);
        } catch {
          /* RulesView surfaces the error */
        }
      },
      invalidatesTags: ['StrategyRule'],
    }),
    updateSettings: builder.mutation<AppSettings, Partial<AppSettings>>({
      query: (patch) => ({
        url: '/api/system/settings',
        method: 'PUT',
        body: patch,
      }),
      // Optimistically patch the shared cache so every consumer (toggles,
      // contexts) reflects the change instantly; roll back if the PUT fails.
      async onQueryStarted(patch, { dispatch, queryFulfilled }) {
        const undo = dispatch(
          sharedApi.util.updateQueryData('getSettings', undefined, (draft) => {
            Object.assign(draft, patch);
          }),
        );
        try {
          const { data } = await queryFulfilled;
          dispatch(
            sharedApi.util.updateQueryData('getSettings', undefined, (draft) => {
              Object.assign(draft, data);
            }),
          );
        } catch {
          undo.undo();
        }
      },
    }),
  }),
});

/** Optimistic lifecycle patches must update every `getStrategyRules` cache key. */
const RULE_LIST_ARGS: Array<'current' | 'all' | undefined> = [undefined, 'current', 'all'];

/** Refetch every cached strategy-rules list (all score_scope keys). */
export function refetchAllRuleLists(
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  dispatch: (action: any) => unknown,
): void {
  for (const arg of RULE_LIST_ARGS) {
    dispatch(sharedApi.endpoints.getStrategyRules.initiate(arg, { forceRefetch: true }));
  }
}

function patchAllRuleLists(
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  dispatch: (action: any) => { undo: () => void },
  recipe: (draft: StrategyRule[]) => void,
): { undo: () => void } {
  const undos = RULE_LIST_ARGS.map((arg) =>
    dispatch(sharedApi.util.updateQueryData('getStrategyRules', arg, recipe)),
  );
  return {
    undo: () => {
      for (const u of undos) u.undo();
    },
  };
}

export const {
  useGetTokensPageQuery,
  useGetCreationStatsQuery,
  useGetTokenDetailQuery,
  useGetTokenTradesQuery,
  useGetSolPriceQuery,
  useLazyGetSolPriceQuery,
  useGetSettingsQuery,
  useGetProfilesQuery,
  useUpdateSettingsMutation,
  useGetStrategyRegistryQuery,
  useGetFingerprintsQuery,
  useCreateFingerprintMutation,
  useUpdateFingerprintMutation,
  useDeleteFingerprintMutation,
  useGetStrategyRulesQuery,
  useGetMintEpisodesQuery,
  useGetStrategyRuleRunsQuery,
  useCreateStrategyRuleMutation,
  useUpdateStrategyRuleMutation,
  useDeleteStrategyRuleMutation,
  useActivateStrategyRuleMutation,
  useEnableStrategyRuleMutation,
  useDisableStrategyRuleMutation,
  usePauseStrategyRuleMutation,
  useStopStrategyRuleMutation,
  usePauseAllStrategyRulesMutation,
  useStopAllStrategyRulesMutation,
} = sharedApi;
