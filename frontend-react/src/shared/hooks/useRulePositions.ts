import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { FALLBACK_POLL_INTERVAL_MS } from 'services/config';
import { connectTpslPositionsChanged } from 'services/sse';
import { useVisiblePolling } from './useVisiblePolling';
import type {
  PositionsSummary,
  RulePositionRecord,
  RulePositionsPage,
  RuleRecord,
  TpslPositionDelta,
} from 'types';

const MAX_SILENT_FAILURES = 3;
/** Coalesce a burst of position deltas into one in-place state update. */
const REFRESH_DEBOUNCE_MS = 200;

/** Apply a coalesced batch of deltas to the *current page*: upsert changed rows
 *  that are ALREADY on the page (by id), drop removed ones. With server-side
 *  pagination the page is a fixed window, so a delta for a row not on this page has
 *  nowhere to land — it's picked up by the page refetch / fallback poll instead.
 *  This keeps the visible rows live (fill→exit transitions) without corrupting the
 *  page window or double-counting; the summary panel (which needs the whole run) is
 *  refreshed separately from its own aggregate endpoint. */
function applyDeltas(
  prev: RulePositionRecord[],
  batch: TpslPositionDelta[],
): RulePositionRecord[] {
  const present = new Set(prev.map((p) => p.id));
  let changed = false;
  const byId = new Map(prev.map((p) => [p.id, p]));
  for (const d of batch) {
    if (!d.position || !present.has(d.position.id)) continue; // only patch visible rows
    if (d.removed) byId.delete(d.position.id);
    else byId.set(d.position.id, d.position);
    changed = true;
  }
  return changed ? Array.from(byId.values()) : prev;
}

/** A rule whose positions can no longer change: entries are off (`is_active`
 *  false) AND nothing is still draining. Active rules can open new positions;
 *  draining rules still have exits running, so both keep polling. */
function isSettled(rule: RuleRecord | undefined): boolean {
  return !!rule && !rule.is_active && rule.open_positions === 0;
}

export interface RulePositions {
  positions: RulePositionRecord[];
  /** Run/rule-wide total (from `X-Total-Count`) for the pager to size itself. */
  total: number;
  /** Run/rule-wide aggregates for the Positions Summary panel (null until loaded). */
  summary: PositionsSummary | null;
  loading: boolean;
  error: string | null;
}

/**
 * Owns the current *page* of the selected rule's positions plus the run-wide
 * summary. Fetches the page on select / page change (with a spinner), then keeps
 * the visible rows live from the backend's `tpsl_positions_changed` push
 * (in-place, only for rows on the page), with a slow visibility-gated poll as a
 * safety net. The **summary** is fetched from its own aggregate endpoint over the
 * whole run — independent of which page is shown — and refetched (debounced) on the
 * same delta signal so its counts stay correct. Polling/patching stop once the rule
 * is settled. Each fetch is abortable so switching rules/pages quickly can't let a
 * stale response overwrite a newer one.
 */
