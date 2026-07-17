// Helpers for the lab metric-pane overlay: extract which metrics/windows a rule
// uses, evaluate conditions the same way the engine does (AND + `=` bucket
// tolerance), and find first entry/exit fire indices for chart markers.

import type { ChartEventMarker } from 'components/token-price-chart';
import type { Condition } from './grammar';
import type { RuleParams, SideConditions } from './ruleParams';
import type { StrategyRegistry } from './registry';
import type { MetricSeriesColumn, MetricSeriesResponse, StrategyRule } from './types';
import { ruleParamsFromJson } from './ruleParams';

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

/** Pull metrics + windows (+ fingerprint) from a selected rule. */
export function extractRuleMetricPrefs(
  rule: StrategyRule,
  registry: StrategyRegistry | undefined,
): RuleMetricPrefs {
  const params = ruleParamsFromJson(rule.params, registry);
  const metrics = new Set<string>();
  const windows = new Set<number>();
  const paneKeys: string[] = [];

  for (const side of [params.entry, params.exit]) {
    if (!side) continue;
    for (const [groupName, group] of Object.entries(side)) {
      const kind = registry?.groups.find((g) => g.name === groupName)?.kind;
      const w =
        typeof group.strict.window_size_sec === 'number' && group.strict.window_size_sec > 0
          ? group.strict.window_size_sec
          : null;
      if (w != null) windows.add(w);
      for (const [metric, conds] of Object.entries(group.metrics)) {
        if (!conds?.length) continue;
        metrics.add(metric);
        const key = metricColKey(metric, kind === 'dynamic' ? w : null);
        if (!paneKeys.includes(key)) paneKeys.push(key);
      }
    }
  }

  return {
    fingerprintId: rule.fingerprint_id,
    metrics: [...metrics],
    windows: windows.size ? [...windows].sort((a, b) => a - b) : [...DEFAULT_WINDOWS],
    paneKeys,
  };
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

/** All conditions on a metric AND together (empty ⇒ true). */
export function evalMetricConditions(
  conds: Condition[],
  value: number,
  eqTolerance: number,
): boolean {
  return conds.every((c) => evalMetricCondition(c, value, eqTolerance));
}

function seriesLookup(series: MetricSeriesColumn[]): Map<string, MetricSeriesColumn> {
  const map = new Map<string, MetricSeriesColumn>();
  for (const s of series) map.set(metricColKey(s.metric, s.window_size_sec), s);
  return map;
}

function sidePassesAt(
  side: SideConditions | undefined,
  idx: number,
  byKey: Map<string, MetricSeriesColumn>,
  registry: StrategyRegistry | undefined,
): boolean {
  if (!side) return true;
  const groups = Object.entries(side);
  if (groups.length === 0) return true;
  for (const [groupName, group] of groups) {
    const gSpec = registry?.groups.find((g) => g.name === groupName);
    const w =
      typeof group.strict.window_size_sec === 'number' && group.strict.window_size_sec > 0
        ? group.strict.window_size_sec
        : null;
    const metricEntries = Object.entries(group.metrics).filter(([, c]) => c?.length);
    if (metricEntries.length === 0) continue;
    for (const [metric, conds] of metricEntries) {
      const col = byKey.get(metricColKey(metric, gSpec?.kind === 'dynamic' ? w : null));
      const value = col?.values[idx];
      const tol = gSpec?.metrics.find((m) => m.name === metric)?.eq_tolerance ?? 0;
      if (value == null || !evalMetricConditions(conds, value, tol)) return false;
    }
  }
  return true;
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
    for (const [groupName, group] of Object.entries(side)) {
      const gSpec = registry?.groups.find((g) => g.name === groupName);
      const w =
        typeof group.strict.window_size_sec === 'number' && group.strict.window_size_sec > 0
          ? group.strict.window_size_sec
          : null;
      for (const [metric, conds] of Object.entries(group.metrics)) {
        if (!conds?.length) continue;
        const col = byKey.get(metricColKey(metric, gSpec?.kind === 'dynamic' ? w : null));
        const value = col?.values[idx] ?? null;
        const tol = gSpec?.metrics.find((m) => m.name === metric)?.eq_tolerance ?? 0;
        out.push({
          side: sideName,
          metric,
          value,
          ok: value != null && evalMetricConditions(conds, value, tol),
        });
      }
    }
  }
  return out;
}

/** First trade index where all entry metric conditions hold; then first later exit. */
export function findRuleFireMarkers(
  params: RuleParams,
  data: MetricSeriesResponse,
  registry: StrategyRegistry | undefined,
): ChartEventMarker[] {
  const n = data.at.length;
  if (n === 0) return [];
  const byKey = seriesLookup(data.series);
  const hasEntry = Object.values(params.entry ?? {}).some((g) =>
    Object.values(g.metrics).some((c) => c?.length),
  );
  const hasExit = Object.values(params.exit ?? {}).some((g) =>
    Object.values(g.metrics).some((c) => c?.length),
  );

  let entryIdx: number | null = null;
  if (hasEntry) {
    for (let i = 0; i < n; i++) {
      if (sidePassesAt(params.entry, i, byKey, registry)) {
        entryIdx = i;
        break;
      }
    }
  }

  let exitIdx: number | null = null;
  if (hasExit) {
    const start = entryIdx != null ? entryIdx + 1 : 0;
    for (let i = start; i < n; i++) {
      if (sidePassesAt(params.exit, i, byKey, registry)) {
        exitIdx = i;
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
        time: data.at[entryIdx],
        priceInSol: price,
        label: 'Entry · metrics',
      });
    }
  }
  if (exitIdx != null) {
    const price = data.price?.[exitIdx];
    if (price != null && Number.isFinite(price)) {
      markers.push({
        kind: 'exit',
        time: data.at[exitIdx],
        priceInSol: price,
        label: 'Exit · metrics',
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
