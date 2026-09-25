// A rule's `params`: WHEN it buys and HOW it sells. The frontend mirror of the
// engine's `hunter_engine::rule_params` (format 2):
//
//   enter    the buy: `event` (the print that triggers it), `filters` (must also hold,
//            else keep watching), `final_filters` (else give up the coin), `lock`,
//            `size_pct_of_pool`
//   signals  named conditions, written once, used by name in any line
//   always   sell lines checked first, in every stage
//   stages   the steps after the buy, each with its own lines and an optional deadline
//
// The editor holds a {@link RuleDoc}: the wire shape with every condition expression
// parsed and a stable `id` on each list item (a condition input keeps its draft text,
// so an index key would hand one row's draft to its neighbour after a delete).
// `ruleDocToJson` writes exactly what the engine's `RuleParams::to_value` writes.

import { conditionExprFromJson, conditionExprToJson, type ConditionExpr } from './grammar';
import type { MetricRef } from './metricRef';

/** The rule format this editor reads and writes (engine `RULE_FORMAT_VERSION`). */
export const RULE_FORMAT_VERSION = 2;

/** Largest partial sell, percent of the first buy's bag (engine `MAX_SELL_PCT`). */
export const MAX_SELL_PCT = 99;

/** Largest buy as a percent of the pool (engine `MAX_SIZE_PCT_OF_POOL`). */
export const MAX_SIZE_PCT_OF_POOL = 10;

/** Most stages a rule may have (engine `MAX_STAGES`). */
export const MAX_STAGES = 32;

export interface MetricCond {
  kind: 'metric';
  id: string;
  ref: MetricRef;
  /** DNF: OR of AND-arms. */
  is: ConditionExpr;
  /** Kept in place and validated, never compiled. */
  off: boolean;
}

export interface SignalCond {
  kind: 'signal';
  id: string;
  signal: string;
  /** Holds when the signal does NOT hold. */
  not: boolean;
  off: boolean;
}

export type Cond = MetricCond | SignalCond;

export interface Sell {
  /** The exit reason. Empty = labelled from the line's first condition. */
  label: string;
  /** Percent of the FIRST buy's bag; `null` = everything left. */
  pct: number | null;
}

/** `if <conditions> -> sell and/or go`. */
export interface Line {
  id: string;
  /** AND. */
  if: Cond[];
  sell: Sell | null;
  go: string | null;
  off: boolean;
}

export type DeadlineBasis = 'age_sec' | 'held_sec' | 'stage_sec';

export interface Deadline {
  basis: DeadlineBasis;
  secs: number;
}

export interface Stage {
  id: string;
  name: string;
  ends: Deadline | null;
  /** Checked on every print and tick while in this stage; the first that holds acts. */
  on: Line[];
  /** Checked once, at the deadline. */
  at_end: Line[];
  /** Where the deadline leads when no `at_end` line acts. `null` = the next stage. */
  then: string | null;
}

export interface Signal {
  id: string;
  name: string;
  /** OR of AND-groups; metric conditions only. */
  groups: MetricCond[][];
}

export type EntryLock = 'token' | 'slot';

export interface Enter {
  event: Cond[];
  filters: Cond[];
  final_filters: Cond[];
  lock: EntryLock | null;
  size_pct_of_pool: number | null;
}

export interface ReEntry {
  cooldown_sec: number;
  max_per_coin: number;
}

export interface RuleDoc {
  enter: Enter;
  take_profit: number | null;
  stop_loss: number | null;
  signals: Signal[];
  always: Line[];
  stages: Stage[];
  reentry: ReEntry | null;
  exclusive: boolean;
  priority: number;
}

let nextId = 0;
/** A fresh list-item id (editor only, never written). */
export function newId(): string {
  nextId += 1;
  return `r${nextId}`;
}

