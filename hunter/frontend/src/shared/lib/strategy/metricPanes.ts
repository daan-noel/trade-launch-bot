// Helpers for the lab metric-pane overlay: extract which metrics/windows a rule
// uses, evaluate conditions the same way the engine does (DNF: OR of AND arms +
// `=` bucket tolerance), and find first entry/exit fire indices for chart markers.

import type { ChartEventMarker } from 'components/token-price-chart';
import type { Condition, ConditionExpr } from './grammar';
import type { RuleParams, SideConditions } from './ruleParams';
import type { StrategyRegistry } from './registry';
import type { MetricSeriesColumn, MetricSeriesResponse, StrategyRule } from './types';
import { ruleParamsFromJson, sideInstances } from './ruleParams';
import { formatMetricExitLabel } from './exitReason';

const DEFAULT_WINDOWS = [10, 30, 60];

/** Stable key for a series column (metric, optionally @window). */
export function metricColKey(metric: string, window: number | null): string {
  return window == null ? metric : `${metric}@${window}`;
}

export interface RuleMetricPrefs {
  fingerprintId: string;
  /** Metric names the rule constrains (entry ∪ exit). */
  metrics: string[];
  /** Dynamic windows authored on the rule; falls back to defaults when empty. */
  windows: number[];
  /** Default pane keys to enable when the user hasn't picked any. */
  paneKeys: string[];
}

/** The two monotone clocks whose sampling density the backend can't infer from
 *  `windows` alone. Mirrors the Rust `sparse_grid_for`. */
export interface MetricClockHorizons {
  /** Largest `time` condition value (secs since creation), 0 when unconstrained. */
  timeHorizonSec: number;
  /** Largest `stall` condition value (secs since the last high), 0 when unconstrained. */
  stallHorizonSec: number;
}

/** Metric names whose clocks run on wall time rather than on trades, keyed to the
 *  sparse-grid horizon they size. `time` counts from creation, `stall` from the last
 *  all-time high — the backend cannot derive either from the requested windows. */
const CLOCK_METRICS = { time: 'timeHorizonSec', stall: 'stallHorizonSec' } as const;

/**
 * Largest authored condition value for each wall-clock metric — the horizons the
 * metric-series endpoint needs to keep its sparse tick grid dense far enough for a
 * `time`/`stall` crossing to land on a row.
 *
 * The backend defaults these to `0` (⇒ "not evaluated"), which only ever drops ticks
 * in quiet gaps past every other horizon. Passing the rule's real ceilings is what
 * makes a `stall > 120` marker land where the engine fires it. Mirrors the Rust
 * `sparse_grid_for` ceiling walk; the `=`-tolerance margin is added backend-side.
 */
export function metricClockHorizons(params: RuleParams): MetricClockHorizons {
  const out: MetricClockHorizons = { timeHorizonSec: 0, stallHorizonSec: 0 };
  for (const side of [params.entry, params.exit]) {
    if (!side) continue;
    for (const [, group] of sideInstances(side)) {
      for (const [metric, arms] of Object.entries(group.metrics)) {
        const key = CLOCK_METRICS[metric as keyof typeof CLOCK_METRICS];
        if (!key || !arms?.length) continue;
        for (const arm of arms) {
          for (const cond of arm) {
            if (Number.isFinite(cond.value)) out[key] = Math.max(out[key], cond.value);
          }
        }
      }
    }
  }
  return out;
}

/** Metrics + windows + default panes constrained by an (already-parsed) params
 *  form — the shared core of `extractRuleMetricPrefs`, also used for ad-hoc
 *  params without a saved rule (e.g. a sweep combo's blob). */
export function metricPrefsFromParams(
  params: RuleParams,
  registry: StrategyRegistry | undefined,
): Omit<RuleMetricPrefs, 'fingerprintId'> {
  const metrics = new Set<string>();
  const windows = new Set<number>();
  const paneKeys: string[] = [];

  for (const side of [params.entry, params.exit]) {
    if (!side) continue;
    for (const [groupName, group] of sideInstances(side)) {
      const kind = registry?.groups.find((g) => g.name === groupName)?.kind;
      const w =
        typeof group.strict.window_size_sec === 'number' && group.strict.window_size_sec > 0
          ? group.strict.window_size_sec
          : null;
      if (w != null) windows.add(w);
      for (const [metric, arms] of Object.entries(group.metrics)) {
        if (!arms?.length) continue;
        metrics.add(metric);
        const key = metricColKey(metric, kind === 'dynamic' ? w : null);
        if (!paneKeys.includes(key)) paneKeys.push(key);
      }
    }
  }

  return {
    metrics: [...metrics],
    windows: windows.size ? [...windows].sort((a, b) => a - b) : [...DEFAULT_WINDOWS],
    paneKeys,
  };
}

