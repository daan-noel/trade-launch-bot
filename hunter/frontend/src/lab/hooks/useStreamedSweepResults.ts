import { useEffect, useRef, useState } from 'react';
import type { GroupedSweepResultRecord } from '@lab/components/sweep/groupedTypes';
import type { SortEntry } from 'components/table/types';
import type { FilterSpec } from 'components/table/numericFilter';

export const COMBO_PAGE_SIZE = 200;

export interface StreamedSweepState {
  rows: GroupedSweepResultRecord[];
  total: number;
  loading: boolean;
  error: string | null;
}

/**
 * Fetches one page of combo results as NDJSON and streams rows into state as
 * each line arrives. Rows become visible progressively rather than all at once.
 *
 * The backend returns `X-Total-Count` (of the *filtered* set) so the DataTable can
 * render a correct page count. A new fetch fires whenever
 * strategyId/runId/groupId/page/pageSize/sort/filters change; the previous fetch is
 * aborted.
 */
export function useStreamedSweepResults(
  strategyId: string,
  runId: string | null,
  groupId: string | null,
  page: number,
  pageSize: number,
  sortKeys: SortEntry[] = [],
  filters: Record<string, FilterSpec> = {},
): StreamedSweepState {
  const [rows, setRows] = useState<GroupedSweepResultRecord[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const abortRef = useRef<AbortController | null>(null);
  const scopeRef = useRef<string | null>(null);

  useEffect(() => {
    if (!runId || !groupId) {
      setRows([]);
      setTotal(0);
      setLoading(false);
      setError(null);
      return;
    }

    abortRef.current?.abort();
    const controller = new AbortController();
    abortRef.current = controller;

    setRows([]);
    // A different run/group is a different dataset — its row count has nothing
    // to do with the previous one, so drop the stale `total` (the pager would
    // otherwise advertise the old group's page count for the new selection).
    // Within the same (run, group) the note below applies and it is kept.
    if (scopeRef.current !== `${runId} ${groupId}`) {
      scopeRef.current = `${runId} ${groupId}`;
      setTotal(0);
    }
    // NOTE: do NOT reset `total` to 0 here. The DataTable's server-side
    // page-clamp effect reacts to `serverTotal`; a transient 0 makes it compute
    // `totalPages = 1` and snap the user back to page 1 the instant they change
    // page or sort (which trips a refetch). Keep the prior count until the new
    // `X-Total-Count` header lands below.
    setLoading(true);
    setError(null);

    // Multi-key sort: ordered `col:dir,…` list (index 0 = primary). The backend
    // applies every level in order with a stable tiebreak; sending only the
    // primary would silently drop the secondary keys the user picked.
    const sortParams =
      sortKeys.length > 0
        ? `&sort=${encodeURIComponent(sortKeys.map((s) => `${s.col}:${s.dir}`).join(','))}`
        : '';
    // Per-column filters as a URL-encoded JSON object the backend applies to both
    // the page query and the `X-Total-Count` count (so the pager stays correct).
    const filterParams =
      Object.keys(filters).length > 0
        ? `&filters=${encodeURIComponent(JSON.stringify(filters))}`
        : '';
    const url =
      `/api/strategies/sweeps/${encodeURIComponent(runId)}` +
      `/groups/${encodeURIComponent(groupId)}` +
      `/results?strategy_id=${encodeURIComponent(strategyId)}` +
      `&page=${page}&limit=${pageSize}${sortParams}${filterParams}`;

    (async () => {
      try {
        const res = await fetch(url, { signal: controller.signal });
        if (!res.ok) {
          throw new Error(`HTTP ${res.status}`);
        }

        const rawTotal = res.headers.get('X-Total-Count');
        if (rawTotal) setTotal(Number(rawTotal));

        if (!res.body) {
          throw new Error('No response body');
        }

        const reader = res.body.getReader();
        const decoder = new TextDecoder();
        let buffer = '';

        while (true) {
          const { done, value } = await reader.read();
          if (done) break;
          buffer += decoder.decode(value, { stream: true });
          const lines = buffer.split('\n');
          // Keep the last (potentially incomplete) line in the buffer
          buffer = lines.pop() ?? '';
          const parsed = lines
            .filter((l) => l.trim())
            .map((l) => JSON.parse(l) as GroupedSweepResultRecord);
          if (parsed.length > 0) {
            setRows((prev) => [...prev, ...parsed]);
          }
        }
        // Flush any remaining buffered line
        if (buffer.trim()) {
          setRows((prev) => [...prev, JSON.parse(buffer) as GroupedSweepResultRecord]);
        }
      } catch (e) {
        if ((e as Error).name !== 'AbortError') {
          setError((e as Error).message ?? 'Failed to load combo results');
        }
      } finally {
        // An aborted fetch settles AFTER its replacement already set `loading`
        // true — clearing it here would hide the spinner while the new stream
        // is still filling in. Only the live fetch may end the loading state.
        if (!controller.signal.aborted) setLoading(false);
      }
    })();

    return () => controller.abort();
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [strategyId, runId, groupId, page, pageSize, JSON.stringify(sortKeys), JSON.stringify(filters)]);

  return { rows, total, loading, error };
}
