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

/** Fingerprint-side config field for a group (e.g. `ix_patterns`).
 *
 *  A group declares EVERY key it reads out of `metric_config`, so the editors are
 *  generated from this rather than hardcoded — an undeclared key is a setting with no
 *  control, writable only by hand-posting JSON and then overwritten by the next save. */
export interface FpConfigFieldSpec {
  name: string;
  /** Wire type hint: `"string[][]"` (ordered label sequences), `"marker[]"` (a subset
   *  of {@link StrategyRegistry.ix_markers}), or `"bool"`. */
  value_type: string;
  required: boolean;
  /** THE definition of the field, authored on the backend `FpConfigFieldSpec` and
   *  rendered straight into the tooltip. Optional only so a pre-description payload
   *  still parses. */
  description?: string;
  /** The value the group assumes when the key is ABSENT, as the engine spells it.
   *  Load-bearing for the booleans: their default is `true`, so a control that renders
   *  an absent field as unchecked says the opposite of what the classifier does. */
  default?: unknown;
  /** Fields naming the OPPOSITE side of the same split — configuring both is rejected
   *  at save, so the editor disables them against each other instead. */
  conflicts_with?: string[];
}

/** One structural marker in the engine's vocabulary (`flow_ix::MARKERS`), served with
 *  the registry so a marker added in Rust reaches the picker with no frontend change.
 *  `router` splits the two kinds: what the transaction DOES, versus the retail
 *  front-end a person clicked through. */
