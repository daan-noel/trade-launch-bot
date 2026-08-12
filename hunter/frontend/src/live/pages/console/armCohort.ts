/**
 * The **one cohort** behind the Console Arms section — the durable arm ledger's
 * date range, rule, mode and end-reason.
 *
 * It lives in the query string under its own `a*` keys (`OPS_PARAMS.aRange`, …)
 * rather than sharing History's `h*` set. The two sections describe different
 * populations — positions the bot took vs. every token it armed on — so a shared
 * window would silently re-scope a funnel the reviewer never touched, and the
 * two would still need separate reason/status channels anyway.
 *
 * `presetStart` is imported, not re-derived: "the last 7 days" has one definition
 * on this page.
 */

import { useCallback, useMemo } from 'react';
import { useSearchParams } from 'react-router-dom';
import { OPS_PARAMS, type HistoryRange } from 'lib/strategy/nav';
import { presetStart } from './historyCohort';

export type ArmMode = 'real' | 'paper' | 'all';

/** Every `end_reason` the backend writes, plus `waiting` for a live episode
 *  (whose `end_reason` is NULL — the server's filter column COALESCEs to this).
 *  Mirrors `ARM_END_REASONS` in `core/src/models/strategy_arm.rs`. */
export const ARM_REASONS = [
  'waiting',
  'entered',
  'dead',
  'migrated',
  'unsatisfiable',
  'paused',
  'duplicate_identity',
] as const;
export type ArmReason = (typeof ARM_REASONS)[number];

export interface ArmCohort {
  range: HistoryRange;
  /** Window start (ISO, UTC) — `null` for the all-time range. */
  fromIso: string | null;
  /** Window end (ISO, UTC, exclusive) — `null` for "up to now". */
  toIso: string | null;
  ruleId: string | null;
  mode: ArmMode;
  /** `null` = every ending, live episodes included. */
  reason: ArmReason | null;
}

export interface ArmCohortApi extends ArmCohort {
  set: (patch: Partial<ArmCohort>) => void;
  reset: () => void;
  /** True when anything narrows the cohort below "every arm, default window". */
  active: boolean;
}

const DEFAULT_RANGE: HistoryRange = 'today';

function isArmReason(v: string | null): v is ArmReason {
  return v != null && (ARM_REASONS as readonly string[]).includes(v);
}

/**
 * Read + write the Arms cohort. `nowMs` is passed in (not read inside the memo)
 * so a preset window is stable across re-renders — a moving `from` bound would
 * refetch the table on every unrelated keystroke on the page.
 *
 * The default window is **today**, not History's 7 days: arms outnumber
 * positions by orders of magnitude (an arm costs nothing on chain), so a week
 * is the wrong first read.
 */
export function useArmCohort(nowMs: number): ArmCohortApi {
  const [params, setParams] = useSearchParams();

  const rawRange = params.get(OPS_PARAMS.aRange);
  const range: HistoryRange =
    rawRange === 'today' ||
    rawRange === '7d' ||
    rawRange === '30d' ||
    rawRange === 'all' ||
    rawRange === 'custom'
      ? rawRange
      : DEFAULT_RANGE;
  const customFrom = params.get(OPS_PARAMS.aFrom);
  const customTo = params.get(OPS_PARAMS.aTo);
  const ruleId = params.get(OPS_PARAMS.aRule);
  const rawMode = params.get(OPS_PARAMS.aMode);
  const mode: ArmMode =
    rawMode === 'paper' || rawMode === 'all' || rawMode === 'real' ? rawMode : 'all';
  const rawReason = params.get(OPS_PARAMS.aReason);
  const reason = isArmReason(rawReason) ? rawReason : null;

  const cohort = useMemo<ArmCohort>(
    () => ({
      range,
      fromIso: range === 'custom' ? (customFrom || null) : presetStart(range, nowMs),
      toIso: range === 'custom' ? (customTo || null) : null,
      ruleId,
      mode,
      reason,
    }),
    [range, customFrom, customTo, ruleId, mode, reason, nowMs],
  );

  // Functional form, patching `prev`: the Console has several independent
  // `useSearchParams` writers over one query string, and a closed-over snapshot
  // lets two writes in one tick drop each other's keys.
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
    (patch: Partial<ArmCohort>) => {
      patchParams((next) => {
        const put = (key: string, val: string | null | undefined) => {
          if (val == null || val === '') next.delete(key);
          else next.set(key, val);
        };
        if ('range' in patch) {
          put(OPS_PARAMS.aRange, patch.range === DEFAULT_RANGE ? null : patch.range);
          // Leaving custom drops the explicit bounds so they can't linger and
          // silently re-apply the moment the user picks custom again.
          if (patch.range !== 'custom') {
            next.delete(OPS_PARAMS.aFrom);
            next.delete(OPS_PARAMS.aTo);
          }
        }
        if ('fromIso' in patch) put(OPS_PARAMS.aFrom, patch.fromIso);
        if ('toIso' in patch) put(OPS_PARAMS.aTo, patch.toIso);
        if ('ruleId' in patch) put(OPS_PARAMS.aRule, patch.ruleId);
        if ('mode' in patch) put(OPS_PARAMS.aMode, patch.mode === 'all' ? null : patch.mode);
        if ('reason' in patch) put(OPS_PARAMS.aReason, patch.reason);
      });
    },
    [patchParams],
  );

  const reset = useCallback(() => {
    patchParams((next) => {
      for (const key of [
        OPS_PARAMS.aRange,
        OPS_PARAMS.aFrom,
        OPS_PARAMS.aTo,
        OPS_PARAMS.aRule,
        OPS_PARAMS.aMode,
        OPS_PARAMS.aReason,
      ]) {
        next.delete(key);
      }
    });
  }, [patchParams]);

  const active =
    range !== DEFAULT_RANGE || ruleId != null || mode !== 'all' || reason != null;

  return { ...cohort, set, reset, active };
}

/** True when any `a*` key is present — the Arms section must open itself on a
 *  deep link, or the landing cohort would be invisible behind a collapsed
 *  header. */
export function hasArmCohortParams(params: URLSearchParams): boolean {
  return [
    OPS_PARAMS.aRange,
    OPS_PARAMS.aFrom,
    OPS_PARAMS.aTo,
    OPS_PARAMS.aRule,
    OPS_PARAMS.aMode,
    OPS_PARAMS.aReason,
  ].some((k) => params.has(k));
}
