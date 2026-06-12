import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { POLL_INTERVAL_MS } from 'services/config';
import { useVisiblePolling } from './useVisiblePolling';
import type { RulePositionRecord, RuleRecord } from 'types';

const MAX_SILENT_FAILURES = 3;

/** A rule whose positions can no longer change: entries are off (`is_active`
 *  false) AND nothing is still draining. Active rules can open new positions;
 *  draining rules still have exits running, so both keep polling. */
function isSettled(rule: RuleRecord | undefined): boolean {
  return !!rule && !rule.is_active && rule.open_positions === 0;
}

export interface RulePositions {
  positions: RulePositionRecord[];
  loading: boolean;
  error: string | null;
}

/**
 * Owns the open-position list for the selected rule. Fetches once on select
 * (with a spinner), then polls silently — but only while the tab is visible and
 * only while the rule can still change. Each fetch is abortable, so switching
 * rules quickly can't let a stale response overwrite a newer one.
 */
export function useRulePositions(
  selectedRuleId: string | null,
  rules: RuleRecord[],
  fetchPositions: (ruleId: string, signal: AbortSignal) => Promise<RulePositionRecord[]>,
): RulePositions {
  const [positions, setPositions] = useState<RulePositionRecord[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const failures = useRef(0);
  // Latest in-flight request; aborted when the rule changes or we unmount.
  const inflight = useRef<AbortController | null>(null);

  const settled = useMemo(
    () => isSettled(rules.find((r) => r.id === selectedRuleId)),
    [rules, selectedRuleId],
  );

  const load = useCallback(
    async (ruleId: string, silent: boolean) => {
      inflight.current?.abort();
      const ctrl = new AbortController();
      inflight.current = ctrl;
      if (!silent) setLoading(true);
      try {
        const data = await fetchPositions(ruleId, ctrl.signal);
        if (ctrl.signal.aborted) return;
        setPositions(data);
        setError(null);
        failures.current = 0;
      } catch (e) {
        if (ctrl.signal.aborted || (e instanceof DOMException && e.name === 'AbortError')) return;
        const msg = e instanceof Error ? e.message : 'Failed to load positions';
        if (!silent || ++failures.current >= MAX_SILENT_FAILURES) setError(msg);
      } finally {
        if (!silent && !ctrl.signal.aborted) setLoading(false);
      }
    },
    [fetchPositions],
  );

  // Reset + non-silent initial fetch whenever the selection changes.
  useEffect(() => {
    failures.current = 0;
    if (!selectedRuleId) {
      inflight.current?.abort();
      setPositions([]);
      setError(null);
      setLoading(false);
      return;
    }
    void load(selectedRuleId, false);
    return () => inflight.current?.abort();
  }, [selectedRuleId, load]);

  // Silent poll — no leading call (the effect above did the first fetch), and
  // disabled once the rule is settled (nothing left to refresh).
  useVisiblePolling(
    () => {
      if (selectedRuleId) void load(selectedRuleId, true);
    },
    POLL_INTERVAL_MS,
    !!selectedRuleId && !settled,
    false,
  );

  return { positions, loading, error };
}
