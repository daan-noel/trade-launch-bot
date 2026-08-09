/**
 * The **one cohort** behind the Console History section.
 *
 * Charts and table must never be computed from different queries — so the
 * filter bar, the charts deck, and the server-paged table all read this single
 * URL-backed cohort. It lives in the query string (the `h*` keys of
 * `OPS_PARAMS`) so a History deep-link from Portfolio lands on exactly the
 * cohort it promised, and a reload keeps it.
 *
 * `focus` is a drill-down lens on top of that cohort (chart cell → slice).
 * Timing charts keep the parent grid; equity / distribution / hold / rules and
 * the table + chip honor `focus`.
 */

import { useCallback, useMemo } from 'react';
import { useSearchParams } from 'react-router-dom';
import { OPS_PARAMS, type HistoryRange } from 'lib/strategy/nav';
import {
  parseHistoryFocus,
  serializeHistoryFocus,
  type HistoryFocus,
} from './historyFocus';
import { canonicalizeHistoryExitFilter } from './historyExitFilter';

export type HistoryMode = 'real' | 'paper' | 'all';

/** Realized-outcome lens from the summary strip's Win% / Worst% tiles. */
export type HistoryOutcome = 'win' | 'loss';

/**
 * Entered-position partition from the summary strip's Fired / Closed / Open
 * tiles. Deliberately its own channel rather than a value of `status`: these are
 * the aggregate's partitions (`fired` = entered at all, `open` = entered and not
 * ended), and `open` in particular spans several DB statuses — Holding,
 * ExitPending, ExitStuck, ExitUnconfirmed — so no single `status` string means it.
 */
export type HistoryLane = 'fired' | 'closed' | 'open';

export interface HistoryCohort {
  range: HistoryRange;
  /** Window start (ISO, UTC) — `null` for the all-time range. */
  fromIso: string | null;
  /** Window end (ISO, UTC, exclusive) — `null` for "up to now". */
  toIso: string | null;
  ruleId: string | null;
  mode: HistoryMode;
  /** Position status (`End` / `EntryFailed` / an open status); `null` = any. */
  status: string | null;
  exitReason: string | null;
  /** Summary Fired / Closed / Open tile; mutually exclusive with `status`. */
  lane: HistoryLane | null;
  /** Summary Win% / Worst% tile — realized SOL sign; `null` = both. */
  outcome: HistoryOutcome | null;
  /** Summary Migrated tile — graduated to AMM; `null` = don't care. */
  migrated: boolean | null;
  /** Chart drill-down — timing stays on parent; other charts + table follow. */
  focus: HistoryFocus | null;
  /** The `range` value the B2 series endpoint understands (it takes presets
   *  only; a custom window is served as `all` and trimmed client-side). */
  seriesRange: 'today' | '7d' | '30d' | 'all';
}

export interface HistoryCohortApi extends HistoryCohort {
  set: (patch: Partial<Omit<HistoryCohort, 'seriesRange'>>) => void;
  reset: () => void;
  /** True when anything narrows the cohort below "all trades, all time". */
  active: boolean;
}

const DEFAULT_RANGE: HistoryRange = '7d';

/** Preset → window start, evaluated against `now`. `all`/`custom` return null. */
function presetStart(range: HistoryRange, now: number): string | null {
  if (range === 'today') {
    const d = new Date(now);
    return new Date(Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate())).toISOString();
  }
  if (range === '7d') return new Date(now - 7 * 86_400_000).toISOString();
  if (range === '30d') return new Date(now - 30 * 86_400_000).toISOString();
  return null;
}

/**
 * Read + write the History cohort. `nowMs` is passed in (not read from the
 * clock inside the memo) so a preset window is stable across re-renders — a
 * moving `from` bound would refetch the table on every keystroke elsewhere on
 * the page.
 */
