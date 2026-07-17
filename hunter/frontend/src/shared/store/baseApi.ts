import { createApi, fetchBaseQuery } from '@reduxjs/toolkit/query/react';
import type { FetchBaseQueryError } from '@reduxjs/toolkit/query';
import type { SerializedError } from '@reduxjs/toolkit';
import { API_BASE } from 'services/config';

/**
 * Central RTK Query cache **shell**. Endpoint definitions are attached from
 * per-mode modules (`sharedEndpoints` / `liveEndpoints` / `labEndpoints`)
 * via `injectEndpoints`, so each build bundles only the endpoints its mode needs.
 *
 * `keepUnusedDataFor` retains a cache entry for 5 minutes after the last
 * component unsubscribes, and `refetchOnMountOrArgChange: false` means
 * navigating back to a page reuses that cache instead of re-fetching.
 *
 * **All tag types are declared here up front** — `injectEndpoints` cannot add
 * tag types, and an unused tag in a given mode is inert.
 */
export const baseApi = createApi({
  reducerPath: 'api',
  baseQuery: fetchBaseQuery({ baseUrl: API_BASE }),
  keepUnusedDataFor: 300,
  refetchOnMountOrArgChange: false,
  tagTypes: [
    'Settings',
    'LiveMode',
    'WalletHoldings',
    'StrategyResult',
    'StrategyPaper',
    'Profiles',
    'Cashback',
    'GroupedSweep',
    'GroupedSweepGroups',
    'TokenBatch',
    'Fingerprint',
    'StrategyRule',
  ],
  endpoints: () => ({}),
});

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
