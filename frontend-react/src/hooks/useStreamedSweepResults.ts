import { useEffect, useRef, useState } from 'react';
import type { GroupedSweepResultRecord } from 'components/sweep/groupedTypes';

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
 * The backend returns `X-Total-Count` so the DataTable can render a correct
 * page count. A new fetch fires whenever strategyId/runId/groupId/page/pageSize
 * changes; the previous fetch is aborted.
 */
export function useStreamedSweepResults(
  strategyId: string,
  runId: string | null,
  groupId: string | null,
  page: number,
  pageSize: number,
): StreamedSweepState {
  const [rows, setRows] = useState<GroupedSweepResultRecord[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const abortRef = useRef<AbortController | null>(null);

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
    setTotal(0);
    setLoading(true);
    setError(null);

    const url =
      `/api/strategies/sweeps/${encodeURIComponent(runId)}` +
      `/groups/${encodeURIComponent(groupId)}` +
      `/results?strategy_id=${encodeURIComponent(strategyId)}` +
      `&page=${page}&limit=${pageSize}`;

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
        setLoading(false);
      }
    })();

    return () => controller.abort();
  }, [strategyId, runId, groupId, page, pageSize]);

  return { rows, total, loading, error };
}
