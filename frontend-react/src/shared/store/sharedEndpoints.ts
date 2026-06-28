import { baseApi } from './baseApi';
import type { AppSettings } from 'services/api';
import type { TokenFilters } from 'components/tokens/filters';
import type { SortEntry } from 'components/table/types';
import { datetimeLocalToUtcWallClock } from 'utils/date';
import type {
  CreationStatsArgs,
  CreationStatsResponse,
} from 'components/creation-stats/creationStats';
import type {
  TokenDetailRecord,
  TokenRecord,
  TradeRecord,
  WalletProfile,
} from 'types';

/**
 * Per-backend feature advertisement (`GET /api/system/capabilities`). The live
 * bin reports `{ has_live_trading: true, has_analysis: false }`; the local bin the
 * inverse. (Legacy: the split SPA no longer gates on this at runtime — kept for
 * diagnostics / the capabilities probe.)
 */
export interface Capabilities {
  has_live_trading: boolean;
  has_analysis: boolean;
}

export interface TokensArgs {
  search: string;
  limit: number;
  offset: number;
}

/**
 * The datetime-range `TokenFilters` keys and which side of the range each is.
 * The bound only matters inside a DST transition, where it makes the wall-clock
 * → UTC tie-break inclusion-safe (see `datetimeLocalToUtcWallClock`). Any key not
 * listed here is passed to the backend verbatim.
 */
const DATETIME_FILTER_BOUND = {
  created_from: 'lower',
  created_to: 'upper',
  last_trade_from: 'lower',
  last_trade_to: 'upper',
  ath_from: 'lower',
  ath_to: 'upper',
} as const;

/**
 * Args for the server-side paginated Tokens view. Unlike `TokensArgs` (which
 * pulls the whole list for client-side analysis on the Swing page), this asks
 * the backend to filter/sort/page so only one page crosses the wire. Mirrors
 * the DataTable view-state plus the global `TokenFilters` panel.
 */
export interface TokensPageArgs {
  page: number; // 1-based
  pageSize: number;
  sortKeys: SortEntry[];
  search: string;
  colFilters: Record<string, string>;
  filters: TokenFilters;
  /**
   * Selected project timezone. Exists to normalize the datetime-range `f_*`
   * filters from picker wall-clock to the exact UTC instant at the param
   * boundary (see `DATETIME_FILTER_BOUND` / `datetimeLocalToUtcWallClock`). Also
   * makes the RTK cache key tz-aware so switching timezone correctly refetches.
   */
  timezone: string;
  /**
   * Swing Detection page only: the last "Swing Detection All" run id and its
   * chain-latency budget. Sent so the backend can sort the chain columns
   * (`swing_pairs` / `max_seq_pairs` / `chain_count`) from that run's raw legs,
   * re-grouping at the latency without re-running detection. Omitted elsewhere.
   */
  swingRunId?: string | null;
  swingChainLatencyMs?: number;
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
 * Upper bound on the per-token trade history pulled for the price chart. The
 * backend `get_trades` handler already caps the response at 5000; requesting it
 * explicitly makes the guardrail visible at the call site (rather than relying
 * on a silent server default) and keeps the chart's working set bounded.
 */
export const TOKEN_TRADES_LIMIT = 5_000;

export interface TokensResponse {
  total: number;
  /** Filtered count restricted to the live, cache-tracked subset (≤ `total`).
   *  Defaulted to `total` for the simple `getTokens` endpoint, which doesn't
   *  emit it. */
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
 * price, the creation-stats heatmap, and capabilities. Injected onto `baseApi`
 * so each build's store registers them for side-effect.
 */
export const sharedApi = baseApi.injectEndpoints({
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
        // Multi-key sort: ordered `col:dir,…` list (index 0 = primary). The
        // backend applies every level in order with a stable tiebreak.
        if (a.sortKeys.length > 0) {
          p.set('sort', a.sortKeys.map((s) => `${s.col}:${s.dir}`).join(','));
        }
        const cf = Object.entries(a.colFilters)
          .filter(([, v]) => v.trim())
          .map(([k, v]) => `${k}:${v}`)
          .join(';');
        if (cf) p.set('cf', cf);
        for (const [k, v] of Object.entries(a.filters)) {
          if (!v) continue;
          // Datetime-range filters carry picker wall-clock in the selected tz;
          // normalize to the exact UTC instant the backend's `parse_dt` expects.
          const bound = DATETIME_FILTER_BOUND[k as keyof typeof DATETIME_FILTER_BOUND];
          const out = bound
            ? datetimeLocalToUtcWallClock(String(v), a.timezone, bound)
            : String(v);
          if (out) p.set(`f_${k}`, out);
        }
        // Chain-column sort inputs (Swing Detection page). The backend only reads
        // them when `sort_col` is a chain column, but sending them always keeps
        // the cache key in sync with the active run + latency.
        if (a.swingRunId) {
          p.set('swing_run_id', a.swingRunId);
          if (a.swingChainLatencyMs != null) {
            p.set('swing_chain_latency_ms', String(a.swingChainLatencyMs));
          }
        }
        if (a.trackedOnly) p.set('tracked_only', 'true');
        return `/api/tokens?${p.toString()}`;
      },
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
    // Batch token lookup by mint list — used by strategy pages to enrich their
    // result tables without extending the strategy response structs. Keyed by
    // the sorted, comma-joined mint string so the same set of mints always hits
    // the same cache entry regardless of the order they were collected in.
    // `keepUnusedDataFor: 120` matches the per-token refresh cadence.
    getTokensByMints: builder.query<TokenRecord[], string[]>({
      query: (mints) => ({
        url: '/api/tokens/batch',
        method: 'POST',
        body: { mints: [...mints].sort() },
      }),
      keepUnusedDataFor: 120,
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
    // Per-backend capability flags (live = live trading, lab = analysis).
    // Static per backend, so it is subscribed once and never refetched.
    getCapabilities: builder.query<Capabilities, void>({
      query: () => '/api/system/capabilities',
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

export const {
  useGetTokensQuery,
  useGetTokensByMintsQuery,
  useGetTokensPageQuery,
  useGetCreationStatsQuery,
  useGetTokenDetailQuery,
  useGetTokenTradesQuery,
  useGetCapabilitiesQuery,
  useGetSolPriceQuery,
  useLazyGetSolPriceQuery,
  useGetSettingsQuery,
  useGetProfilesQuery,
  useUpdateSettingsMutation,
} = sharedApi;
