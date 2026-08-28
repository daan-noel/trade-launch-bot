// The strategy **registry** — the frontend mirror of the backend's
// `GET /api/meta/strategy-registry` payload (`hunter_engine::metrics::registry_json`).
// One fetch drives the whole rule-authoring UI: group pickers, metric rows,
// operator lists, sweep axes, chart pane picker, monitor columns, and validation
// messages. Adding a metric in Rust ⇒ it appears everywhere on the next load with
// zero frontend work (extensibility contract, backend plan §8 / FE plan §1).

import { REGISTRY_STALE_SECS, useGetStrategyRegistryQuery } from 'store/sharedEndpoints';
import type { CompareOp } from 'components/table/numericFilter';

/** The six comparison operators, shared with the condition grammar. */
export type Operator = CompareOp;

/** A metric's (and its condition values') unit. */
export type MetricUnit = 'seconds' | 'sol' | 'percent' | 'count';

/** Whether a group's metrics are rule-independent (`static`) or need per-rule
 *  strict params like `window_size_sec` (`dynamic`). */
export type MetricGroupKind = 'static' | 'dynamic';

/** What a group's metric state anchors on. `token` (default) = one value per token;
 *  `position` = anchored on your entry fill, so it only exists while holding —
 *  **exit-only** (the backend rejects it under `entry`; the builder hides it there). */
export type MetricScope = 'token' | 'position';

/** Which metrics a group is expected to *interact* with — the grouping the lab's
 *  metric-combo discovery pipeline grids over (backend `MetricFamily`). Mirrors the
 *  hue families the registry already keeps, so it also reads as "these chips are
 *  siblings". `standalone` = a group belonging to no established family. */
export type MetricFamily = 'price' | 'flow' | 'flow_ix' | 'state' | 'standalone';

/** A group's strict (non-condition) parameter, e.g. `window_size_sec`. */
export interface StrictParamSpec {
  name: string;
  required: boolean;
  /** Whether `0` is a legal value of this param's domain (`>= 0` instead of `> 0`).
   *  Mirrors the Rust `StrictParamSpec.allows_zero`; optional so a pre-flag registry
   *  payload still parses (absent ⇒ the old `> 0` rule). NOT a zero-as-unbound
   *  sentinel — an absent optional param is what means "off". */
  allows_zero?: boolean;
}

/** Fingerprint-side config field for a group (e.g. `ix_patterns`). */
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
  /** THE definition of the metric, authored on the backend `MetricSpec` and rendered
   *  straight into the tooltip. Optional only so a pre-description registry payload
   *  still parses; when it is present it wins over any frontend copy. */
  description?: string;
  unit: MetricUnit;
  eq_tolerance: number;
  /** Whether this metric reads its group's SECOND window axis (`slice_size_*`).
   *  `m_flow_window` declares that axis for every instance but only these metrics may
   *  set it, so the editor asks per METRIC and never by group name. Optional so a
   *  registry payload from before the axis moved still parses; absent reads false. */
  two_window?: boolean;
  monotonic: boolean;
  /** HSL hue `[0, 359]` — group siblings share a nearby family. */
  hue: number;
}

/** One metric group (one JSON key under `entry`/`exit`). */
export interface GroupSpec {
  name: string;
  kind: MetricGroupKind;
  /** Token-scoped (default) or position-scoped (`m_position`, exit-only). Optional
   *  so a pre-scope registry payload still parses. */
  scope?: MetricScope;
  /** Interaction family. Optional so a pre-family registry payload still parses. */
  family?: MetricFamily;
  strict_params: StrictParamSpec[];
  /** Fingerprint-level config fields (empty / omitted for most groups). */
  fingerprint_config?: FpConfigFieldSpec[];
  metrics: MetricSpec[];
}

/** Groups that declare fingerprint-side config (drives FingerprintForm sections). */
export function groupsWithFingerprintConfig(reg: StrategyRegistry | undefined): GroupSpec[] {
  return (reg?.groups ?? []).filter((g) => (g.fingerprint_config?.length ?? 0) > 0);
}

/** Read `m_flow_ix.ix_patterns` from a fingerprint's `metric_config`. */
export function ixPatternsFromConfig(
  cfg: Record<string, unknown> | null | undefined,
): string[][] {
  const flow = cfg?.m_flow_ix;
  if (!flow || typeof flow !== 'object') return [];
  const pats = (flow as { ix_patterns?: unknown }).ix_patterns;
  if (!Array.isArray(pats)) return [];
  return pats.filter(
    (p): p is string[] => Array.isArray(p) && p.every((x) => typeof x === 'string'),
  );
}

