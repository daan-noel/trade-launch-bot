import { useCallback, useEffect, useRef, useState } from 'react';
import type { TableQuery } from 'components/table/types';

/** Initial view-state for a positions / results table: page 1, default size, no
 *  sort/filter. Pages seed `posQuery` with this, then replace it from DataTable. */
export const DEFAULT_POSITIONS_QUERY: TableQuery = {
  page: 1,
  pageSize: 20,
  sortKeys: [],
  search: '',
  colFilters: {},
};

/** One fetched page of a server-side table. */
export interface ServerPage<R> {
  items: R[];
  /** Full match count (from `X-Total-Count`) so the pager can size itself. */
  total: number;
}

export interface UseServerTable<R, S> {
  items: R[];
  total: number;
  summary: S | null;
  loading: boolean;
  error: string | null;
  /** Force a refetch of the current page + summary (e.g. after a run completes). */
  reload: () => void;
}

/**
 * Generic server-side table driver: owns the current *page* + total + optional
 * filtered-population summary, refetching whenever `body` (the serialized
 * `TableQuery` + extras) changes, with stale-response protection so quick
 * paging/sorting can't let an older response overwrite a newer one.
 *
 * Live position patching is NOT here — Matched/Simulated results are static once
 * computed. This hook is for those: page/sort/filter server round-trips over a
 * fixed result set.
 *
 * `enabled` gates fetching (e.g. no rule selected, or the sim hasn't run yet);
 * when false the hook clears to empty. `body` should be referentially stable for
 * an unchanged view-state — callers memoize it (e.g. `useMemo(toTableRequest…)`)
 * or pass its JSON key via `bodyKey`. Pass `summaryBody` when the summary
 * endpoint ignores pagination/sort (defaults to `body`).
 *
 * Fetchers are read from refs (not effect deps) so an inline arrow doesn't
 * retrigger a clear→refetch loop that flashes the summary card / Temporal band.
 * When the closed-over resource changes without the filter body changing (e.g.
 * another rule id), pass `resourceKey` so both effects re-run.
 */
export function useServerTable<R, S = unknown>(
  enabled: boolean,
  body: unknown,
  fetchPage: (body: unknown, signal: AbortSignal) => Promise<ServerPage<R>>,
  fetchSummary?: (summaryBody: unknown, signal: AbortSignal) => Promise<S>,
  summaryBody?: unknown,
  /** Bust page+summary caches when the closed-over resource changes (rule id, scope). */
  resourceKey?: string | number,
): UseServerTable<R, S> {
  const [items, setItems] = useState<R[]>([]);
  const [total, setTotal] = useState(0);
  const [summary, setSummary] = useState<S | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const inflight = useRef<AbortController | null>(null);
  const summaryInflight = useRef<AbortController | null>(null);
  const bodyKey = JSON.stringify(body);
  const summaryKey = JSON.stringify(summaryBody ?? body);
  const [nonce, setNonce] = useState(0);
  const reload = useCallback(() => setNonce((n) => n + 1), []);

  const fetchPageRef = useRef(fetchPage);
  fetchPageRef.current = fetchPage;
  const fetchSummaryRef = useRef(fetchSummary);
  fetchSummaryRef.current = fetchSummary;

  useEffect(() => {
    if (!enabled) {
      inflight.current?.abort();
      setItems([]);
      setTotal(0);
      setError(null);
      setLoading(false);
      return;
    }
    inflight.current?.abort();
    const ctrl = new AbortController();
    inflight.current = ctrl;
    setLoading(true);
    void (async () => {
      try {
        const page = await fetchPageRef.current(body, ctrl.signal);
        if (ctrl.signal.aborted) return;
        setItems(page.items);
        setTotal(page.total);
        setError(null);
      } catch (e) {
        if (ctrl.signal.aborted || (e instanceof DOMException && e.name === 'AbortError')) return;
        setError(e instanceof Error ? e.message : 'Failed to load');
      } finally {
        if (inflight.current === ctrl) setLoading(false);
      }
    })();
    return () => ctrl.abort();
    // `body` is captured via `bodyKey`. Fetchers live in refs — pass `resourceKey`
    // when the closed-over id/scope changes without the filter body changing.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled, bodyKey, nonce, resourceKey]);

  useEffect(() => {
    if (!enabled || !fetchSummaryRef.current) {
      setSummary(null);
      return;
    }
    summaryInflight.current?.abort();
    const ctrl = new AbortController();
    summaryInflight.current = ctrl;
    // Drop the prior cohort's card immediately so a failed/slow summary can't
    // leave another filter's aggregates on screen (or look like "no summary").
    setSummary(null);
    const payload = summaryBody ?? body;
    void (async () => {
      try {
        const s = await fetchSummaryRef.current!(payload, ctrl.signal);
        if (!ctrl.signal.aborted) setSummary(s);
      } catch (e) {
        if (e instanceof DOMException && e.name === 'AbortError') return;
        // Summary is non-blocking; leave null rather than a stale prior value.
      }
    })();
    return () => ctrl.abort();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled, summaryKey, nonce, resourceKey]);

  return { items, total, summary, loading, error, reload };
}
