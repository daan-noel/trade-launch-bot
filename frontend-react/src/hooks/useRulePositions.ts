import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { FALLBACK_POLL_INTERVAL_MS } from 'services/config';
import { connectTpslPositionsChanged } from 'services/sse';
import { useVisiblePolling } from './useVisiblePolling';
import type { RulePositionRecord, RuleRecord } from 'types';

const MAX_SILENT_FAILURES = 3;
/** Coalesce a burst of position signals for the same rule into one refetch. */
const REFRESH_DEBOUNCE_MS = 200;

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
 * (with a spinner), then refetches on the backend's `tpsl_positions_changed`
 * push, with a slow visibility-gated poll as a safety net. Polling stops once
 * the rule is settled. Each fetch is abortable, so switching rules quickly can't
 * let a stale response overwrite a newer one.
 */
export function useRulePositions(
  selectedRuleId: string | null,
  rules: RuleRecord[],
  fetchPositions: (ruleId: string, signal: AbortSignal) => Promise<RulePositionRecord[]>,
  strategy: 'tpsl1' | 'tpsl2',
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

  // Primary path: refetch (debounced) when the backend signals a position change
  // for the selected rule. Skipped once the rule is settled (no more changes).
  useEffect(() => {
    if (!selectedRuleId || settled) return;
    let timer: ReturnType<typeof setTimeout> | null = null;
    const handle = connectTpslPositionsChanged(strategy, (ruleId) => {
      if (ruleId !== selectedRuleId || timer) return;
      timer = setTimeout(() => {
        timer = null;
        void load(selectedRuleId, true);
      }, REFRESH_DEBOUNCE_MS);
    });
    return () => {
      if (timer) clearTimeout(timer);
      handle.close();
    };
  }, [strategy, selectedRuleId, settled, load]);

  // Safety net: a slow visibility-gated poll catches dropped/lagged SSE frames.
  // No leading call (the select effect did the first fetch); off once settled.
  useVisiblePolling(
    () => {
      if (selectedRuleId) void load(selectedRuleId, true);
    },
    FALLBACK_POLL_INTERVAL_MS,
    !!selectedRuleId && !settled,
    false,
  );

  return { positions, loading, error };
}
