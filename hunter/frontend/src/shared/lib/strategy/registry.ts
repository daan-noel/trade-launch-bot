// The strategy **registry** — the frontend mirror of the backend's
// `GET /api/meta/strategy-registry` payload (`hunter_engine::metrics::registry_json`).
// One fetch drives the whole rule-authoring UI: group pickers, metric rows,
// operator lists, sweep axes, chart pane picker, monitor columns, and validation
// messages. Adding a metric in Rust ⇒ it appears everywhere on the next load with
// zero frontend work (extensibility contract, backend plan §8 / FE plan §1).

import { useGetStrategyRegistryQuery } from 'store/sharedEndpoints';
import type { CompareOp } from 'components/table/numericFilter';

/** The six comparison operators, shared with the condition grammar. */
export type Operator = CompareOp;

/** A metric's (and its condition values') unit. */
export type MetricUnit = 'seconds' | 'sol' | 'percent';

/** Whether a group's metrics are rule-independent (`static`) or need per-rule
 *  strict params like `window_size_sec` (`dynamic`). */
export type MetricGroupKind = 'static' | 'dynamic';

/** A group's strict (non-condition) parameter, e.g. `window_size_sec`. */
export interface StrictParamSpec {
  name: string;
  required: boolean;
}

/** Fingerprint-side config field for a group (e.g. `volume_ix_patterns`). */
export interface FpConfigFieldSpec {
  name: string;
  /** Wire type hint — currently `"string[][]"` for volume-ix patterns. */
  value_type: string;
  required: boolean;
}

/** One metric within a group. `eq_tolerance` is its own `=`/`!=` bucket width.
 *  `hue` is the backend SSOT UI color (HSL degrees); the FE applies a fixed
 *  per-operator shade on top (see `metricColors.ts`). */
export interface MetricSpec {
  name: string;
  unit: MetricUnit;
  eq_tolerance: number;
  monotonic: boolean;
  /** HSL hue `[0, 359]` — group siblings share a nearby family. */
  hue: number;
}

/** One metric group (one JSON key under `entry`/`exit`). */
export interface GroupSpec {
  name: string;
  kind: MetricGroupKind;
  strict_params: StrictParamSpec[];
  /** Fingerprint-level config fields (empty / omitted for most groups). */
  fingerprint_config?: FpConfigFieldSpec[];
  metrics: MetricSpec[];
}

/** Groups that declare fingerprint-side config (drives FingerprintForm sections). */
export function groupsWithFingerprintConfig(reg: StrategyRegistry | undefined): GroupSpec[] {
  return (reg?.groups ?? []).filter((g) => (g.fingerprint_config?.length ?? 0) > 0);
}

/** Read `m_flow_split.volume_ix_patterns` from a fingerprint's `metric_config`. */
export function volumeIxPatternsFromConfig(
  cfg: Record<string, unknown> | null | undefined,
): string[][] {
  const flow = cfg?.m_flow_split;
  if (!flow || typeof flow !== 'object') return [];
  const pats = (flow as { volume_ix_patterns?: unknown }).volume_ix_patterns;
  if (!Array.isArray(pats)) return [];
  return pats.filter(
    (p): p is string[] => Array.isArray(p) && p.every((x) => typeof x === 'string'),
  );
}

/** Build `metric_config` for flow patterns. Empty ⇒ `{}` (unconfigured). */
export function metricConfigWithVolumePatterns(patterns: string[][]): Record<string, unknown> {
  const cleaned = patterns.map((p) => p.map((s) => s.trim()).filter(Boolean)).filter((p) => p.length > 0);
  if (cleaned.length === 0) return {};
  return { m_flow_split: { volume_ix_patterns: cleaned } };
}

/** The whole registry payload. */
export interface StrategyRegistry {
  operators: Operator[];
  groups: GroupSpec[];
}

/** Short unit suffix for labels/hints (`◎` for SOL, `s`, `%`). */
export function unitSuffix(unit: MetricUnit): string {
  switch (unit) {
    case 'seconds':
      return 's';
    case 'sol':
      return '◎';
    case 'percent':
      return '%';
  }
}

/** Find a group by name (registry lookups are small, linear is fine). */
export function findGroup(reg: StrategyRegistry | undefined, group: string): GroupSpec | undefined {
  return reg?.groups.find((g) => g.name === group);
}

/** Find a metric spec by group + metric name. */
export function findMetric(
  reg: StrategyRegistry | undefined,
  group: string,
  metric: string,
): MetricSpec | undefined {
  return findGroup(reg, group)?.metrics.find((m) => m.name === metric);
}

/**
 * Cached, app-wide access to the registry. The payload is static for the backend
 * process lifetime, so it is fetched once and held for the session (see the long
 * `keepUnusedDataFor` on the endpoint). Every rule-authoring surface reads it.
 */
export function useStrategyRegistry() {
  return useGetStrategyRegistryQuery();
}
