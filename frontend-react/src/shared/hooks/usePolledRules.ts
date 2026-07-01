import { useCallback, useEffect, useRef, useState, type Dispatch, type SetStateAction } from 'react';
import { FALLBACK_POLL_INTERVAL_MS } from 'services/config';
import { connectTpslPositionsChanged, connectTpslRulesChanged } from 'services/sse';
import { useVisiblePolling } from './useVisiblePolling';
import type { RuleRecord } from 'types';

/** Coalesce a burst of SSE signals (e.g. stop-and-close fires many position
 *  events at once) into a single refetch. */
const REFRESH_DEBOUNCE_MS = 200;

/** Recompute the lifecycle badge from the new open count — mirrors the backend
 *  `lifecycle_label`. A position change can only flip Active↔Draining↔Idle; the
 *  paper-only "Finished" state arrives via `tpsl_rules_changed` /
 *  `paper_test_finished`, so it's preserved here rather than recomputed. */
function deriveLifecycle(rule: RuleRecord, open: number): string {
  if (rule.is_active) return 'Active';
  if (open > 0) return 'Draining';
  if (rule.trade_mode === 'paper' && rule.lifecycle === 'Finished') return 'Finished';
  return 'Idle';
}

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
 * Owns the rule list for a strategy page: one non-silent initial load, then
 * SSE-driven silent refreshes (the backend pushes `tpsl_rules_changed` /
 * `tpsl_positions_changed`), with a slow visibility-gated poll as a safety net
 * for dropped frames. Shared by Tpsl1Page and Tpsl2Page — they differ only in
 * `strategy` and which `fetchRules` they pass.
 */
export function usePolledRules(
  fetchRules: () => Promise<RuleRecord[]>,
  strategy: 'tpsl1' | 'tpsl2' | 'swing_1',
): PolledRules {
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

  // Primary path: refetch (debounced) whenever the backend signals a rule or
  // position change, so the list updates in real time instead of on a timer.
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | null = null;
    const handle = connectTpslRulesChanged(strategy, () => {
      if (timer) return;
      timer = setTimeout(() => {
        timer = null;
        void refresh(true);
      }, REFRESH_DEBOUNCE_MS);
    });
    return () => {
      if (timer) clearTimeout(timer);
      handle.close();
    };
  }, [strategy, refresh]);

  // Position deltas only shift a rule's open/total counts + lifecycle badge —
  // patch those in place instead of refetching the whole list on every fill/exit.
  // Create/update/delete and lifecycle actions still come via `tpsl_rules_changed`
  // above (a real refetch). Coalesced so a stop-and-close burst is one update.
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | null = null;
    const pending = new Map<string, { open: number; total: number }>();
    const flush = () => {
      timer = null;
      const updates = new Map(pending);
      pending.clear();
      setRules((prev) =>
        prev.map((r) => {
          const u = updates.get(r.id);
          if (!u) return r;
          return {
            ...r,
            open_positions: u.open,
            total_positions: u.total,
            lifecycle: deriveLifecycle(r, u.open),
          };
        }),
      );
    };
    const handle = connectTpslPositionsChanged(strategy, (delta) => {
      pending.set(delta.ruleId, { open: delta.openPositions, total: delta.totalPositions });
      if (!timer) timer = setTimeout(flush, REFRESH_DEBOUNCE_MS);
    });
    return () => {
      if (timer) clearTimeout(timer);
      handle.close();
    };
  }, [strategy]);

  // Safety net: a slow visibility-gated poll catches anything a dropped/lagged
  // SSE frame missed. No leading call (the initial-load effect covers mount).
  useVisiblePolling(() => void refresh(true), FALLBACK_POLL_INTERVAL_MS, true, false);

  return { rules, setRules, loading, error, refresh };
}
