import { createApi, fetchBaseQuery } from '@reduxjs/toolkit/query/react';
import type { FetchBaseQueryError } from '@reduxjs/toolkit/query';
import type { SerializedError } from '@reduxjs/toolkit';
import { API_BASE } from 'services/config';
import type { TokenDetailRecord, TokenRecord, TradeRecord } from 'types';

export interface TokensArgs {
  search: string;
  limit: number;
  offset: number;
}

export interface TokensResponse {
  total: number;
  items: TokenRecord[];
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
  }),
});

export const {
  useGetTokensQuery,
  useGetTokenDetailQuery,
  useGetTokenTradesQuery,
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