export function emptyRuleDoc(): RuleDoc {
  return {
    enter: { event: [], filters: [], final_filters: [], lock: null, size_pct_of_pool: null },
    take_profit: null,
    stop_loss: null,
    signals: [],
    always: [],
    stages: [],
    reentry: null,
    exclusive: false,
    priority: 0,
  };
}

export function metricCond(ref: MetricRef, is: ConditionExpr = [], off = false): MetricCond {
  return { kind: 'metric', id: newId(), ref, is, off };
}

export function signalCond(signal: string, not = false): SignalCond {
  return { kind: 'signal', id: newId(), signal, not, off: false };
}

export function newLine(partial: Partial<Omit<Line, 'id'>> = {}): Line {
  return { id: newId(), if: [], sell: { label: '', pct: null }, go: null, off: false, ...partial };
}

export function newStage(name: string): Stage {
  return { id: newId(), name, ends: null, on: [], at_end: [], then: null };
}

// ── Parse (tolerant of shape, strict about the format) ─────────────────────────

/** Why a stored document is not a v2 rule, or `null` when it is one. A v1 rule
 *  (`entry`, `exit`, `scale_out`, ...) is converted by the backend on save; the editor
 *  only reads v2. */
export function notV2Reason(raw: unknown): string | null {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return 'params must be an object';
  const v1 = ['entry', 'exit', 'entry_event', 'entry_lock', 'scale_out', 'disabled', 'buy_pct_of_vsol'];
  const found = Object.keys(raw).filter((k) => v1.includes(k));
  return found.length
    ? `this is a format-1 rule (${found.join(', ')}); save it through the backend to convert it`
    : null;
}

type Obj = Record<string, unknown>;
const isObj = (v: unknown): v is Obj => !!v && typeof v === 'object' && !Array.isArray(v);
const str = (v: unknown): string | undefined => (typeof v === 'string' ? v : undefined);
const numOrNull = (v: unknown): number | null =>
  typeof v === 'number' && Number.isFinite(v) ? v : null;

function condFromJson(v: unknown): Cond {
  const o = isObj(v) ? v : {};
  const off = o.off === true;
  if (typeof o.signal === 'string') {
    return { kind: 'signal', id: newId(), signal: o.signal, not: o.not === true, off };
  }
  const ref: MetricRef = { metric: str(o.metric) ?? '' };
  const tag = str(o.tag);
  const span = str(o.span);
  const slice = str(o.slice);
  if (tag) ref.tag = tag;
  if (span) ref.span = span;
  if (slice) ref.slice = slice;
  return { kind: 'metric', id: newId(), ref, is: conditionExprFromJson(o.is) ?? [], off };
}

function condsFromJson(v: unknown): Cond[] {
  return Array.isArray(v) ? v.map(condFromJson) : [];
}

function lineFromJson(v: unknown): Line {
  const o = isObj(v) ? v : {};
  let sell: Sell | null = null;
  if (o.sell === true) sell = { label: '', pct: numOrNull(o.sell_pct) };
  else if (typeof o.sell === 'string') sell = { label: o.sell, pct: numOrNull(o.sell_pct) };
  return { id: newId(), if: condsFromJson(o.if), sell, go: str(o.go) ?? null, off: o.off === true };
}

function linesFromJson(v: unknown): Line[] {
  return Array.isArray(v) ? v.map(lineFromJson) : [];
}

function stageFromJson(v: unknown): Stage {
  const o = isObj(v) ? v : {};
  let ends: Deadline | null = null;
  if (isObj(o.ends)) {
    const [k, secs] = Object.entries(o.ends)[0] ?? [];
    if ((k === 'age_sec' || k === 'held_sec' || k === 'stage_sec') && typeof secs === 'number') {
      ends = { basis: k, secs };
    }
  }
  return {
    id: newId(),
    name: str(o.name) ?? '',
    ends,
    on: linesFromJson(o.on),
    at_end: linesFromJson(o.at_end),
    then: str(o.then) ?? null,
  };
}

