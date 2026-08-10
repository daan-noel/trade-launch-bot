// `RuleParams` — the TS mirror of the backend `hunter_engine::rule_params::RuleParams`
// (the typed form of `strategy_rules.params` JSONB). The wire JSON mixes a group's
// **strict params** (e.g. `window_size_sec`) with its **metric condition exprs** at
// the same level inside each `entry`/`exit` group object, so JSON⇄form conversion
// is registry-guided (the registry says which keys are strict params vs metrics).
//
// Metric conditions are DNF (`Condition[][]`): `,` AND within an arm, `|` OR across
// arms. Wire JSON is flat for a single arm (legacy) and nested for multi-arm.

import {
  conditionExprFromJson,
  conditionExprToJson,
  normalizeConditionExpr,
  type ConditionExpr,
} from './grammar';
import type { StrategyRegistry } from './registry';

/**
 * Editor-friendly form of one side's (entry/exit) groups, keyed by group name.
 *
 * Each group holds an **array of instances** — one per distinct `window_size_sec`
 * — mirroring the backend `hunter_engine::rule_params::SideConditions`
 * (`BTreeMap<group, Vec<GroupConditions>>`). This is what lets the same metric be
 * authored at two different trailing windows (e.g. `m_price_window.trail@5` AND
 * `@30`): two instances of the one group. A static group always has a single
 * window-less instance. On the wire a one-instance group serializes as a plain
 * object and a multi-instance group as an array (see `sideToJson`).
 */
export type SideConditions = Record<string, GroupConditions[]>;

/** One group **instance**'s authored content: strict params (its window) beside
 *  per-metric condition exprs. A group may hold several — one per window. */
export interface GroupConditions {
  /** Strict params by name, e.g. `{ window_size_sec: 10 }`. */
  strict: Record<string, number>;
  /** DNF condition arms per metric name. */
  metrics: Record<string, ConditionExpr>;
}

/** Flatten a side into `(groupName, instance)` pairs — one entry per window
 *  instance. Consumers that iterate metrics/windows use this so they don't have to
 *  care that a group may hold several instances. */
export function sideInstances(
  side: SideConditions | undefined,
): Array<[string, GroupConditions]> {
  if (!side) return [];
  const out: Array<[string, GroupConditions]> = [];
  for (const [groupName, instances] of Object.entries(side)) {
    for (const inst of instances) out.push([groupName, inst]);
  }
  return out;
}

/** Re-entry lifecycle (backend `hunter_engine::rule_params::ReEntry`). Absent ⇒
 *  one-shot (`Done` terminal per token+rule). Present ⇒ after a normal exit the
 *  (token, rule) waits `cooldown_sec` then re-arms, up to `max_episodes_per_token`
 *  entries. Mirror of the wire `reentry` object — must round-trip so the FE never
 *  silently drops it (it did before this field existed). */
export interface ReEntry {
  /** Seconds to wait after a close before re-arming (`>= 0`). */
  cooldown_sec: number;
  /** Hard cap on entries per token for this rule (integer `>= 1`). */
  max_episodes_per_token: number;
}

/** One ordered scale-out stage (mirror of `hunter_engine::rule_params::ExitStage`).
 *  `sell_bps: null` = remainder/`All` stage (must be last). */
export interface ExitStage {
  /** Basis points of the **initial** bag to sell. `null` = remainder stage. */
  sell_bps: number | null;
  /** Per-stage take-profit sugar (`pnl >= tp`). `null` = off. */
  take_profit: number | null;
  /** Authored metric conditions for this stage (exit grammar). */
  conditions: SideConditions;
}

/** Hard caps mirroring `hunter_engine::rule_params::{MAX_EXPLICIT_SCALE_STAGES,
 *  MAX_SCALE_SELL_BPS}` — keep in lockstep (validate.ts asserts the same). */
export const MAX_EXPLICIT_SCALE_STAGES = 3;
export const MAX_SCALE_SELL_BPS = 9900;

/**
 * Parked (toggled-off) conditions and scale-out stages — mirror of the backend
 * `hunter_engine::rule_params::DisabledConditions`. Same shape as a live side /
 * ladder, so flipping a toggle is a move between the two bags and nothing else. Kept
 * in `params` so a parked condition survives save+reload, but compiled by nothing:
 * the engine reads `entry`/`exit`/`scale_out` only. A parked row MAY duplicate a live
 * one on the same (group, window, metric) — that is the point (park `trail >= 12`
 * while trying `trail >= 20`), and separate bags keep them from overwriting each
 * other.
 *
 * Parked **stages** are validated per stage only: the cross-stage rules (remainder
 * last, stage count, bps sum) describe a ladder, and this is a shelf of spares —
 * summing them would make parking a stage pointless.
 */
