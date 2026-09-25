// A fingerprint's **tags**: named trade lists. Each tag splits every coin's trades in
// two, `@name` (the trades that carry it) and `@!name` (the rest). The frontend mirror
// of `hunter_engine::metrics::tags::config`:
//
//   { "volume": { "match": { "program": [...], "cluster": {"min_prints": 3, "sol_tol_pct": 10} },
//                 "side": "sell", "sticky": true, "exclude_creation_slot": true } }
//
// A trade carries the tag when ANY `match` entry holds, on the tag's `side` only.
// `tagsToJson` is the one writer: every surface that adds to a tag goes through
// `withTagListValue` / `withTagShape`, so a save never drops a key another surface
// wrote. Removing a tag's last matcher value removes the tag: the engine refuses a
// tag no trade could carry.

import {
  MAX_TX_COMPUTE_UNITS,
  parseIxPatternRows,
  patternRowKey,
  serializeIxPatternRows,
  type IxPatternRow,
} from './ixPatternRows';
import type { StrategyRegistry } from './registry';

/** Tag names: `[a-z0-9_]`, 1 to 24 characters (engine `tags::check_name`). */
export const TAG_NAME_RE = /^[a-z0-9_]{1,24}$/;

export interface Cluster {
  min_prints: number;
  sol_tol_pct: number;
}

/** The matchers a tag may use. An absent key is not used. */
export interface TagMatch {
  program?: string[];
  ix_shape?: IxPatternRow[];
  ix_template?: string[];
  ix_contains?: string[];
  ix_lacks?: string[];
  wallet?: string[];
  creator?: boolean;
  cluster?: Cluster;
}

export type MatcherKey = keyof TagMatch;

/** The list matchers (each a list of strings), for generic add/remove. */
export const STRING_LIST_MATCHERS = ['program', 'ix_template', 'ix_contains', 'ix_lacks', 'wallet'] as const;
export type StringListMatcher = (typeof STRING_LIST_MATCHERS)[number];

export interface TagDef {
  /** Editor-only list key. */
  id: string;
  name: string;
  match: TagMatch;
  /** Only buys, or only sells, can carry the tag. `null` = both. */
  side: 'buy' | 'sell' | null;
  /** A wallet that carried the tag once carries it for the rest of the coin. */
  sticky: boolean;
  /** A creation-slot buyer that matches nothing counts on neither side. */
  exclude_creation_slot: boolean;
}

let nextId = 0;
const newId = () => `t${(nextId += 1)}`;

type Obj = Record<string, unknown>;
const isObj = (v: unknown): v is Obj => !!v && typeof v === 'object' && !Array.isArray(v);
const strings = (v: unknown): string[] =>
  Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string') : [];

export function emptyTag(name: string): TagDef {
  return { id: newId(), name, match: {}, side: null, sticky: false, exclude_creation_slot: false };
}

/** Read a stored `tags` document. Unknown keys are dropped; the backend refuses them. */
export function tagsFromJson(doc: unknown): TagDef[] {
  if (!isObj(doc)) return [];
  return Object.entries(doc).map(([name, def]) => {
    const d = isObj(def) ? def : {};
    const m = isObj(d.match) ? d.match : {};
    const match: TagMatch = {};
    for (const k of STRING_LIST_MATCHERS) if (m[k] !== undefined) match[k] = strings(m[k]);
    if (m.ix_shape !== undefined) match.ix_shape = parseIxPatternRows(m.ix_shape);
    if (typeof m.creator === 'boolean') match.creator = m.creator;
    if (isObj(m.cluster)) {
      match.cluster = {
        min_prints: Number(m.cluster.min_prints),
        sol_tol_pct: Number(m.cluster.sol_tol_pct),
      };
    }
    return {
      id: newId(),
      name,
      match,
      side: d.side === 'buy' || d.side === 'sell' ? d.side : null,
      sticky: d.sticky === true,
      exclude_creation_slot: d.exclude_creation_slot === true,
    };
  });
}

/** Whether a matcher is in use (a non-empty list, `creator: true`, a cluster). */
export function matcherUsed(m: TagMatch, k: MatcherKey): boolean {
  const v = m[k];
  if (v === undefined) return false;
  if (Array.isArray(v)) return v.length > 0;
  if (typeof v === 'boolean') return v;
  return true;
}

