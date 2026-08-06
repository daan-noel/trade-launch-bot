/**
 * History cohort exit-reason filter — needles that match what live actually
 * persists on `strategy_positions.exit_reason`.
 *
 * System exits keep PascalCase labels (`TakeProfit`, …). Metric exits store
 * spaced `name op value` (`stall >= 300`, `trail >= 20`), so the filter offers
 * **metric names** as `contains` needles — never the retired ladder aliases
 * (`Trailing`, bare `Stall`, `TimeStop`, …).
 *
 * Exit-mix tiles also write synthetic cohort needles (`metric_win` /
 * `metric_loss` / `metric`) for the Metric± split — those are client-only
 * (not SQL `contains`) so they compose with chart `hfocus`.
 *
 * Do **not** run these needles through `normalizeExitReasonFilter`: that helper
 * maps `stall` → `Stall` for badge/search aliases and would break metric matching.
 */

import { isMetricExitReason } from 'lib/strategy/exitReason';

export type HistoryExitFilterKind = 'system' | 'metric';

/** Synthetic hexit values from the exit-mix Metric± tiles (not dropdown options). */
export const HISTORY_METRIC_EXIT_NEEDLES = [
  'metric_win',
  'metric_loss',
  'metric',
] as const;

export type HistoryMetricExitNeedle = (typeof HISTORY_METRIC_EXIT_NEEDLES)[number];

export function isHistoryMetricExitNeedle(
  needle: string | null | undefined,
): needle is HistoryMetricExitNeedle {
  return (
    needle === 'metric_win' || needle === 'metric_loss' || needle === 'metric'
  );
}

export type HistoryExitFilterOption = {
  /** Bound as `exit_reason contains` (ILIKE). */
  value: string;
  label: string;
  kind: HistoryExitFilterKind;
};

/** System reasons from `ExitReason::label` + Migrated. */
const SYSTEM_EXITS: readonly HistoryExitFilterOption[] = [
  { value: 'TakeProfit', label: 'TakeProfit', kind: 'system' },
  { value: 'StopLoss', label: 'StopLoss', kind: 'system' },
  { value: 'Dead', label: 'Dead', kind: 'system' },
  { value: 'Manual', label: 'Manual', kind: 'system' },
  { value: 'Migrated', label: 'Migrated', kind: 'system' },
];

/**
 * Common exit-side metric names (engine `MetricId::name`). One needle covers
 * every threshold for that metric (`stall >= 300`, `stall > 15`, …) and still
 * hits legacy PascalCase rows where the name is a substring (`TimeStop`,
 * `TrailingStop` via `trail`, `LiquidityExit`).
 */
const METRIC_EXITS: readonly HistoryExitFilterOption[] = [
  { value: 'stall', label: 'stall', kind: 'metric' },
  { value: 'trail', label: 'trail', kind: 'metric' },
  { value: 'retrace', label: 'retrace', kind: 'metric' },
  { value: 'pnl', label: 'pnl', kind: 'metric' },
  { value: 'held', label: 'held', kind: 'metric' },
  { value: 'bounce', label: 'bounce', kind: 'metric' },
  { value: 'time', label: 'time', kind: 'metric' },
  { value: 'liquidity', label: 'liquidity', kind: 'metric' },
  { value: 'rise', label: 'rise', kind: 'metric' },
];

/** Dropdown SSOT for Console History. */
export const HISTORY_EXIT_FILTER_OPTIONS: readonly HistoryExitFilterOption[] = [
  ...SYSTEM_EXITS,
  ...METRIC_EXITS,
];

/**
 * Old History URL / UI needles → canonical options. Kept so a bookmarked
 * `hexit=Trailing` still selects `trail` and matches metric rows.
 */
const LEGACY_HISTORY_EXIT_NEEDLES: Readonly<Record<string, string>> = {
  Trailing: 'trail',
  TrailingStop: 'trail',
  trailing: 'trail',
  Stall: 'stall',
  Time: 'time',
  TimeStop: 'time',
  Liquidity: 'liquidity',
  LiquidityExit: 'liquidity',
};

/** Map a raw `hexit` query value onto a dropdown option value when possible. */
export function canonicalizeHistoryExitFilter(
  raw: string | null | undefined,
): string | null {
  if (raw == null || raw === '') return null;
  return LEGACY_HISTORY_EXIT_NEEDLES[raw] ?? raw;
}

/** Text tone for a History exit filter selection. */
export function historyExitFilterToneClass(
  value: string | null | undefined,
): string {
  if (!value) return 'text-text-dim';
  if (value === 'metric_win') return 'text-green';
  if (value === 'metric_loss') return 'text-red';
  if (value === 'metric') return 'text-info';
  const opt = HISTORY_EXIT_FILTER_OPTIONS.find((o) => o.value === value);
  if (opt?.kind === 'metric') return 'text-info';
  switch (value) {
    case 'TakeProfit':
      return 'text-green';
    case 'StopLoss':
      return 'text-red';
    case 'Dead':
      return 'text-accent';
    case 'Manual':
    case 'Migrated':
      return 'text-secondary';
    default:
      // Exact metric-condition labels from the detailed exit mix.
      if (isMetricExitReason(value)) return 'text-info';
      return 'text-text-dim';
  }
}

/**
 * Mirror of the server `exit_reason contains` (case-insensitive substring),
 * plus the synthetic Metric± cohort needles (need `pnlSol` for win/loss).
 */
export function exitReasonMatchesFilter(
  exitReason: string | null | undefined,
  needle: string | null | undefined,
  pnlSol?: number | null,
): boolean {
  if (!needle) return true;
  if (needle === 'metric_win') {
    return isMetricExitReason(exitReason) && (pnlSol ?? 0) > 0;
  }
  if (needle === 'metric_loss') {
    return isMetricExitReason(exitReason) && (pnlSol ?? 0) <= 0;
  }
  if (needle === 'metric') {
    return isMetricExitReason(exitReason);
  }
  return (exitReason ?? '').toLowerCase().includes(needle.toLowerCase());
}

/**
 * Closes-series is `status = 'End'` only. Any other status filter means the
 * chart cohort is empty (table still pages the matching statuses via B1).
 */
export function seriesStatusAllowsCloses(
  status: string | null | undefined,
): boolean {
  return status == null || status === 'End';
}

/** One close point as far as cohort exit/status/window trimming cares. */
export type CohortClosePoint = {
  exit_time: string;
  exit_reason?: string | null;
  /** Required for synthetic Metric± needles; ignored for contains needles. */
  pnl_sol?: number;
};

/**
 * Client trim so the charts deck matches the History table cohort: custom
 * window + status + exit-reason contains (or Metric± synthetic needles).
 */
export function filterClosesForCohort<T extends CohortClosePoint>(
  closes: readonly T[],
  cohort: {
    fromIso: string | null;
    toIso: string | null;
    status: string | null;
    exitReason: string | null;
  },
): T[] {
  if (!seriesStatusAllowsCloses(cohort.status)) return [];
  const from = cohort.fromIso ? Date.parse(cohort.fromIso) : -Infinity;
  const to = cohort.toIso ? Date.parse(cohort.toIso) : Infinity;
  const needle = cohort.exitReason;
  return closes.filter((c) => {
    const t = Date.parse(c.exit_time);
    if (!(t >= from && t < to)) return false;
    return exitReasonMatchesFilter(c.exit_reason, needle, c.pnl_sol);
  });
}
