import { createApi, fetchBaseQuery } from '@reduxjs/toolkit/query/react';
import type { FetchBaseQueryError } from '@reduxjs/toolkit/query';
import type { SerializedError } from '@reduxjs/toolkit';
import { API_BASE } from 'services/config';
import type { AppSettings } from 'services/api';
import type { TokenFilters } from 'components/tokens/filters';
import type { SortDir } from 'components/table/types';
import type {
  GroupedSweepRunRecord,
  GroupedSweepGroupRecord,
  GroupedSweepResultRecord,
  GroupedSweepStartArgs,
} from 'components/sweep/groupedTypes';

import type {
  AnalysisRecord,
  CreatorRecord,
  MatchedTokenRecord,
  PaperResultResponse,
  SimulatedTokenResult,
  TokenDetailRecord,
  TokenRecord,
  TradeRecord,
  WalletHolding,
  WalletPrice,
  WalletProfile,
  CashbackStatus,
  CashbackClaimResult,
} from 'types';

/** Args for the per-rule strategy result reads (matched / simulate / paper),
 *  shared by both strategy pages so tpsl1 and tpsl2 keep distinct cache keys. */
export interface StrategyRuleArg {
  strategy: 'tpsl1' | 'tpsl2';
  ruleId: string;
}

/** Cache tag for a rule's `matched` + `simulate` results — both derive from the
 *  rule's entry criteria, so editing the rule invalidates the pair. */
const strategyResultTag = (a: StrategyRuleArg) =>
  ({ type: 'StrategyResult', id: `${a.strategy}:${a.ruleId}` }) as const;

export interface TokensArgs {
  search: string;
  limit: number;
  offset: number;
}

/**
 * Args for the server-side paginated Tokens view. Unlike `TokensArgs` (which
 * pulls the whole list for client-side analysis on the Swing page), this asks
 * the backend to filter/sort/page so only one page crosses the wire. Mirrors
 * the DataTable view-state plus the global `TokenFilters` panel.
 */
