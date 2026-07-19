import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { FALLBACK_POLL_INTERVAL_MS } from 'services/config';
import { connectTpslPositionsChanged } from 'services/sse';
import { useVisiblePolling } from './useVisiblePolling';
import { toSummaryBody, toTableRequest, type TableRequestBody } from 'services/tableRequest';
import type { TableQuery } from 'components/table/types';
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
 *  page window or double-counting; the summary panel is refreshed separately from its
 *  own aggregate endpoint over the current filter cohort. */
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

/** Initial view-state for a positions table: page 1, default size, no sort/filter.
 *  Pages seed their `posQuery` state with this, then replace it wholesale from the
 *  `DataTable`'s `onQueryChange`. */
export const DEFAULT_POSITIONS_QUERY: TableQuery = {
  page: 1,
  pageSize: 20,
  sortKeys: [],
  search: '',
  colFilters: {},
};

export interface RulePositions {
  positions: RulePositionRecord[];
  /** Run/rule-wide total (from `X-Total-Count`) for the pager to size itself. */
  total: number;
  /** Filtered-population aggregates for the Positions Summary panel (null until loaded). */
  summary: PositionsSummary | null;
  loading: boolean;
  error: string | null;
}

/**
 * Owns the current *page* of the selected rule's positions plus the filtered
 * summary. Fetches the page on select / page change (with a spinner), then keeps
 * the visible rows live from the backend's `tpsl_positions_changed` push
 * (in-place, only for rows on the page), with a slow visibility-gated poll as a
 * safety net. The **summary** is fetched from its own aggregate endpoint over the
 * table's current search/filters (mint-set included) — independent of which page
 * is shown — and refetched (debounced) on the same delta signal so its counts stay
 * correct while a run is live. Polling/patching stop once the rule is settled. Each
 * fetch is abortable so switching rules/pages quickly can't let a stale response
 * overwrite a newer one.
 */
export function useRulePositions(
  selectedRuleId: string | null,
  rules: RuleRecord[],
  fetchPositions: (
    ruleId: string,
    body: TableRequestBody,
    signal?: AbortSignal,
  ) => Promise<RulePositionsPage>,
  fetchSummary: (
    ruleId: string,
    body: TableRequestBody,
    signal?: AbortSignal,
  ) => Promise<PositionsSummary>,
  strategy: 'tpsl1' | 'tpsl2',
  query: TableQuery,
  /** Column keys that filter numerically (so `>5`/`1..10` become structured ops).
   *  Derive with `numericColKeys(columns)`; empty = every filter is substring. */
  numericCols?: ReadonlySet<string>,
  /** When false, this is a *static* (immutable) view — the run-history ("old runs")
   *  population never changes, so skip the SSE delta patching, the fallback poll, and
   *  the settle reconcile. Only the fetch-on-select/query path runs. Default true. */
  live: boolean = true,
): RulePositions {
  const numericKey = numericCols ? [...numericCols].sort().join(',') : '';
  const filterKey = useMemo(
    () =>
      JSON.stringify({
        search: query.search,
        colFilters: query.colFilters,
        structuredFilters: query.structuredFilters,
      }),
    [query.search, query.colFilters, query.structuredFilters],
  );
  const body = useMemo(
    () => toTableRequest(query, numericCols ?? new Set()),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [query.page, query.pageSize, query.search, JSON.stringify(query.sortKeys), filterKey, numericKey],
  );
  const summaryBody = useMemo(
    () => toSummaryBody(query, numericCols ?? new Set()),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [filterKey, numericKey],
  );
  const summaryKey = JSON.stringify(summaryBody);
  const [positions, setPositions] = useState<RulePositionRecord[]>([]);
  const [total, setTotal] = useState(0);
  const [summary, setSummary] = useState<PositionsSummary | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const failures = useRef(0);
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
        const { items, total } = await fetchPositions(ruleId, body, ctrl.signal);
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
        if (inflight.current === ctrl) setLoading(false);
      }
    },
    [fetchPositions, body],
  );

  const loadSummary = useCallback(
    async (ruleId: string) => {
      summaryInflight.current?.abort();
      const ctrl = new AbortController();
      summaryInflight.current = ctrl;
      try {
        const s = await fetchSummary(ruleId, summaryBody, ctrl.signal);
        if (!ctrl.signal.aborted) setSummary(s);
      } catch (e) {
        if (e instanceof DOMException && e.name === 'AbortError') return;
      }
    },
    [fetchSummary, summaryBody],
  );

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

  useEffect(() => {
    if (!selectedRuleId) return;
    void loadSummary(selectedRuleId);
    return () => summaryInflight.current?.abort();
  }, [selectedRuleId, summaryKey, loadSummary]);

  useEffect(() => {
    if (!selectedRuleId || settled || !live) return;
    const pending: TpslPositionDelta[] = [];
    let timer: ReturnType<typeof setTimeout> | null = null;
    const flush = () => {
      timer = null;
      if (!pending.length) return;
      const batch = pending.splice(0);
      setPositions((prev) => applyDeltas(prev, batch));
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
  }, [strategy, selectedRuleId, settled, live, loadSummary]);

  const wasSettled = useRef(settled);
  const reconcileRuleId = useRef<string | null>(null);
  useEffect(() => {
    if (selectedRuleId !== reconcileRuleId.current) {
      reconcileRuleId.current = selectedRuleId;
      wasSettled.current = settled;
      return;
    }
    if (live && settled && !wasSettled.current && selectedRuleId) {
      void loadPage(selectedRuleId, true);
      void loadSummary(selectedRuleId);
    }
    wasSettled.current = settled;
  }, [settled, selectedRuleId, live, loadPage, loadSummary]);

  useVisiblePolling(
    () => {
      if (selectedRuleId) {
        void loadPage(selectedRuleId, true);
        void loadSummary(selectedRuleId);
      }
    },
    FALLBACK_POLL_INTERVAL_MS,
    !!selectedRuleId && !settled && live,
    false,
  );

  return { positions, total, summary, loading, error };
}
