import { useCallback, useEffect, useState } from 'react';

export interface Resource<T> {
  data: T | undefined;
  loading: boolean;
  error: string | null;
  /** Re-run the fetcher (e.g. after a mutation) — toggles `loading`. */
  reload: () => Promise<void>;
  /**
   * Silent re-fetch: refresh `data` in place WITHOUT flipping `loading`, so a
   * background poll doesn't flash the table's loading state every tick.
   */
  refresh: () => Promise<void>;
  /** Surface a mutation error through the same channel as load errors. */
  setError: (e: string | null) => void;
}

/**
 * Standard fetch-with-loading/error/reload for a management tab — replaces the
 * hand-rolled `loading`/`error`/`load()` triplet each tab reimplemented (and
 * which had already drifted in where the error rendered). `deps` re-runs the
 * fetcher when they change (e.g. a role filter), matching a `useEffect` dep list.
 */
export function useResource<T>(fetcher: () => Promise<T>, deps: unknown[] = []): Resource<T> {
  const [data, setData] = useState<T | undefined>(undefined);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setData(await fetcher());
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);

  const refresh = useCallback(async () => {
    try {
      setData(await fetcher());
    } catch (e) {
      setError(String(e));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);

  useEffect(() => {
    reload();
  }, [reload]);

  return { data, loading, error, reload, refresh, setError };
}