export interface TokensPageArgs {
  page: number; // 1-based
  pageSize: number;
  sortCol: string | null;
  sortDir: SortDir;
  search: string;
  colFilters: Record<string, string>;
  filters: TokenFilters;
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
 * Upper bound on the per-token trade history pulled for the price chart. The
 * backend `get_trades` handler already caps the response at 5000; requesting it
 * explicitly makes the guardrail visible at the call site (rather than relying
 * on a silent server default) and keeps the chart's working set bounded.
 */
export const TOKEN_TRADES_LIMIT = 5_000;

export interface TokensResponse {
  total: number;
  items: TokenRecord[];
}

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
 * Parse each row's `created_at` to epoch-ms exactly once, here, instead of in
 * every age cell on every render/tick. RTK's structural sharing runs *after*
 * this, so the derived field is deterministic and unchanged rows keep their
 * object identity across polls (the row memoization still holds).
 */
function withCreatedMs(r: TokensResponse): TokensResponse {
  return {
    total: r.total,
    items: r.items.map((t) => ({ ...t, created_at_ms: Date.parse(t.created_at) })),
  };
}

export interface BuyTokenArgs {
  mint: string;
  sol_amount: number;
  /// Omitted for manual buys — the backend resolves the token program on-chain.
  token_program_id?: string;
  /// Per-trade slippage in basis points; omit to use the global default.
  slippage_bps?: number;
}

export interface SellTokenArgs {
  mint: string;
  /// Optional token-account hint (row "Sell All" supplies it to skip a wallet
  /// scan; a manual sell by mint omits it). The backend always sells the full
  /// live balance, so no amount is sent.
  token_account?: string;
  slippage_bps?: number;
}

/**
 * Central RTK Query cache for expensive / shared server data.
 *
 * `keepUnusedDataFor` retains a cache entry for 5 minutes after the last
 * component unsubscribes, and `refetchOnMountOrArgChange: false` means
 * navigating back to a page reuses that cache instead of re-fetching. The big
 * token list (see `TOKENS_LIST_LIMIT`) is therefore fetched once and shared by
 * every page that subscribes with the same args (Tokens + Swing detection).
 */
export const apiSlice = createApi({
  reducerPath: 'api',
  baseQuery: fetchBaseQuery({ baseUrl: API_BASE }),
  keepUnusedDataFor: 300,
  refetchOnMountOrArgChange: false,
  tagTypes: ['Settings', 'LiveMode', 'WalletHoldings', 'StrategyResult', 'StrategyPaper', 'Profiles', 'Cashback', 'GroupedSweep'],
  endpoints: (builder) => ({
    getTokens: builder.query<TokensResponse, TokensArgs>({
      query: ({ search, limit, offset }) => {
        const params = new URLSearchParams({
          limit: String(limit),
          offset: String(offset),
        });
        if (search) params.set('search', search);
        return `/api/tokens?${params.toString()}`;
      },
      transformResponse: withCreatedMs,
    }),
    // Server-side paginated/filtered/sorted token list. The backend applies the
    // full TokenFilters set + search + per-column filters + sort over its
    // in-memory cache, returning one page plus the filtered `total`. Distinct
    // from `getTokens` (which Swing uses to pull everything) so the two no
    // longer share a cache entry. A short retention keeps abandoned
    // filter/sort/page permutations from accumulating.
    getTokensPage: builder.query<TokensResponse, TokensPageArgs>({
      query: (a) => {
        const p = new URLSearchParams();
        p.set('limit', String(a.pageSize));
        p.set('offset', String((a.page - 1) * a.pageSize));
        if (a.search) p.set('search', a.search);
        if (a.sortCol) {
          p.set('sort_col', a.sortCol);
          p.set('sort_dir', a.sortDir);
        }
        const cf = Object.entries(a.colFilters)
          .filter(([, v]) => v.trim())
          .map(([k, v]) => `${k}:${v}`)
          .join(';');
        if (cf) p.set('cf', cf);
        for (const [k, v] of Object.entries(a.filters)) {
          if (v) p.set(`f_${k}`, String(v));
        }
        return `/api/tokens?${p.toString()}`;
      },
      transformResponse: withCreatedMs,
      keepUnusedDataFor: 30,
    }),
    // Per-rule strategy result reads. Driven imperatively from the strategy
    // pages via `endpoints.X.initiate` (the pages keep their own open/toggle
    // state), so folding them into RTK Query buys dedupe + structural sharing +
    // a short retention: re-opening a view or switching rules and back reuses
    // the cache instead of re-hitting the backend. `matched`/`simulate` are
    // tagged by rule so a rule edit can invalidate them; `paper-result` is
    // force-refetched on the paper-finished SSE event.
    getStrategyMatched: builder.query<MatchedTokenRecord[], StrategyRuleArg>({
      query: ({ strategy, ruleId }) =>
        `/api/strategies/${strategy}/rules/${encodeURIComponent(ruleId)}/matched`,
      providesTags: (_r, _e, a) => [strategyResultTag(a)],
      keepUnusedDataFor: 60,
    }),
    // A user-cancelled run resolves to `{ cancelled: true }` instead of the token
    // array (HTTP 200, no result persisted); callers type-guard on the shape.
    getStrategySimulate: builder.query<
      SimulatedTokenResult[] | { cancelled: true },
      StrategyRuleArg
    >({
      query: ({ strategy, ruleId }) =>
        `/api/strategies/${strategy}/rules/${encodeURIComponent(ruleId)}/simulate`,
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
    // Trigger a grouped DB-range sweep (single-flight on the backend — a 409 means
    // a sweep is already running). Invalidating `GroupedSweep` refetches the runs.
    // A user-cancelled sweep resolves to `{ cancelled: true }` (no run persisted).
    startGroupedSweep: builder.mutation<
      GroupedSweepRunRecord | { cancelled: true },
      GroupedSweepStartArgs
    >({
      query: (body) => ({
        url: '/api/strategies/sweeps',
        method: 'POST',
        body,
      }),
      invalidatesTags: ['GroupedSweep'],
    }),
    getTokenDetail: builder.query<TokenDetailRecord, string>({
      query: (mint) => `/api/tokens/${encodeURIComponent(mint)}`,
    }),
    getTokenTrades: builder.query<TradeRecord[], string>({
      // Explicitly bounded (see TOKEN_TRADES_LIMIT) so a high-volume token never
      // pulls an unbounded list into the chart's memory.
      query: (mint) =>
        `/api/tokens/${encodeURIComponent(mint)}/trades?limit=${TOKEN_TRADES_LIMIT}`,
    }),

    // Creator profiles + analysis results — paged server-side (limit/offset).
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

    // Wallet holdings — an expensive read (full wallet RPC scan + Jupiter batch
    // price + migration resolution). Cached like the token list so revisiting
    // the page reuses it instead of re-scanning the chain. A manual trade
    // refreshes it surgically (see getWalletHolding) rather than re-fetching.
    getWalletHoldings: builder.query<WalletHolding[], void>({
      query: () => '/api/solana/wallet/tokens',
      providesTags: ['WalletHoldings'],
    }),
    // Single-mint counterpart used only for post-trade confirmation polling:
    // one cheap RPC + one price lookup. Not exposed as a hook — callers drive
    // it imperatively via `initiate` and patch the result into the list cache.
    getWalletHolding: builder.query<WalletHolding | null, string>({
      query: (mint) => `/api/solana/wallet/tokens/${encodeURIComponent(mint)}`,
    }),
    // Live prices for the held mints, decoupled from the balance read. Polled
    // on a short interval (see the page) so the value column ticks without
    // re-scanning the wallet. Keyed by the (sorted) mint list — caller passes
    // the mints already in the balances cache.
    getWalletPrices: builder.query<Record<string, WalletPrice>, string[]>({
      query: (mints) =>
        `/api/solana/prices?ids=${mints.map(encodeURIComponent).join(',')}`,
    }),
    buyToken: builder.mutation<{ success: boolean }, BuyTokenArgs>({
      query: (body) => ({ url: '/api/solana/wallet/buy', method: 'POST', body }),
    }),
    sellToken: builder.mutation<{ success: boolean }, SellTokenArgs>({
      query: (body) => ({ url: '/api/solana/wallet/sell', method: 'POST', body }),
    }),

    // Accrued pump.fun cashback — a read-only on-chain status (two account
    // reads). Cached, not polled: cashback accrues slowly, so the wallet card
    // refreshes on mount / after a claim, never on the live price tick.
    getCashbackStatus: builder.query<CashbackStatus, void>({
      query: () => '/api/cashback/status',
      providesTags: ['Cashback'],
    }),
    // Sweep both pots back to the wallet as native SOL. Off the trade hot path;
    // invalidates the status so the card reflects the drained balance.
    claimCashback: builder.mutation<CashbackClaimResult, void>({
      query: () => ({ url: '/api/cashback/claim', method: 'POST' }),
      invalidatesTags: ['Cashback'],
    }),

    // System reads shared app-wide (header + price toggle). Folding them into
    // RTK Query collapses the StrictMode double-fire and the multiple
    // independent callers into a single deduped request per cache key.
    getSolPrice: builder.query<number | null, void>({
      query: () => '/api/system/price',
      transformResponse: (r: { usd_rate: number | null }) => r.usd_rate,
    }),
    getLiveMode: builder.query<boolean, void>({
      query: () => '/api/system/live',
      transformResponse: (r: { live: boolean }) => r.live,
      providesTags: ['LiveMode'],
    }),
    setLiveMode: builder.mutation<boolean, boolean>({
      query: (live) => ({
        url: '/api/system/live',
        method: 'PUT',
        body: { live },
      }),
      transformResponse: (r: { live: boolean }) => r.live,
      invalidatesTags: ['LiveMode'],
    }),

    // Global app settings — one shared object read by both root contexts
    // (timezone / price-unit), the Settings page, and the Sync page. A single
    // cache entry replaces what used to be 4+ independent fetches on startup.
    getSettings: builder.query<AppSettings, void>({
      query: () => '/api/system/settings',
      providesTags: ['Settings'],
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
          apiSlice.util.updateQueryData('getSettings', undefined, (draft) => {
            Object.assign(draft, patch);
          }),
        );
        try {
          const { data } = await queryFulfilled;
          dispatch(
            apiSlice.util.updateQueryData('getSettings', undefined, (draft) => {
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

export const {
  useGetTokensQuery,
  useGetTokensPageQuery,
  useGetTokenDetailQuery,
  useGetTokenTradesQuery,
  useGetCreatorsPageQuery,
  useGetAnalysisPageQuery,
  useGetGroupedSweepRunsQuery,
  useGetGroupedSweepGroupsQuery,
  useGetGroupedSweepResultsQuery,
  useStartGroupedSweepMutation,
  useGetWalletHoldingsQuery,
  useGetWalletPricesQuery,
  useBuyTokenMutation,
  useSellTokenMutation,
  useGetCashbackStatusQuery,
  useClaimCashbackMutation,
  useGetSolPriceQuery,
  useLazyGetSolPriceQuery,
  useGetLiveModeQuery,
  useSetLiveModeMutation,
  useGetSettingsQuery,
  useGetProfilesQuery,
  useUpdateSettingsMutation,
} = apiSlice;

/**
 * Extract a human-readable message from an RTK Query error, preserving the
 * `{ error: "..." }` body shape the backend returns (mirrors the old
 * `request()` helper in services/api.ts).
 */
export function apiErrorMessage(
  error: FetchBaseQueryError | SerializedError | undefined,
  fallback = 'Request failed',
): string | null {
  if (!error) return null;
  if ('status' in error) {
    const data = error.data;
    if (data && typeof data === 'object' && 'error' in data) {
      return String((data as { error: unknown }).error);
    }
    if (typeof error.status === 'number') return `HTTP ${error.status}`;
    return String(error.status);
  }
  return error.message ?? fallback;
}