export interface DisabledConditions {
  entry?: SideConditions;
  exit?: SideConditions;
  /** Parked scale-out stages. Order is authoring order only — no execution meaning. */
  scale_out?: ExitStage[] | null;
}

/** The whole rule `params` object in form shape. */
export interface RuleParams {
  /** Take-profit % of entry price (`100` = +100%). `null`/absent = off. */
  take_profit: number | null;
  /** Stop-loss % drop from entry. `null`/absent = off. */
  stop_loss: number | null;
  /** Entry side. `undefined`/empty ⇒ enter on arm (fingerprint alone). */
  entry?: SideConditions;
  /** Exit side. `undefined`/empty ⇒ TP/SL/death only. */
  exit?: SideConditions;
  /** Ordered partial-exit ladder. `null`/empty ⇒ legacy full close only. */
  scale_out?: ExitStage[] | null;
  /** Re-entry lifecycle. `null`/absent ⇒ one-shot. */
  reentry?: ReEntry | null;
  /** Skip entry while ANY other rule (manual buys included) already holds this
   *  token. `false` = today's behavior: rules stack independently. */
  exclusive: boolean;
  /** Tiebreak between two contesting `exclusive` rules — higher wins, ties break by
   *  rule id. Meaningless when `exclusive` is false. */
  priority: number;
  /** Conditions toggled off but kept. `null`/absent = nothing parked. */
  disabled?: DisabledConditions | null;
  /** Size the buy as a percent of the pool's SOL reserve instead of the rule's fixed
   *  amount. `null`/absent = fixed. Holds our own price impact (`buy / vsol`) constant
   *  across a liquidity band a fixed size varies over. */
  buy_pct_of_vsol?: number | null;
}

/** An empty rule (fingerprint-only, no TP/SL/conditions). */
export function emptyRuleParams(): RuleParams {
  return {
    take_profit: null,
    stop_loss: null,
    entry: {},
    exit: {},
    scale_out: null,
    reentry: null,
    exclusive: false,
    priority: 0,
    disabled: null,
    buy_pct_of_vsol: null,
  };
}

/** Serialize form → canonical `params` JSON. Drops empty metric lists and no-op
 *  groups (a group with no metric conditions — the backend rejects those). */
export function ruleParamsToJson(p: RuleParams): Record<string, unknown> {
  const root: Record<string, unknown> = {};
  if (p.take_profit != null) root.take_profit = p.take_profit;
  if (p.stop_loss != null) root.stop_loss = p.stop_loss;
  const entry = sideToJson(p.entry);
  if (entry) root.entry = entry;
  const exit = sideToJson(p.exit);
  if (exit) root.exit = exit;
  const scaleOut = scaleOutToJson(p.scale_out);
  if (scaleOut) root.scale_out = scaleOut;
  // Re-entry rides along only when fully specified (both a cooldown and an episode
  // cap) — the backend requires both keys and rejects a partial object.
  const re = p.reentry;
  if (
    re &&
    Number.isFinite(re.cooldown_sec) &&
    Number.isFinite(re.max_episodes_per_token)
  ) {
    root.reentry = {
      cooldown_sec: re.cooldown_sec,
      max_episodes_per_token: re.max_episodes_per_token,
    };
  }
  // Parked conditions/stages ride along only when something is actually parked — an
  // empty bag is the same as none (mirrors the backend `parse_opt_disabled` fold).
  const parked: Record<string, unknown> = {};
  const parkedEntry = sideToJson(p.disabled?.entry);
  if (parkedEntry) parked.entry = parkedEntry;
  const parkedExit = sideToJson(p.disabled?.exit);
  if (parkedExit) parked.exit = parkedExit;
  const parkedStages = scaleOutToJson(p.disabled?.scale_out);
  if (parkedStages) parked.scale_out = parkedStages;
  if (Object.keys(parked).length) root.disabled = parked;
  // Defaults stay off the wire so existing rules round-trip unchanged.
  if (p.buy_pct_of_vsol != null && Number.isFinite(p.buy_pct_of_vsol)) {
    root.buy_pct_of_vsol = p.buy_pct_of_vsol;
  }
  if (p.exclusive) root.exclusive = true;
  if (Number.isFinite(p.priority) && p.priority !== 0) root.priority = p.priority;
  return root;
}

