import { createApi, fetchBaseQuery } from '@reduxjs/toolkit/query/react';
import type { FetchBaseQueryError } from '@reduxjs/toolkit/query';
import type { SerializedError } from '@reduxjs/toolkit';

/** `injectEndpoints` KEEPS an already-registered definition unless told otherwise,
 *  so a Vite hot update that re-runs an endpoint module against this surviving
 *  `baseApi` silently keeps the PREVIOUS `query` builder — the page sends new
 *  args to an old URL. True in dev so a hot update actually replaces the
 *  definition; false in prod, where a duplicate name is a real mistake. */
export const OVERRIDE_ENDPOINTS_ON_HMR = import.meta.env.DEV;

/**
 * Central RTK Query cache shell. Endpoints are attached from `endpoints.ts` via
 * `injectEndpoints`. All tag types are declared up front (injectEndpoints can't
 * add tag types). Base URL is empty — the dev server proxies `/api` → the live
 * bin, and prod is served same-origin by that bin.
 */
export const baseApi = createApi({
  reducerPath: 'api',
  baseQuery: fetchBaseQuery({ baseUrl: '' }),
  keepUnusedDataFor: 120,
  tagTypes: [
    'Bootstrap',
    'Templates',
    'MetadataTemplates',
    'Wallets',
    'Launches',
    'Ingest',
    'Dimensions',
    'Positions',
    'ManageActions',
    'Ladders',
    'Volume',
  ],
  endpoints: () => ({}),
});

/** Human-readable message from an RTK Query error, preserving the backend's
 *  `{ error: "..." }` body shape (falls back to the raw text/status). */
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
    if (typeof data === 'string' && data) return data;
    if (typeof error.status === 'number') return `HTTP ${error.status}`;
    return String(error.status);
  }
  return error.message ?? fallback;
}