/** Pull metrics + windows (+ fingerprint) from a selected rule. */
export function extractRuleMetricPrefs(
  rule: StrategyRule,
  registry: StrategyRegistry | undefined,
): RuleMetricPrefs {
  const params = ruleParamsFromJson(rule.params, registry);
  return { fingerprintId: rule.fingerprint_id, ...metricPrefsFromParams(params, registry) };
}

/** Judge one condition — mirrors `hunter_engine::metrics::evaluator::eval_one`. */
export function evalMetricCondition(cond: Condition, value: number, eqTolerance: number): boolean {
  if (!Number.isFinite(value)) return false;
  switch (cond.operator) {
    case '>':
      return value > cond.value;
    case '>=':
      return value >= cond.value;
    case '<':
      return value < cond.value;
    case '<=':
      return value <= cond.value;
    case '=':
      return Math.abs(value - cond.value) <= eqTolerance / 2;
    case '!=':
      return Math.abs(value - cond.value) > eqTolerance / 2;
    default:
      return false;
  }
}

/** DNF: any OR arm whose conditions all hold (empty arms ⇒ true). */
export function evalMetricConditions(
  arms: ConditionExpr,
  value: number,
  eqTolerance: number,
): boolean {
  if (arms.length === 0) return true;
  return arms.some((arm) => arm.every((c) => evalMetricCondition(c, value, eqTolerance)));
}

function seriesLookup(series: MetricSeriesColumn[]): Map<string, MetricSeriesColumn> {
  const map = new Map<string, MetricSeriesColumn>();
  for (const s of series) map.set(metricColKey(s.metric, s.window_size_sec), s);
  return map;
}

/** Collect authored (group, metric, arms) rows for one side. */
function sideMetricRows(
  side: SideConditions | undefined,
  registry: StrategyRegistry | undefined,
): Array<{ groupName: string; metric: string; arms: ConditionExpr; dynamic: boolean; window: number | null }> {
  if (!side) return [];
  const out: Array<{
    groupName: string;
    metric: string;
    arms: ConditionExpr;
    dynamic: boolean;
    window: number | null;
  }> = [];
  for (const [groupName, group] of sideInstances(side)) {
    const gSpec = registry?.groups.find((g) => g.name === groupName);
    const w =
      typeof group.strict.window_size_sec === 'number' && group.strict.window_size_sec > 0
        ? group.strict.window_size_sec
        : null;
    for (const [metric, arms] of Object.entries(group.metrics)) {
      if (!arms?.length) continue;
      out.push({
        groupName,
        metric,
        arms,
        dynamic: gSpec?.kind === 'dynamic',
        window: w,
      });
    }
  }
  return out;
}

/** First condition on the first satisfied DNF arm — spaced `name op value` labels. */
function firstSatisfiedCondition(
  arms: ConditionExpr,
  value: number,
  eqTolerance: number,
): Condition | null {
  for (const arm of arms) {
    if (arm.length && arm.every((c) => evalMetricCondition(c, value, eqTolerance))) {
      return arm[0];
    }
  }
  return null;
}

/**
 * Side combinator mirrors the engine (`arm.rs`): entry ANDs across metrics;
 * exit ORs (any one satisfied metric fires). Within a metric, DNF still applies.
 */
function sidePassesAt(
  side: SideConditions | undefined,
  idx: number,
  byKey: Map<string, MetricSeriesColumn>,
  registry: StrategyRegistry | undefined,
  combinator: 'and' | 'or',
): boolean {
  const rows = sideMetricRows(side, registry);
  // Empty ⇒ vacuous entry-true / exit-false matching the engine.
  if (rows.length === 0) return combinator === 'and';
  return firstFiredMetricLabel(side, idx, byKey, registry, combinator) != null;
}

/**
 * Spaced label for the metric that justifies a side pass at `idx`:
 * `name op value` (e.g. `stall > 3`). Exit OR → first satisfied metric; entry
 * AND → first authored metric (all must hold). `null` when the side does not
 * pass (or is empty — callers that need vacuous-true use {@link sidePassesAt}).
 */
function firstFiredMetricLabel(
  side: SideConditions | undefined,
  idx: number,
  byKey: Map<string, MetricSeriesColumn>,
  registry: StrategyRegistry | undefined,
  combinator: 'and' | 'or',
): string | null {
  const rows = sideMetricRows(side, registry);
  if (rows.length === 0) return null;

  let andLabel: string | null = null;
  for (const row of rows) {
    const gSpec = registry?.groups.find((g) => g.name === row.groupName);
    const col = byKey.get(metricColKey(row.metric, row.dynamic ? row.window : null));
    const reading = col?.values[idx];
    const tol = gSpec?.metrics.find((m) => m.name === row.metric)?.eq_tolerance ?? 0;
    if (reading == null || !Number.isFinite(reading)) {
      if (combinator === 'and') return null;
      continue;
    }
    const cond = firstSatisfiedCondition(row.arms, reading, tol);
    if (cond == null) {
      if (combinator === 'and') return null;
      continue;
    }
    const label = formatMetricExitLabel(row.metric, cond.operator, cond.value);
    if (combinator === 'or') return label;
    if (andLabel == null) andLabel = label;
  }
  return combinator === 'and' ? andLabel : null;
}

