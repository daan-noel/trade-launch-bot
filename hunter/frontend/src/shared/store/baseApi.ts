import { createApi, fetchBaseQuery } from '@reduxjs/toolkit/query/react';
import type { FetchBaseQueryError } from '@reduxjs/toolkit/query';
import type { SerializedError } from '@reduxjs/toolkit';
import { API_BASE } from 'services/config';

/** `injectEndpoints` KEEPS an already-registered definition unless told otherwise,
 *  so a Vite hot update that re-runs an endpoint module against this surviving
 *  `baseApi` silently keeps the PREVIOUS `query` builder — the page sends new
 *  args to an old URL. True in dev so a hot update actually replaces the
 *  definition; false in prod, where a duplicate name is a real mistake. */
export const OVERRIDE_ENDPOINTS_ON_HMR = import.meta.env.DEV;

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
    // Which transport carries curve traffic (`grpc` | `nats`). Its own tag, not
    // `Settings`: the switch re-points the live feed, so its readback must refresh
    // on the mutation alone and not wait on the broad settings cache.
    'CurveSource',
    'WalletHoldings',
    // Closed-trade PnL for the Portfolio page (both real + paper). Split from
    // WalletHoldings so a paper close can refresh performance WITHOUT triggering
    // the expensive real-wallet RPC scan those holdings reads carry.
    'PortfolioPerf',
    'StrategyResult',
    'StrategyPaper',
    'Profiles',
    'Cashback',
    'GroupedSweep',
    'GroupedSweepGroups',
    'TokenBatch',
    'Fingerprint',
    'StrategyRule',
    // Per-position fill ledger, keyed by position id. Needs its own tag because a
    // scale-out leg lands mid-view: with `keepUnusedDataFor: 300` +
    // `refetchOnMountOrArgChange: false`, an untagged entry would serve the
    // pre-leg ledger for five minutes, including across a close/reopen.
    'PositionFills',
    // Analysis-owned ix_labels pattern sets (Trader Analysis' flow lens). Its own
    // tag, not `Fingerprint`: the two are separate owners of the same fact and a
    // lens edit must not invalidate the rule-facing fingerprint cache.
    'IxPatternSet',
    // A mint's re-entry episodes (chart marker overlay), keyed by mint. A new entry
    // on a mint already on screen has to redraw the overlay, same as a leg does.
    'MintEpisodes',
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