/** The matchers a tag uses, in the classifier's fixed order (cluster last). */
export function usedMatchers(t: TagDef): MatcherKey[] {
  const order: MatcherKey[] = ['program', 'ix_shape', 'ix_template', 'ix_contains', 'ix_lacks', 'wallet', 'creator', 'cluster'];
  return order.filter((k) => matcherUsed(t.match, k));
}

/** The stored document. A matcher not in use is left out, so the stored row carries
 *  only what classifies. */
export function tagsToJson(tags: TagDef[]): Obj {
  const out: Obj = {};
  for (const t of tags) {
    const match: Obj = {};
    for (const k of STRING_LIST_MATCHERS) {
      const list = (t.match[k] ?? []).map((s) => s.trim()).filter(Boolean);
      if (list.length) match[k] = [...new Set(list)];
    }
    const shapes = serializeIxPatternRows(t.match.ix_shape ?? []);
    if (shapes.length) match.ix_shape = shapes;
    if (t.match.creator) match.creator = true;
    if (t.match.cluster) match.cluster = { ...t.match.cluster };
    const def: Obj = { match };
    if (t.side) def.side = t.side;
    if (t.sticky) def.sticky = true;
    if (t.exclude_creation_slot) def.exclude_creation_slot = true;
    out[t.name] = def;
  }
  return out;
}

/** The tag names a stored document defines. */
export function tagNames(doc: unknown): string[] {
  return isObj(doc) ? Object.keys(doc) : [];
}

/** Whether the slot / wave families and `m_crowd.unique_ix_templates` can read the
 *  tag: only its `ix_template` and `program` matchers count at that level. */
export function hasTemplateView(t: TagDef): boolean {
  return matcherUsed(t.match, 'ix_template') || matcherUsed(t.match, 'program');
}

/** `program X, Y or ix template Z or the creator`: what a tag matches, in words. */
export function tagSentence(t: TagDef): string {
  const m = t.match;
  const parts: string[] = [];
  const list = (xs: string[] | undefined, what: string) => {
    if (xs?.length) parts.push(`${what} ${xs.length > 3 ? `${xs.slice(0, 3).join(', ')} (+${xs.length - 3})` : xs.join(', ')}`);
  };
  list(m.program, 'program');
  if (m.ix_shape?.length) parts.push(`${m.ix_shape.length} exact ix shape${m.ix_shape.length === 1 ? '' : 's'}`);
  list(m.ix_template, 'ix template');
  list(m.ix_contains, 'contains');
  list(m.ix_lacks, 'lacks all of');
  if (m.wallet?.length) parts.push(`${m.wallet.length} wallet${m.wallet.length === 1 ? '' : 's'}`);
  if (m.creator) parts.push('the creator');
  if (m.cluster) parts.push(`a same-slot cluster of ${m.cluster.min_prints}+ prints within ${m.cluster.sol_tol_pct} % SOL`);
  const opts: string[] = [];
  if (t.side) opts.push(`${t.side}s only`);
  if (t.sticky) opts.push('sticky per wallet');
  if (t.exclude_creation_slot) opts.push('creation-slot buyers on neither side');
  const body = parts.length ? parts.join(', or ') : 'nothing yet';
  return `@${t.name} = trades by ${body}${opts.length ? ` (${opts.join('; ')})` : ''}.`;
}

