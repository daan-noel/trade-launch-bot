import { createApi, fetchBaseQuery } from '@reduxjs/toolkit/query/react';
import type { FetchBaseQueryError } from '@reduxjs/toolkit/query';
import type { SerializedError } from '@reduxjs/toolkit';

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