/** The `m_flow_ix` wallet rules, as booleans, defaulting the way the BACKEND defaults
 *  them: absent means `true` (`FlowPatterns::default`).
 *
 *  Reading absent as `false` shows a control saying the opposite of what the engine
 *  does — and these two decide what the classifier measures, not how tightly.
 */
export function flowWalletRules(cfg: Record<string, unknown> | null | undefined): {
  wallet_contagion: boolean;
  creator_is_tagged: boolean;
} {
  const flow = cfg?.m_flow_ix;
  const obj =
    flow && typeof flow === 'object' && !Array.isArray(flow)
      ? (flow as Record<string, unknown>)
      : {};
  const read = (k: string) => (typeof obj[k] === 'boolean' ? (obj[k] as boolean) : true);
  return { wallet_contagion: read('wallet_contagion'), creator_is_tagged: read('creator_is_tagged') };
}

/** Write the two wallet rules into a config produced by {@link metricConfigWithIxPatterns}.
 *
 *  Always EXPLICIT, never left to the backend default: a row that omits them says
 *  nothing about which classifier it meant, which is how they came to be reverted
 *  unnoticed. No `m_flow_ix` group (no patterns) ⇒ nothing to attach them to.
 */
export function withFlowWalletRules(
  cfg: Record<string, unknown>,
  rules: { wallet_contagion: boolean; creator_is_tagged: boolean },
): Record<string, unknown> {
  const flow = cfg.m_flow_ix as Record<string, unknown> | undefined;
  return flow ? { ...cfg, m_flow_ix: { ...flow, ...rules } } : cfg;
}

/** Write flow patterns into `prev`, an existing `metric_config`, and return the WHOLE
 *  config — everything else preserved.
 *
 *  A PUT replaces the row, so this is the only safe way to write the key. Two levels
 *  have to survive and both used to be lost:
 *
 *  * the other GROUPS — `prev` is the base rather than the result;
 *  * the other `m_flow_ix` KEYS — `wallet_contagion`, `creator_is_tagged`, and the
 *    marker masks are carried across one level down.
 *
 *  Rebuilding `m_flow_ix` from patterns alone reads as a partial write and lands as a
 *  full one: it reverted both wallet rules to their `true` defaults, silently, on any
 *  save that touched the fingerprint at all. Those defaults are a DIFFERENT
 *  classifier — contagion makes a tag a property of the sender's history instead of
 *  the transaction — so the fingerprint stopped measuring what its rule was derived on.
 *
 *  No patterns ⇒ the `m_flow_ix` group is dropped (unconfigured ⇒ every flow metric
 *  reads NaN), which is what an empty editor means.
 */
export function metricConfigWithIxPatterns(
  patterns: string[][],
  prev: Record<string, unknown> = {},
): Record<string, unknown> {
  const cleaned = patterns.map((p) => p.map((s) => s.trim()).filter(Boolean)).filter((p) => p.length > 0);
  const { m_flow_ix: prevFlow, ...otherGroups } = prev;
  if (cleaned.length === 0) return otherGroups;
  const keep: Record<string, unknown> = {};
  if (prevFlow && typeof prevFlow === 'object' && !Array.isArray(prevFlow)) {
    for (const [k, v] of Object.entries(prevFlow as Record<string, unknown>)) {
      if (k !== 'ix_patterns') keep[k] = v;
    }
  }
  return { ...otherGroups, m_flow_ix: { ...keep, ix_patterns: cleaned } };
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
    // A tally is bare: "12 wallets" already says what it is, and any glyph here would
    // read as a quantity of something else.
    case 'count':
      return '';
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
 *
 * The tab, however, outlives the backend process. `refetchOnMountOrArgChange`
 * overrides the app-wide `false` so a copy older than `REGISTRY_STALE_SECS` is
 * re-read on the next mount — otherwise a restart that adds a metric group leaves
 * the pickers silently rendering the previous vocabulary for a full hour.
 */
export function useStrategyRegistry() {
  return useGetStrategyRegistryQuery(undefined, {
    refetchOnMountOrArgChange: REGISTRY_STALE_SECS,
  });
}
