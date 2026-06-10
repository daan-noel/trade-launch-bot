import { createApi, fetchBaseQuery } from '@reduxjs/toolkit/query/react';
import type { FetchBaseQueryError } from '@reduxjs/toolkit/query';
import type { SerializedError } from '@reduxjs/toolkit';
import { API_BASE } from 'services/config';
import type { AppSettings } from 'services/api';
import type {
  TokenDetailRecord,
  TokenRecord,
  TradeRecord,
  WalletHolding,
  WalletPrice,
} from 'types';

export interface TokensArgs {
  search: string;
  limit: number;
  offset: number;
}

export interface TokensResponse {
  total: number;
  items: TokenRecord[];
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
  token_amount: number;
  token_account: string;
  slippage_bps?: number;
}

/**
 * Central RTK Query cache for expensive / shared server data.
 *
 * `keepUnusedDataFor` retains a cache entry for 5 minutes after the last
 * component unsubscribes, and `refetchOnMountOrArgChange: false` means
 * navigating back to a page reuses that cache instead of re-fetching. The big
 * 5000-row token list is therefore fetched once and shared by every page that
 * subscribes with the same args (Tokens + Swing detection).
 */
export const apiSlice = createApi({
  reducerPath: 'api',
  baseQuery: fetchBaseQuery({ baseUrl: API_BASE }),
  keepUnusedDataFor: 300,
  refetchOnMountOrArgChange: false,
  tagTypes: ['Settings', 'LiveMode', 'WalletHoldings'],
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
    }),
    getTokenDetail: builder.query<TokenDetailRecord, string>({
      query: (mint) => `/api/tokens/${encodeURIComponent(mint)}`,
    }),
    getTokenTrades: builder.query<TradeRecord[], string>({
      query: (mint) => `/api/tokens/${encodeURIComponent(mint)}/trades`,
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
  useGetTokenDetailQuery,
  useGetTokenTradesQuery,
  useGetWalletHoldingsQuery,
  useGetWalletPricesQuery,
  useBuyTokenMutation,
  useSellTokenMutation,
  useGetSolPriceQuery,
  useLazyGetSolPriceQuery,
  useGetLiveModeQuery,
  useSetLiveModeMutation,
  useGetSettingsQuery,
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