function sideToJson(side: SideConditions | undefined): Record<string, unknown> | undefined {
  if (!side) return undefined;
  const out: Record<string, unknown> = {};
  for (const [groupName, instances] of Object.entries(side)) {
    const objs: Record<string, unknown>[] = [];
    for (const group of instances) {
      const g: Record<string, unknown> = {};
      let hasMetric = false;
      for (const [metric, arms] of Object.entries(group.metrics)) {
        if (arms.length > 0) {
          g[metric] = conditionExprToJson(arms);
          hasMetric = true;
        }
      }
      // Strict params only ride along an instance that actually constrains something.
      if (!hasMetric) continue;
      for (const [name, v] of Object.entries(group.strict)) {
        if (v != null && Number.isFinite(v)) g[name] = v;
      }
      objs.push(g);
    }
    if (objs.length === 0) continue;
    // Mirror the backend `side_to_value`: a single instance stays a plain object
    // (no migration for existing rules); several become a JSON array, one per window.
    out[groupName] = objs.length === 1 ? objs[0] : objs;
  }
  return Object.keys(out).length ? out : undefined;
}

/** Parse canonical `params` JSON → form, using the registry to split each group's
 *  strict params from its metric condition lists. Tolerant: unknown keys are
 *  dropped (validation reports them separately). */
export function ruleParamsFromJson(json: unknown, reg: StrategyRegistry | undefined): RuleParams {
  const obj = (json && typeof json === 'object' ? json : {}) as Record<string, unknown>;
  return {
    take_profit: numOrNull(obj.take_profit),
    stop_loss: numOrNull(obj.stop_loss),
    entry: sideFromJson(obj.entry, reg) ?? {},
    exit: sideFromJson(obj.exit, reg) ?? {},
    scale_out: scaleOutFromJson(obj.scale_out, reg),
    reentry: reentryFromJson(obj.reentry),
    buy_pct_of_vsol:
      typeof obj.buy_pct_of_vsol === 'number' && Number.isFinite(obj.buy_pct_of_vsol)
        ? obj.buy_pct_of_vsol
        : null,
    exclusive: obj.exclusive === true,
    priority: typeof obj.priority === 'number' && Number.isFinite(obj.priority) ? obj.priority : 0,
    disabled: disabledFromJson(obj.disabled, reg),
  };
}

/** Parse the wire `disabled` bag → form. An all-empty bag folds to `null`, the
 *  same sentinel the backend applies. */
function disabledFromJson(
  v: unknown,
  reg: StrategyRegistry | undefined,
): DisabledConditions | null {
  if (!v || typeof v !== 'object' || Array.isArray(v)) return null;
  const o = v as Record<string, unknown>;
  const entry = sideFromJson(o.entry, reg);
  const exit = sideFromJson(o.exit, reg);
  const scale_out = scaleOutFromJson(o.scale_out, reg);
  const has = (s: SideConditions | undefined) => s != null && Object.keys(s).length > 0;
  if (!has(entry) && !has(exit) && !scale_out?.length) return null;
  return { entry, exit, scale_out };
}

/** Empty array folds to `null` (same sentinel as backend `configured_labels`). */
function scaleOutFromJson(
  v: unknown,
  reg: StrategyRegistry | undefined,
): ExitStage[] | null {
  if (v == null) return null;
  if (!Array.isArray(v) || v.length === 0) return null;
  const stages: ExitStage[] = [];
  for (const item of v) {
    if (!item || typeof item !== 'object' || Array.isArray(item)) continue;
    const o = item as Record<string, unknown>;
    const sellRaw = o.sell_bps;
    let sell_bps: number | null = null;
    if (sellRaw != null && typeof sellRaw === 'number' && Number.isFinite(sellRaw)) {
      sell_bps = sellRaw;
    }
    stages.push({
      sell_bps,
      take_profit: numOrNull(o.take_profit),
      conditions: sideFromJson(o.conditions, reg) ?? {},
    });
  }
  return stages.length > 0 ? stages : null;
}

function scaleOutToJson(stages: ExitStage[] | null | undefined): unknown[] | undefined {
  if (!stages || stages.length === 0) return undefined;
  const out: Record<string, unknown>[] = [];
  for (const s of stages) {
    const obj: Record<string, unknown> = {};
    if (s.sell_bps != null && Number.isFinite(s.sell_bps)) obj.sell_bps = s.sell_bps;
    if (s.take_profit != null) obj.take_profit = s.take_profit;
    const conditions = sideToJson(s.conditions);
    if (conditions) obj.conditions = conditions;
    // Skip a completely blank draft stage (no bps, no TP, no conditions).
    if (Object.keys(obj).length === 0 && s.sell_bps == null) continue;
    out.push(obj);
  }
  return out.length > 0 ? out : undefined;
}

/** Parse the wire `reentry` object → form (`null` when absent or malformed). Both
 *  keys must be present + finite (a partial object is not a valid re-entry block). */
