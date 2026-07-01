import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { FALLBACK_POLL_INTERVAL_MS } from 'services/config';
import { connectTpslPositionsChanged } from 'services/sse';
import { useVisiblePolling } from './useVisiblePolling';
import type { RulePositionRecord, RuleRecord, TpslPositionDelta } from 'types';

const MAX_SILENT_FAILURES = 3;
/** Coalesce a burst of position deltas into one in-place state update. */
const REFRESH_DEBOUNCE_MS = 200;

/** Apply a coalesced batch of deltas to the current list: upsert changed rows by
 *  id, drop removed ones. Order is preserved for existing rows; brand-new rows
 *  append (the table sorts client-side anyway). */
function applyDeltas(
  prev: RulePositionRecord[],
  batch: TpslPositionDelta[],
): RulePositionRecord[] {
  const byId = new Map(prev.map((p) => [p.id, p]));
  for (const d of batch) {
    if (!d.position) continue;
    if (d.removed) byId.delete(d.position.id);
    else byId.set(d.position.id, d.position);
  }
  return Array.from(byId.values());
}

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
  strategy: 'tpsl1' | 'tpsl2' | 'swing_1',
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
        // Clear the spinner only when THIS request is still the latest one.
        // A superseded (aborted) request must not touch it: the old guard
        // (`!ctrl.signal.aborted`) skipped the clear entirely when a silent
        // reconcile/poll aborted the initial non-silent load, leaving `loading`
        // stuck true forever. The newest load — silent or not — owns the flag.
        if (inflight.current === ctrl) setLoading(false);
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

  // Primary path: patch the list in place from the backend's position deltas —
  // no refetch. Deltas for the selected rule are buffered and flushed together
  // (coalescing fill/exit bursts into one state update). Skipped once the rule is
  // settled (no more changes); the fallback poll below reconciles dropped frames.
  useEffect(() => {
    if (!selectedRuleId || settled) return;
    const pending: TpslPositionDelta[] = [];
    let timer: ReturnType<typeof setTimeout> | null = null;
    const flush = () => {
      timer = null;
      if (!pending.length) return;
      const batch = pending.splice(0);
      setPositions((prev) => applyDeltas(prev, batch));
    };
    const handle = connectTpslPositionsChanged(strategy, (delta) => {
      if (delta.ruleId !== selectedRuleId) return;
      pending.push(delta);
      if (!timer) timer = setTimeout(flush, REFRESH_DEBOUNCE_MS);
    });
    return () => {
      if (timer) clearTimeout(timer);
      handle.close();
    };
  }, [strategy, selectedRuleId, settled]);

  // Reconcile once when the rule transitions to settled: the terminal exit delta
  // can race the settle-driven unsubscribe above, so do one final silent fetch so
  // the table always lands on the terminal state. Fires once per settle, not on a
  // timer — cheap, and the only fetch this hook makes after the initial load.
  const wasSettled = useRef(settled);
  const reconcileRuleId = useRef<string | null>(null);
  useEffect(() => {
    // Only reconcile on an in-place transition to settled for the SAME rule —
    // not when the selection changes to an already-settled rule. The select
    // effect's initial fetch already covers a fresh selection; treating it as a
    // transition would fire a redundant silent load that aborts that initial
    // (non-silent) fetch. On selection change just (re)prime the baseline.
    if (selectedRuleId !== reconcileRuleId.current) {
      reconcileRuleId.current = selectedRuleId;
      wasSettled.current = settled;
      return;
    }
    if (settled && !wasSettled.current && selectedRuleId) void load(selectedRuleId, true);
    wasSettled.current = settled;
  }, [settled, selectedRuleId, load]);

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