/** Read a stored `params` document into the editor model. Throws with the reason when
 *  the document is not a v2 rule; shape slips inside a v2 document are kept for the
 *  validator to name. */
export function ruleDocFromJson(raw: unknown): RuleDoc {
  const why = notV2Reason(raw);
  if (why) throw new Error(why);
  const o = raw as Obj;
  const e = isObj(o.enter) ? o.enter : {};
  const lock = e.lock === 'token' || e.lock === 'slot' ? e.lock : null;
  const signals: Signal[] = isObj(o.signals)
    ? Object.entries(o.signals).map(([name, groups]) => ({
        id: newId(),
        name,
        groups: (Array.isArray(groups) ? groups : []).map((g) =>
          condsFromJson(g).filter((c): c is MetricCond => c.kind === 'metric'),
        ),
      }))
    : [];
  const re = isObj(o.reentry) ? o.reentry : null;
  return {
    enter: {
      event: condsFromJson(e.event),
      filters: condsFromJson(e.filters),
      final_filters: condsFromJson(e.final_filters),
      lock,
      size_pct_of_pool: numOrNull(e.size_pct_of_pool),
    },
    take_profit: numOrNull(o.take_profit),
    stop_loss: numOrNull(o.stop_loss),
    signals,
    always: linesFromJson(o.always),
    stages: Array.isArray(o.stages) ? o.stages.map(stageFromJson) : [],
    reentry: re
      ? { cooldown_sec: numOrNull(re.cooldown_sec) ?? NaN, max_per_coin: numOrNull(re.max_per_coin) ?? NaN }
      : null,
    exclusive: o.exclusive === true,
    priority: typeof o.priority === 'number' ? o.priority : 0,
  };
}

// ── Serialize (the engine's `to_value`, key for key) ───────────────────────────

function condToJson(c: Cond): Obj {
  if (c.kind === 'signal') {
    const o: Obj = { signal: c.signal };
    if (c.not) o.not = true;
    if (c.off) o.off = true;
    return o;
  }
  const o: Obj = { metric: c.ref.metric };
  if (c.ref.tag) o.tag = c.ref.tag;
  if (c.ref.span) o.span = c.ref.span;
  if (c.ref.slice) o.slice = c.ref.slice;
  o.is = conditionExprToJson(c.is);
  if (c.off) o.off = true;
  return o;
}

function lineToJson(l: Line): Obj {
  const o: Obj = {};
  if (l.if.length) o.if = l.if.map(condToJson);
  if (l.sell) {
    o.sell = l.sell.label.trim() ? l.sell.label.trim() : true;
    if (l.sell.pct != null) o.sell_pct = l.sell.pct;
  }
  if (l.go) o.go = l.go;
  if (l.off) o.off = true;
  return o;
}

function stageToJson(s: Stage): Obj {
  const o: Obj = { name: s.name };
  if (s.ends) o.ends = { [s.ends.basis]: s.ends.secs };
  if (s.on.length) o.on = s.on.map(lineToJson);
  if (s.at_end.length) o.at_end = s.at_end.map(lineToJson);
  if (s.then) o.then = s.then;
  return o;
}