export interface IxMarkerSpec {
  name: string;
  router: boolean;
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

/** Which side of the split a marker mask names.
 *
 *  Not a convenience flag - the two are opposite claims. "Carries a throwaway account"
 *  identifies machines and leaves everything else unjudged; "came through a named
 *  router" identifies people and judges everything else a machine. */
export type MarkerSide = 'tagged' | 'untagged';

/** The `m_flow_ix` classifier a fingerprint configures, as the editor holds it - the
 *  frontend mirror of the engine's `FlowPatterns`.
 *
 *  ONE model for the whole key, because it is written as a whole: a PUT replaces the
 *  row, so any writer that rebuilds `m_flow_ix` from a subset of its fields lands as a
 *  full write and silently drops the rest. */
export interface FlowClassifier {
  /** Whether the fingerprint configures `m_flow_ix` at all. `false` means the key is
   *  absent and every flow metric reads NaN - a different state from a classifier that
   *  tags nothing. */
  configured: boolean;
  /** Exact ordered label sequences that TAG a trade. */
  ix_patterns: string[][];
  /** Structural marker names, from {@link StrategyRegistry.ix_markers}. */
  markers: string[];
  /** Which side {@link FlowClassifier.markers} names. */
  markers_side: MarkerSide;
  wallet_contagion: boolean;
  creator_is_tagged: boolean;
}

/** The key each marker side is stored under. */
const MARKER_KEY: Record<MarkerSide, string> = {
  tagged: 'tagged_ix_markers',
  untagged: 'untagged_ix_markers',
};

/** Keys this model owns, so a write carries every OTHER key across untouched. */
const OWNED_KEYS = [
  'ix_patterns',
  MARKER_KEY.tagged,
  MARKER_KEY.untagged,
  'wallet_contagion',
  'creator_is_tagged',
];

function flowObject(
  cfg: Record<string, unknown> | null | undefined,
): Record<string, unknown> | null {
  const flow = cfg?.m_flow_ix;
  return flow && typeof flow === 'object' && !Array.isArray(flow)
    ? (flow as Record<string, unknown>)
    : null;
}

function stringList(v: unknown): string[] {
  return Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string') : [];
}

/** Read `m_flow_ix.ix_patterns` from a fingerprint's `metric_config`. */
export function ixPatternsFromConfig(
  cfg: Record<string, unknown> | null | undefined,
): string[][] {
  const pats = flowObject(cfg)?.ix_patterns;
  if (!Array.isArray(pats)) return [];
  return pats.filter(
    (p): p is string[] => Array.isArray(p) && p.every((x) => typeof x === 'string'),
  );
}

/** The whole `m_flow_ix` classifier a `metric_config` carries.
 *
 *  The booleans default the way the BACKEND defaults them - absent means `true`
 *  (`FlowPatterns::default`), and the registry payload carries that default so the two
 *  cannot drift. Reading absent as `false` shows a control saying the opposite of what
 *  the engine does, and these two decide what the classifier measures, not how tightly.
 */
export function flowClassifierFromConfig(
  cfg: Record<string, unknown> | null | undefined,
  reg?: StrategyRegistry,
): FlowClassifier {
  const obj = flowObject(cfg);
  const fallback = (name: string) => {
    const d = findGroup(reg, 'm_flow_ix')?.fingerprint_config?.find((f) => f.name === name)
      ?.default;
    return typeof d === 'boolean' ? d : true;
  };
  const bool = (name: string) =>
    typeof obj?.[name] === 'boolean' ? (obj[name] as boolean) : fallback(name);
  // A row cannot legally carry both masks (the backend rejects it), so the side is
  // whichever key is PRESENT; `tagged` is the reading of a row carrying neither.
  const hasUntagged = obj !== null && obj[MARKER_KEY.untagged] !== undefined;
  const side: MarkerSide = hasUntagged ? 'untagged' : 'tagged';
  return {
    configured: obj !== null,
    ix_patterns: ixPatternsFromConfig(cfg),
    markers: stringList(obj?.[MARKER_KEY[side]]),
    markers_side: side,
    wallet_contagion: bool('wallet_contagion'),
    creator_is_tagged: bool('creator_is_tagged'),
  };
}

/** The `m_flow_ix` wallet rules alone - {@link flowClassifierFromConfig} narrowed for
 *  callers that only toggle those two. */
export function flowWalletRules(cfg: Record<string, unknown> | null | undefined): {
  wallet_contagion: boolean;
  creator_is_tagged: boolean;
} {
  const { wallet_contagion, creator_is_tagged } = flowClassifierFromConfig(cfg);
  return { wallet_contagion, creator_is_tagged };
}

/** **The one writer** of `fingerprints.metric_config.m_flow_ix`.
 *
 *  A PUT replaces the row, so every field of the classifier is written together and
 *  everything else has to survive. Three levels do:
 *
 *  * the other GROUPS - `prev` is the base rather than the result;
 *  * the other `m_flow_ix` keys this model does not own, carried across verbatim;
 *  * the side NOT selected - its key is removed, because the backend rejects a row
 *    carrying both masks rather than picking one.
 *
 *  `configured: false` drops the group, which is the only spelling of "no classifier"
 *  (unconfigured means every flow metric reads NaN). Every other writer routes through
 *  here: rebuilding the key from a subset of its fields reads as a partial write and
 *  lands as a full one, which is how both wallet rules were once reverted to their
 *  defaults - a DIFFERENT classifier - on any save that touched the fingerprint. */
export function metricConfigWithFlowClassifier(
  prev: Record<string, unknown>,
  classifier: FlowClassifier,
): Record<string, unknown> {
  const { m_flow_ix: prevFlow, ...otherGroups } = prev;
  if (!classifier.configured) return otherGroups;
  const keep: Record<string, unknown> = {};
  if (prevFlow && typeof prevFlow === 'object' && !Array.isArray(prevFlow)) {
    for (const [k, v] of Object.entries(prevFlow as Record<string, unknown>)) {
      if (!OWNED_KEYS.includes(k)) keep[k] = v;
    }
  }
  const patterns = classifier.ix_patterns
    .map((p) => p.map((x) => x.trim()).filter(Boolean))
    .filter((p) => p.length > 0);
  const markers = classifier.markers.map((m) => m.trim()).filter(Boolean);
  const flow: Record<string, unknown> = {
    ...keep,
    // Always EXPLICIT: a row that omits them says nothing about which classifier it
    // meant, which is how they came to be reverted unnoticed.
    wallet_contagion: classifier.wallet_contagion,
    creator_is_tagged: classifier.creator_is_tagged,
  };
  if (patterns.length > 0) flow.ix_patterns = patterns;
  if (markers.length > 0) flow[MARKER_KEY[classifier.markers_side]] = markers;
  return { ...otherGroups, m_flow_ix: flow };
}

/** The key `m_dump_ix` stores its build list under. Same field name as `m_flow_ix`'s,
 *  a DIFFERENT list: `m_flow_ix.ix_patterns` says which trades are tagged, and this
 *  says whose SELLS `dump_sell` / `dump_sell_count` count. The backend rejects a build
 *  present in both, so the two are disjoint by construction. */
export const DUMP_GROUP = 'm_dump_ix';

/** Read `m_dump_ix.ix_patterns` from a fingerprint's `metric_config`. */
export function dumpPatternsFromConfig(
  cfg: Record<string, unknown> | null | undefined,
): string[][] {
  const obj = cfg?.[DUMP_GROUP];
  const pats =
    obj && typeof obj === 'object' && !Array.isArray(obj)
      ? (obj as Record<string, unknown>).ix_patterns
      : undefined;
  if (!Array.isArray(pats)) return [];
  return pats.filter(
    (p): p is string[] => Array.isArray(p) && p.every((x) => typeof x === 'string'),
  );
}

/** **The one writer** of `fingerprints.metric_config.m_dump_ix`.
 *
 *  Same contract as {@link metricConfigWithFlowClassifier}: a PUT replaces the row, so
 *  `prev` is the base and every other group survives. This group has ONE field and no
 *  wallet rules - a build is a property of the transaction - so an empty list has no
 *  marker-classifier case to protect and simply drops the group, which is the only
 *  spelling of "no dump list" (both dump metrics then read NaN, never 0). */
export function metricConfigWithDumpPatterns(
  prev: Record<string, unknown>,
  patterns: string[][],
): Record<string, unknown> {
  const { [DUMP_GROUP]: prevDump, ...otherGroups } = prev;
  const cleaned = patterns
    .map((p) => p.map((x) => x.trim()).filter(Boolean))
    .filter((p) => p.length > 0);
  if (cleaned.length === 0) return otherGroups;
  const keep: Record<string, unknown> = {};
  if (prevDump && typeof prevDump === 'object' && !Array.isArray(prevDump)) {
    for (const [k, v] of Object.entries(prevDump as Record<string, unknown>)) {
      if (k !== 'ix_patterns') keep[k] = v;
    }
  }
  return { ...otherGroups, [DUMP_GROUP]: { ...keep, ix_patterns: cleaned } };
}

/** Which of the two ix-structure lists a surface is reading or writing.
 *
 *  `tagged` is `m_flow_ix.ix_patterns` - which trades the flow split calls
 *  volume-side. `dump` is `m_dump_ix.ix_patterns` - the builds whose SELLS
 *  `dump_sell` / `dump_sell_count` count. Two questions about the same ix structure,
 *  and a build may answer yes to both, so every staging surface has to say which one
 *  it means rather than leaving a click ambiguous - a click that lands in the wrong
 *  list is the real risk, since the overlap itself is legal. */
export type IxPatternList = 'tagged' | 'dump';

/** Read one list off a `metric_config`. */
export function patternsForList(
  cfg: Record<string, unknown> | null | undefined,
  list: IxPatternList,
): string[][] {
  return list === 'dump' ? dumpPatternsFromConfig(cfg) : ixPatternsFromConfig(cfg);
}

/** Write one list into `prev`, through that group's OWN writer, and return the whole
 *  config. Never rebuild either key from its pattern rows alone: a PUT replaces the
 *  row, so a partial write lands as a full one. */
export function metricConfigWithList(
  prev: Record<string, unknown>,
  patterns: string[][],
  list: IxPatternList,
): Record<string, unknown> {
  return list === 'dump'
    ? metricConfigWithDumpPatterns(prev, patterns)
    : metricConfigWithIxPatterns(patterns, prev);
}

/** Write flow patterns into `prev`, an existing `metric_config`, and return the WHOLE
 *  config - everything else preserved, through {@link metricConfigWithFlowClassifier}.
 *
 *  Clearing the patterns drops the group **unless the row is a marker classifier**. A
 *  marker classifier legitimately has no patterns - the backend rejects
 *  `untagged_ix_markers` alongside `ix_patterns`, so an empty list is the only shape it
 *  can have - and dropping the group there deleted the whole classifier on every save
 *  from a pattern-only surface, silently reclassifying flow for every rule bound to it.
 */
export function metricConfigWithIxPatterns(
  patterns: string[][],
  prev: Record<string, unknown> = {},
): Record<string, unknown> {
  const current = flowClassifierFromConfig(prev);
  const cleaned = patterns
    .map((p) => p.map((x) => x.trim()).filter(Boolean))
    .filter((p) => p.length > 0);
  return metricConfigWithFlowClassifier(prev, {
    ...current,
    ix_patterns: cleaned,
    configured: cleaned.length > 0 || current.markers.length > 0,
  });
}

/** Write the two wallet rules into a config, through the one writer. No `m_flow_ix`
 *  group means nothing to attach them to. */
export function withFlowWalletRules(
  cfg: Record<string, unknown>,
  rules: { wallet_contagion: boolean; creator_is_tagged: boolean },
): Record<string, unknown> {
  const current = flowClassifierFromConfig(cfg);
  if (!current.configured) return cfg;
  return metricConfigWithFlowClassifier(cfg, { ...current, ...rules });
}

/** The whole registry payload. */
export interface StrategyRegistry {
  operators: Operator[];
  /** The structural-marker vocabulary a `marker[]` field is picked from, served with
   *  the payload so a marker added to the engine reaches the picker with no frontend
   *  change. Optional only so a pre-marker payload still parses. */
  ix_markers?: IxMarkerSpec[];
  groups: GroupSpec[];
}

/** The marker vocabulary, routers last so the two kinds read as two blocks. */
export function ixMarkers(reg: StrategyRegistry | undefined): IxMarkerSpec[] {
  return [...(reg?.ix_markers ?? [])].sort((a, b) => Number(a.router) - Number(b.router));
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