function reentryFromJson(v: unknown): ReEntry | null {
  if (!v || typeof v !== 'object') return null;
  const o = v as Record<string, unknown>;
  const cooldown = o.cooldown_sec;
  const max = o.max_episodes_per_token;
  if (typeof cooldown !== 'number' || !Number.isFinite(cooldown)) return null;
  if (typeof max !== 'number' || !Number.isFinite(max)) return null;
  return { cooldown_sec: cooldown, max_episodes_per_token: max };
}

function sideFromJson(
  v: unknown,
  reg: StrategyRegistry | undefined,
): SideConditions | undefined {
  if (!v || typeof v !== 'object') return undefined;
  const side: SideConditions = {};
  for (const [groupName, groupVal] of Object.entries(v as Record<string, unknown>)) {
    const spec = reg?.groups.find((g) => g.name === groupName);
    // A group is a plain object (one window instance) OR a JSON array (one object
    // per window). Parse both so a multi-window rule loads without dropping its
    // conditions — the bug this fixes silently collapsed the array to nothing.
    const rawInstances = Array.isArray(groupVal) ? groupVal : [groupVal];
    const instances: GroupConditions[] = [];
    for (const raw of rawInstances) {
      const group: GroupConditions = { strict: {}, metrics: {} };
      const gobj = (raw && typeof raw === 'object' && !Array.isArray(raw) ? raw : {}) as Record<
        string,
        unknown
      >;
      for (const [key, val] of Object.entries(gobj)) {
        if (spec?.strict_params.some((sp) => sp.name === key)) {
          if (typeof val === 'number') group.strict[key] = val;
        } else if (Array.isArray(val)) {
          const arms = conditionExprFromJson(val);
          if (arms) {
            const tol = spec?.metrics.find((m) => m.name === key)?.eq_tolerance ?? 0;
            group.metrics[key] = normalizeConditionExpr(arms, tol);
          }
        }
      }
      instances.push(group);
    }
    side[groupName] = instances;
  }
  return side;
}

function numOrNull(v: unknown): number | null {
  return typeof v === 'number' && Number.isFinite(v) ? v : null;
}

/**
 * Canonicalize wire `params` JSON for equality: drop null/undefined, drop empty
 * objects, sort object keys, and sort **set-like** arrays. So
 * `{take_profit:50,stop_loss:null}` equals `{take_profit:50}` — the same semantic
 * rule the engine treats as identical.
 *
 * Array order carries no meaning anywhere in `params` **except** `scale_out`: a
 * group's window instances (`m_flow_window: [{…@25},{…@50}]`) are a set keyed by
 * window, and a metric's DNF arms / AND atoms are conjunctions and disjunctions —
 * all order-free. Only the scale-out ladder executes in the authored order, so it
 * is the one array kept positional. Without this, a rule that survived an editor
 * round-trip (which re-emits window instances sorted by window) failed to compare
 * equal to the sweep combo it was promoted from — the "best" badge on the sweep's
 * Used-by column silently went missing on every multi-window winner.
 */
export function canonicalizeRuleParamsJson(raw: unknown): unknown {
  return canonicalizeJson(raw) ?? {};
}

function canonicalizeJson(v: unknown, ordered = false): unknown {
  if (v == null) return undefined;
  if (typeof v !== 'object') return v;
  if (Array.isArray(v)) {
    const items = v.map((x) => canonicalizeJson(x) ?? null);
    // Sort by the canonical rendering of each element, so the order of a set-like
    // array can't decide equality. `ordered` (the `scale_out` ladder) opts out.
    return ordered
      ? items
      : items
          .map((x) => [JSON.stringify(x) ?? 'null', x] as const)
          .sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0))
          .map(([, x]) => x);
  }
  const out: Record<string, unknown> = {};
  for (const k of Object.keys(v as object).sort()) {
    // Only the ladder ITSELF is positional — each stage's nested `conditions` are
    // set-like again, which falls out of passing the flag one level deep only.
    const c = canonicalizeJson((v as Record<string, unknown>)[k], k === 'scale_out');
    if (c === undefined) continue;
    if (typeof c === 'object' && c !== null && !Array.isArray(c) && Object.keys(c).length === 0) {
      continue;
    }
    out[k] = c;
  }
  return out;
}

/** True when two `params` blobs are semantically the same rule logic. */
export function ruleParamsJsonEqual(a: unknown, b: unknown): boolean {
  return (
    JSON.stringify(canonicalizeRuleParamsJson(a)) === JSON.stringify(canonicalizeRuleParamsJson(b))
  );
}
