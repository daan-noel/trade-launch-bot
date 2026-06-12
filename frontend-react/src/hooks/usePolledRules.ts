import { useCallback, useEffect, useRef, useState, type Dispatch, type SetStateAction } from 'react';
import { POLL_INTERVAL_MS } from 'services/config';
import { useVisiblePolling } from './useVisiblePolling';
import type { RuleRecord } from 'types';

/** Surface an error only after this many *consecutive* silent-poll failures, so
 *  a single dropped request doesn't flash an alert but a real outage still shows. */
const MAX_SILENT_FAILURES = 3;

export interface PolledRules {
  rules: RuleRecord[];
  setRules: Dispatch<SetStateAction<RuleRecord[]>>;
  loading: boolean;
  error: string | null;
  /** Force a refresh. Pass `silent` to suppress the loading spinner / error flash
   *  (used by the SSE handler when a paper test finishes). */
  refresh: (silent?: boolean) => Promise<void>;
}

/**
 * Owns the rule list for a strategy page: one non-silent initial load, then a
 * visibility-gated silent poll. Shared by Tpsl1Page and Tpsl2Page — they differ
 * only in which `fetchRules` they pass.
 */
export function usePolledRules(fetchRules: () => Promise<RuleRecord[]>): PolledRules {
  const [rules, setRules] = useState<RuleRecord[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const failures = useRef(0);

  const refresh = useCallback(
    async (silent = false) => {
      if (!silent) setLoading(true);
      try {
        const data = await fetchRules();
        setRules(data);
        setError(null);
        failures.current = 0;
      } catch (e) {
        const msg = e instanceof Error ? e.message : 'Failed to load rules';
        // Non-silent failures show immediately; silent ones only once they
        // persist, so transient blips during polling stay invisible.
        if (!silent || ++failures.current >= MAX_SILENT_FAILURES) setError(msg);
      } finally {
        if (!silent) setLoading(false);
      }
    },
    [fetchRules],
  );

  // One non-silent initial load (shows the spinner).
  useEffect(() => {
    void refresh();
  }, [refresh]);

  // Recurring silent refresh — no leading call (the effect above covers mount).
  useVisiblePolling(() => void refresh(true), POLL_INTERVAL_MS, true, false);

  return { rules, setRules, loading, error, refresh };
}
