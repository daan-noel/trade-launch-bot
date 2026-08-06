/**
 * Page through a server table until the full filtered cohort is in hand — feeds
 * position-summary charts (distribution / scatter / equity) so they never fold
 * only the current DataTable page.
 *
 * Soft-caps at `maxRows` to protect the browser on pathological All-time scopes;
 * the run selector remains the primary bound.
 */

import type { TableRequestBody } from 'services/tableRequest';

const PAGE = 1000;
const DEFAULT_MAX = 20_000;

export async function fetchAllTablePages<T>(
  fetchPage: (
    body: TableRequestBody,
    signal: AbortSignal,
  ) => Promise<{ items: T[]; total: number }>,
  baseBody: TableRequestBody,
  signal: AbortSignal,
  maxRows = DEFAULT_MAX,
): Promise<T[]> {
  const out: T[] = [];
  let page = 1;
  let total = Infinity;
  while (out.length < total && out.length < maxRows) {
    const body: TableRequestBody = {
      ...baseBody,
      pagination: { page, pageSize: PAGE },
    };
    const { items, total: t } = await fetchPage(body, signal);
    total = t;
    out.push(...items);
    if (items.length === 0) break;
    page += 1;
    if (out.length >= total) break;
  }
  return out;
}