export function useRulePositions(
  selectedRuleId: string | null,
  rules: RuleRecord[],
  fetchPositions: (
    ruleId: string,
    opts: { limit: number; offset: number; signal?: AbortSignal },
  ) => Promise<RulePositionsPage>,
  fetchSummary: (ruleId: string, signal?: AbortSignal) => Promise<PositionsSummary>,
  strategy: 'tpsl1' | 'tpsl2' | 'swing_1',
  page: number,
  pageSize: number,
): RulePositions {
  const [positions, setPositions] = useState<RulePositionRecord[]>([]);
  const [total, setTotal] = useState(0);
  const [summary, setSummary] = useState<PositionsSummary | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const failures = useRef(0);
  // Latest in-flight page request; aborted when rule/page changes or we unmount.
  const inflight = useRef<AbortController | null>(null);
  const summaryInflight = useRef<AbortController | null>(null);

  const settled = useMemo(
    () => isSettled(rules.find((r) => r.id === selectedRuleId)),
    [rules, selectedRuleId],
  );

  const loadPage = useCallback(
    async (ruleId: string, silent: boolean) => {
      inflight.current?.abort();
      const ctrl = new AbortController();
      inflight.current = ctrl;
      if (!silent) setLoading(true);
      try {
        const { items, total } = await fetchPositions(ruleId, {
          limit: pageSize,
          offset: (page - 1) * pageSize,
          signal: ctrl.signal,
        });
        if (ctrl.signal.aborted) return;
        setPositions(items);
        setTotal(total);
        setError(null);
        failures.current = 0;
      } catch (e) {
        if (ctrl.signal.aborted || (e instanceof DOMException && e.name === 'AbortError')) return;
        const msg = e instanceof Error ? e.message : 'Failed to load positions';
        if (!silent || ++failures.current >= MAX_SILENT_FAILURES) setError(msg);
      } finally {
        // Clear the spinner only when THIS request is still the latest one — a
        // superseded (aborted) request must not touch it.
        if (inflight.current === ctrl) setLoading(false);
      }
    },
    [fetchPositions, page, pageSize],
  );

  const loadSummary = useCallback(
    async (ruleId: string) => {
      summaryInflight.current?.abort();
      const ctrl = new AbortController();
      summaryInflight.current = ctrl;
      try {
        const s = await fetchSummary(ruleId, ctrl.signal);
        if (!ctrl.signal.aborted) setSummary(s);
      } catch (e) {
        // Summary is non-blocking: a failure just leaves the last-known value
        // (or null). Swallow aborts; ignore transient errors (poll/delta retries).
        if (e instanceof DOMException && e.name === 'AbortError') return;
      }
    },
    [fetchSummary],
  );

  // Reset + non-silent fetch whenever the selection or the page window changes.
  useEffect(() => {
    failures.current = 0;
    if (!selectedRuleId) {
      inflight.current?.abort();
      summaryInflight.current?.abort();
      setPositions([]);
      setTotal(0);
      setSummary(null);
      setError(null);
      setLoading(false);
      return;
    }
    void loadPage(selectedRuleId, false);
    return () => inflight.current?.abort();
  }, [selectedRuleId, loadPage]);

  // Summary follows the selected rule (not the page) — refetch on rule change only.
  useEffect(() => {
    if (!selectedRuleId) return;
    void loadSummary(selectedRuleId);
    return () => summaryInflight.current?.abort();
  }, [selectedRuleId, loadSummary]);

  // Primary live path: patch visible rows from position deltas + refresh the
  // summary. Deltas for the selected rule are buffered and flushed together
  // (coalescing fill/exit bursts). Skipped once the rule is settled; the fallback
  // poll below reconciles dropped frames.
  useEffect(() => {
    if (!selectedRuleId || settled) return;
    const pending: TpslPositionDelta[] = [];
    let timer: ReturnType<typeof setTimeout> | null = null;
    const flush = () => {
      timer = null;
      if (!pending.length) return;
      const batch = pending.splice(0);
      setPositions((prev) => applyDeltas(prev, batch));
      // Any transition can change the run-wide aggregates — refresh them once per
      // coalesced burst (cheap COUNT/SUM, no rows shipped).
      void loadSummary(selectedRuleId);
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
  }, [strategy, selectedRuleId, settled, loadSummary]);

  // Reconcile once when the rule transitions to settled: the terminal exit delta
  // can race the settle-driven unsubscribe above, so do one final silent page +
  // summary fetch so both always land on the terminal state.
  const wasSettled = useRef(settled);
  const reconcileRuleId = useRef<string | null>(null);
  useEffect(() => {
    if (selectedRuleId !== reconcileRuleId.current) {
      reconcileRuleId.current = selectedRuleId;
      wasSettled.current = settled;
      return;
    }
    if (settled && !wasSettled.current && selectedRuleId) {
      void loadPage(selectedRuleId, true);
      void loadSummary(selectedRuleId);
    }
    wasSettled.current = settled;
  }, [settled, selectedRuleId, loadPage, loadSummary]);

  // Safety net: a slow visibility-gated poll catches dropped/lagged SSE frames.
  // No leading call (the effects above did the first fetch); off once settled.
  useVisiblePolling(
    () => {
      if (selectedRuleId) {
        void loadPage(selectedRuleId, true);
        void loadSummary(selectedRuleId);
      }
    },
    FALLBACK_POLL_INTERVAL_MS,
    !!selectedRuleId && !settled,
    false,
  );

  return { positions, total, summary, loading, error };
}