export function useHistoryCohort(nowMs: number): HistoryCohortApi {
  const [params, setParams] = useSearchParams();

  const rawRange = params.get(OPS_PARAMS.range);
  const range: HistoryRange =
    rawRange === 'today' ||
    rawRange === '7d' ||
    rawRange === '30d' ||
    rawRange === 'all' ||
    rawRange === 'custom'
      ? rawRange
      : DEFAULT_RANGE;
  const customFrom = params.get(OPS_PARAMS.from);
  const customTo = params.get(OPS_PARAMS.to);
  const ruleId = params.get(OPS_PARAMS.hRule);
  const rawMode = params.get(OPS_PARAMS.hMode);
  const mode: HistoryMode =
    rawMode === 'paper' || rawMode === 'all' || rawMode === 'real' ? rawMode : 'real';
  const status = params.get(OPS_PARAMS.hStatus);
  // Canonicalize legacy ladder needles (`Trailing` → `trail`) so the dropdown
  // selection and metric-label contains match stay aligned.
  const exitReason = canonicalizeHistoryExitFilter(params.get(OPS_PARAMS.hExit));
  const rawLane = params.get(OPS_PARAMS.hLane);
  const lane: HistoryLane | null =
    rawLane === 'fired' || rawLane === 'closed' || rawLane === 'open' ? rawLane : null;
  const rawOutcome = params.get(OPS_PARAMS.hOutcome);
  const outcome: HistoryOutcome | null =
    rawOutcome === 'win' || rawOutcome === 'loss' ? rawOutcome : null;
  const rawMigrated = params.get(OPS_PARAMS.hMigrated);
  // Tri-state, so `0` must stay distinct from absent: "not migrated" is a real
  // cohort, not "no migration filter".
  const migrated = rawMigrated === '1' ? true : rawMigrated === '0' ? false : null;
  const focusRaw = params.get(OPS_PARAMS.hFocus);

  const cohort = useMemo<HistoryCohort>(() => {
    const fromIso = range === 'custom' ? (customFrom || null) : presetStart(range, nowMs);
    const toIso = range === 'custom' ? (customTo || null) : null;
    return {
      range,
      fromIso,
      toIso,
      ruleId,
      mode,
      status,
      exitReason,
      lane,
      outcome,
      migrated,
      focus: parseHistoryFocus(focusRaw),
      seriesRange: range === 'custom' ? 'all' : range,
    };
  }, [
    range,
    customFrom,
    customTo,
    ruleId,
    mode,
    status,
    exitReason,
    lane,
    outcome,
    migrated,
    focusRaw,
    nowMs,
  ]);

  // Both writers take the functional form and patch `prev`: the Console has
  // three independent `useSearchParams` writers over one query string (this
  // cohort, the page's own position/mint deep link, the History scroll
  // cleanup), so a closed-over snapshot lets two writes in one tick drop each
  // other's keys. It also keeps `set`/`reset` referentially stable, so a
  // consumer can depend on them without re-running every render.
  const patchParams = useCallback(
    (mutate: (next: URLSearchParams) => void) => {
      setParams(
        (prev) => {
          const next = new URLSearchParams(prev);
          mutate(next);
          return next;
        },
        { replace: true },
      );
    },
    [setParams],
  );

  const set = useCallback(
    (patch: Partial<Omit<HistoryCohort, 'seriesRange'>>) => {
      patchParams((next) => {
        const put = (key: string, val: string | null | undefined) => {
          if (val == null || val === '') next.delete(key);
          else next.set(key, val);
        };
        if ('range' in patch) {
          put(OPS_PARAMS.range, patch.range === DEFAULT_RANGE ? null : patch.range);
          // Leaving custom drops the explicit bounds so they can't linger and
          // silently re-apply the moment the user picks custom again.
          if (patch.range !== 'custom') {
            next.delete(OPS_PARAMS.from);
            next.delete(OPS_PARAMS.to);
          }
        }
        if ('fromIso' in patch) put(OPS_PARAMS.from, patch.fromIso);
        if ('toIso' in patch) put(OPS_PARAMS.to, patch.toIso);
        if ('ruleId' in patch) put(OPS_PARAMS.hRule, patch.ruleId);
        if ('mode' in patch) put(OPS_PARAMS.hMode, patch.mode === 'real' ? null : patch.mode);
        if ('status' in patch) put(OPS_PARAMS.hStatus, patch.status);
        if ('exitReason' in patch) {
          put(OPS_PARAMS.hExit, canonicalizeHistoryExitFilter(patch.exitReason));
        }
        if ('lane' in patch) {
          put(OPS_PARAMS.hLane, patch.lane);
          // The bar's exact-status dropdown and the lane tiles both narrow by
          // status; letting both stand would silently intersect to an empty
          // cohort ("Open" ∩ "End"), which reads as "no trades" rather than as a
          // contradiction. Last one clicked wins.
          if (patch.lane) next.delete(OPS_PARAMS.hStatus);
        }
        if ('status' in patch && patch.status) next.delete(OPS_PARAMS.hLane);
        if ('outcome' in patch) put(OPS_PARAMS.hOutcome, patch.outcome);
        if ('migrated' in patch) {
          // Explicit `'0'`, not `put`'s empty-means-delete — `false` is a cohort.
          put(OPS_PARAMS.hMigrated, patch.migrated == null ? null : patch.migrated ? '1' : '0');
        }
        if ('focus' in patch) put(OPS_PARAMS.hFocus, serializeHistoryFocus(patch.focus));
      });
    },
    [patchParams],
  );

  const reset = useCallback(() => {
    patchParams((next) => {
      for (const key of [
        OPS_PARAMS.range,
        OPS_PARAMS.from,
        OPS_PARAMS.to,
        OPS_PARAMS.hRule,
        OPS_PARAMS.hMode,
        OPS_PARAMS.hStatus,
        OPS_PARAMS.hExit,
        OPS_PARAMS.hLane,
        OPS_PARAMS.hOutcome,
        OPS_PARAMS.hMigrated,
        OPS_PARAMS.hFocus,
      ]) {
        next.delete(key);
      }
    });
  }, [patchParams]);

  const active =
    range !== DEFAULT_RANGE ||
    ruleId != null ||
    mode !== 'real' ||
    status != null ||
    exitReason != null ||
    lane != null ||
    outcome != null ||
    migrated != null ||
    focusRaw != null;

  return { ...cohort, set, reset, active };
}