/** The canonical `params` JSON. Empty parts are left out, as the engine writes them. */
export function ruleDocToJson(d: RuleDoc): Obj {
  const root: Obj = {};
  const enter: Obj = {};
  if (d.enter.event.length) enter.event = d.enter.event.map(condToJson);
  if (d.enter.filters.length) enter.filters = d.enter.filters.map(condToJson);
  if (d.enter.final_filters.length) enter.final_filters = d.enter.final_filters.map(condToJson);
  if (d.enter.lock) enter.lock = d.enter.lock;
  if (d.enter.size_pct_of_pool != null) enter.size_pct_of_pool = d.enter.size_pct_of_pool;
  if (Object.keys(enter).length) root.enter = enter;
  if (d.take_profit != null) root.take_profit = d.take_profit;
  if (d.stop_loss != null) root.stop_loss = d.stop_loss;
  if (d.signals.length) {
    // The engine keeps signals in a map sorted by name.
    const sorted = [...d.signals].sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
    root.signals = Object.fromEntries(sorted.map((s) => [s.name, s.groups.map((g) => g.map(condToJson))]));
  }
  if (d.always.length) root.always = d.always.map(lineToJson);
  if (d.stages.length) root.stages = d.stages.map(stageToJson);
  if (d.reentry) root.reentry = { cooldown_sec: d.reentry.cooldown_sec, max_per_coin: d.reentry.max_per_coin };
  if (d.exclusive) root.exclusive = true;
  if (d.priority !== 0) root.priority = d.priority;
  return root;
}

// ── Walks ───────────────────────────────────────────────────────────────────

/** Every line: `always`, then each stage's `on` and `at_end` (the engine's order). */
export function allLines(d: RuleDoc): Line[] {
  return [...d.always, ...d.stages.flatMap((s) => [...s.on, ...s.at_end])];
}

/** Every live metric condition the rule reads, in every part. */
export function liveMetricConds(d: RuleDoc): MetricCond[] {
  const out: MetricCond[] = [];
  const take = (cs: Cond[]) => {
    for (const c of cs) if (c.kind === 'metric' && !c.off) out.push(c);
  };
  take(d.enter.event);
  take(d.enter.filters);
  take(d.enter.final_filters);
  for (const s of d.signals) for (const g of s.groups) take(g);
  for (const l of allLines(d)) if (!l.off) take(l.if);
  return out;
}

/** True when the rule buys on arming alone: no live entry condition at all. */
export function entersOnArm(d: RuleDoc): boolean {
  const live = (cs: Cond[]) => cs.some((c) => !c.off);
  return !live(d.enter.event) && !live(d.enter.filters) && !live(d.enter.final_filters);
}

/** A name that is not in `taken`: `base`, `base_2`, `base_3`, ... */
export function freshName(base: string, taken: readonly string[]): string {
  if (!taken.includes(base)) return base;
  for (let i = 2; ; i += 1) {
    const n = `${base}_${i}`;
    if (!taken.includes(n)) return n;
  }
}

// ── Renames (a name is referenced by lines, so a rename carries its references) ──

function mapConds(cs: Cond[], f: (c: Cond) => Cond): Cond[] {
  return cs.map(f);
}

function mapLines(d: RuleDoc, f: (l: Line) => Line): RuleDoc {
  return {
    ...d,
    always: d.always.map(f),
    stages: d.stages.map((s) => ({ ...s, on: s.on.map(f), at_end: s.at_end.map(f) })),
  };
}

/** Rename a stage and every `go` / `then` that names it. */
export function renameStage(d: RuleDoc, from: string, to: string): RuleDoc {
  const next = mapLines(d, (l) => (l.go === from ? { ...l, go: to } : l));
  return {
    ...next,
    stages: next.stages.map((s) => ({
      ...s,
      name: s.name === from ? to : s.name,
      then: s.then === from ? to : s.then,
    })),
  };
}

/** Rename a signal and every condition that uses it. */
export function renameSignal(d: RuleDoc, from: string, to: string): RuleDoc {
  const f = (c: Cond): Cond => (c.kind === 'signal' && c.signal === from ? { ...c, signal: to } : c);
  const next = mapLines(d, (l) => ({ ...l, if: mapConds(l.if, f) }));
  return {
    ...next,
    enter: {
      ...next.enter,
      event: mapConds(next.enter.event, f),
      filters: mapConds(next.enter.filters, f),
      final_filters: mapConds(next.enter.final_filters, f),
    },
    signals: next.signals.map((s) => (s.name === from ? { ...s, name: to } : s)),
  };
}