/** Per-metric pass/fail at one series index (for the crosshair readout). */
export function metricConditionStatesAt(
  params: RuleParams,
  idx: number,
  data: MetricSeriesResponse,
  registry: StrategyRegistry | undefined,
): Array<{ side: 'entry' | 'exit'; metric: string; ok: boolean; value: number | null }> {
  const byKey = seriesLookup(data.series);
  const out: Array<{ side: 'entry' | 'exit'; metric: string; ok: boolean; value: number | null }> =
    [];
  for (const sideName of ['entry', 'exit'] as const) {
    const side = params[sideName];
    if (!side) continue;
    for (const [groupName, group] of sideInstances(side)) {
      const gSpec = registry?.groups.find((g) => g.name === groupName);
      const w =
        typeof group.strict.window_size_sec === 'number' && group.strict.window_size_sec > 0
          ? group.strict.window_size_sec
          : null;
      for (const [metric, arms] of Object.entries(group.metrics)) {
        if (!arms?.length) continue;
        const col = byKey.get(metricColKey(metric, gSpec?.kind === 'dynamic' ? w : null));
        const value = col?.values[idx] ?? null;
        const tol = gSpec?.metrics.find((m) => m.name === metric)?.eq_tolerance ?? 0;
        out.push({
          side: sideName,
          metric,
          value,
          ok: value != null && evalMetricConditions(arms, value, tol),
        });
      }
    }
  }
  return out;
}

/**
 * First trade index where entry may fire (entry AND holds and exit OR does not —
 * mirrors `CompiledRule::can_enter`); then first later exit.
 *
 * Markers are `role: 'signal'` with spaced `name op value` labels (e.g.
 * `stall > 3`) — the frontend condition-fire estimate, never the backend fill
 * pointers.
 */
export function findRuleFireMarkers(
  params: RuleParams,
  data: MetricSeriesResponse,
  registry: StrategyRegistry | undefined,
): ChartEventMarker[] {
  const n = data.at.length;
  if (n === 0) return [];
  const byKey = seriesLookup(data.series);
  const hasEntry = sideInstances(params.entry).some(([, g]) =>
    Object.values(g.metrics).some((c) => c?.length),
  );
  const hasExit = sideInstances(params.exit).some(([, g]) =>
    Object.values(g.metrics).some((c) => c?.length),
  );

  let entryIdx: number | null = null;
  let entryLabel: string | null = null;
  if (hasEntry) {
    for (let i = 0; i < n; i++) {
      const label = firstFiredMetricLabel(params.entry, i, byKey, registry, 'and');
      if (label == null) continue;
      // Engine refuses entry while exit metrics already hold.
      if (hasExit && sidePassesAt(params.exit, i, byKey, registry, 'or')) continue;
      entryIdx = i;
      entryLabel = label;
      break;
    }
  }

  let exitIdx: number | null = null;
  let exitLabel: string | null = null;
  if (hasExit) {
    const start = entryIdx != null ? entryIdx + 1 : 0;
    for (let i = start; i < n; i++) {
      const label = firstFiredMetricLabel(params.exit, i, byKey, registry, 'or');
      if (label != null) {
        exitIdx = i;
        exitLabel = label;
        break;
      }
    }
  }

  const markers: ChartEventMarker[] = [];
  if (entryIdx != null) {
    const price = data.price?.[entryIdx];
    if (price != null && Number.isFinite(price)) {
      markers.push({
        kind: 'entry',
        role: 'signal',
        time: data.at[entryIdx],
        priceInSol: price,
        label: entryLabel ?? 'entry',
      });
    }
  }
  if (exitIdx != null) {
    const price = data.price?.[exitIdx];
    if (price != null && Number.isFinite(price)) {
      markers.push({
        kind: 'exit',
        role: 'signal',
        time: data.at[exitIdx],
        priceInSol: price,
        label: exitLabel ?? 'exit',
      });
    }
  }
  return markers;
}

/** Nearest series index to a wall-clock unix second (or null). */
export function nearestSeriesIndex(atSec: number[], timeSec: number): number | null {
  if (!atSec.length) return null;
  let best = 0;
  let bestDist = Math.abs(atSec[0] - timeSec);
  for (let i = 1; i < atSec.length; i++) {
    const d = Math.abs(atSec[i] - timeSec);
    if (d < bestDist) {
      best = i;
      bestDist = d;
    }
  }
  return best;
}

export function parseSeriesAtSec(at: string[]): number[] {
  return at.map((s) => {
    const ms = Date.parse(s);
    return Number.isFinite(ms) ? ms / 1000 : NaN;
  });
}

export { DEFAULT_WINDOWS };