/** Every error in the tags, each naming the tag (mirror of `validate_tags`). */
export function validateTags(tags: TagDef[], reg: StrategyRegistry | undefined): string[] {
  const errors: string[] = [];
  const builtin = reg?.tags.builtin.map((b) => b.name) ?? [];
  const markers = new Set(reg?.tags.markers.map((m) => m.name) ?? []);
  const seen = new Set<string>();
  for (const t of tags) {
    const at = `Tag \`${t.name || '(no name)'}\``;
    if (!TAG_NAME_RE.test(t.name)) errors.push(`${at}: a name is 1 to 24 characters of a-z, 0-9 and _`);
    if (builtin.includes(t.name)) errors.push(`${at} is a built-in wallet class; pick another name`);
    if (seen.has(t.name)) errors.push(`Two tags are named \`${t.name}\``);
    seen.add(t.name);
    if (usedMatchers(t).length === 0) errors.push(`${at} has no matcher, so no trade could carry it`);
    for (const s of t.match.ix_template ?? []) {
      if (s.trim() && !s.includes('|')) {
        errors.push(`${at}: \`${s}\` is not a template (program|CU|ATA|N|S|F); a bare program name goes under Program`);
      }
    }
    for (const k of ['ix_contains', 'ix_lacks'] as const) {
      for (const s of t.match[k] ?? []) {
        if (markers.size && !markers.has(s)) errors.push(`${at}: unknown ix marker \`${s}\``);
      }
    }
    for (const r of t.match.ix_shape ?? []) {
      if (r.cu_limit != null && r.cu_limit > MAX_TX_COMPUTE_UNITS) {
        errors.push(`${at}: cu_limit ${r.cu_limit} exceeds the ${MAX_TX_COMPUTE_UNITS} compute units a transaction may request`);
      }
    }
    const c = t.match.cluster;
    if (c) {
      if (!(Number.isInteger(c.min_prints) && c.min_prints >= 2)) {
        errors.push(`${at}: a cluster needs at least 2 prints (one trade is not a cluster)`);
      }
      if (!(Number.isInteger(c.sol_tol_pct) && c.sol_tol_pct >= 0 && c.sol_tol_pct <= 100)) {
        errors.push(`${at}: the cluster's SOL tolerance is a whole percent from 0 to 100`);
      }
    }
  }
  return errors;
}

/** Warning text when a tag pins a fee preset on an ix shape (engine `fee_pin_warning`):
 *  fee capture is forward-only, so a pinned entry matches no trade recorded before it. */
export function feePinWarning(tags: TagDef[]): string | null {
  const pinned = tags
    .filter((t) => (t.match.ix_shape ?? []).some((r) => r.cu_limit != null || r.cu_price != null || r.tip_lamports != null))
    .map((t) => t.name);
  return pinned.length
    ? `Tag ${pinned.join(', ')} pins a fee preset on an ix shape. Fee capture is forward-only, so those entries match no trade recorded before it: check against recent data.`
    : null;
}

// ── Staging: add one value to one tag, through the one writer ─────────────────

/** Whether `doc`'s tag `name` already lists `value` under a list matcher. */
export function tagListHas(doc: unknown, name: string, k: StringListMatcher, value: string): boolean {
  const t = tagsFromJson(doc).find((x) => x.name === name);
  return !!t && (t.match[k] ?? []).includes(value);
}

/** Whether `doc`'s tag `name` lists this ix shape (pins included in the identity). */
export function tagHasShape(doc: unknown, name: string, row: IxPatternRow): boolean {
  const t = tagsFromJson(doc).find((x) => x.name === name);
  const key = patternRowKey(row);
  return !!t && (t.match.ix_shape ?? []).some((r) => patternRowKey(r) === key);
}

/** The document with `value` added to (or, `remove`, taken from) tag `name`'s list
 *  matcher `k`. A missing tag is created. Everything else is kept as stored. */
export function withTagListValue(doc: unknown, name: string, k: StringListMatcher, value: string, remove = false): Obj {
  const tags = tagsFromJson(doc);
  let t = tags.find((x) => x.name === name);
  if (!t) {
    t = emptyTag(name);
    tags.push(t);
  }
  const list = t.match[k] ?? [];
  t.match[k] = remove ? list.filter((v) => v !== value) : list.includes(value) ? list : [...list, value];
  return tagsToJson(tags.filter((x) => usedMatchers(x).length > 0 || x.name !== name || !remove));
}

/** The document with one ix shape added to (or taken from) tag `name`. */
export function withTagShape(doc: unknown, name: string, row: IxPatternRow, remove = false): Obj {
  const tags = tagsFromJson(doc);
  let t = tags.find((x) => x.name === name);
  if (!t) {
    t = emptyTag(name);
    tags.push(t);
  }
  const key = patternRowKey(row);
  const rows = t.match.ix_shape ?? [];
  t.match.ix_shape = remove
    ? rows.filter((r) => patternRowKey(r) !== key)
    : rows.some((r) => patternRowKey(r) === key)
      ? rows
      : [...rows, row];
  return tagsToJson(tags.filter((x) => usedMatchers(x).length > 0 || x.name !== name || !remove));
}
