/**
 * Analysis-owned pattern sets — the wire types of `ix_pattern_sets` plus the
 * paste parser that fills one.
 *
 * A fingerprint's lists are what the ENGINE classifies with; a pattern set is
 * the same fact owned by a study surface, for tokens that belong to no cohort
 * (Trader Analysis). A set is **one vocabulary**, chosen at create:
 *
 * * `exact` — ordered `ix_labels` plus optional fee pins. Overlay match is
 *   `'labels'` (same as tagged/dump). `group` labels a subset for narrowing.
 * * `templates` — grain ids (`program|CU|ATA|N|S|F`). Overlay match is
 *   `'grain'` (same as working). No fee pins.
 *
 * Both feed the ONE classifier (`lib/flow/classifyFlow`) via `classifyOptsForTape`.
 */

import { patternKeysFrom } from 'lib/flow/classifyFlow';
import type { TapeList } from 'lib/strategy/registry';
import { parseGrainIds } from 'lib/strategy/templateGrain';
import {
  FEE_FIELDS,
  patternKey,
  patternRowKey,
  togglePatternRow,
  type IxPatternFee,
  type IxPatternRow,
} from 'lib/strategy/ixPatternRows';

/** Insert-only vocabulary. The set picker is the switch; kind never changes. */
export type IxPatternSetKind = 'exact' | 'templates';

export interface IxPattern extends IxPatternFee {
  /** Subset label; `null` ⇒ ungrouped. Display + narrowing only. Exact sets. */
  group: string | null;
  /** EXACT ordered instruction labels, verbatim from `trades.ix_labels`. */
  ix_labels: string[];
}

export interface IxPatternSet {
  id: string;
  name: string;
  wallet_address: string | null;
  kind: IxPatternSetKind;
  patterns: IxPattern[];
  working_templates: string[];
  notes: string | null;
  created_at: string;
  updated_at: string;
}

/** Body of `POST /api/ix-pattern-sets` and its `PUT` twin (full replace).
 *  Kind on PUT is ignored — insert-only on the server. */
export interface IxPatternSetDraft {
  name: string;
  wallet_address?: string | null;
  kind?: IxPatternSetKind;
  patterns: IxPattern[];
  working_templates?: string[];
  notes?: string | null;
}

/** Bucket name shown for `group: null`. Display only — never persisted. */
export const UNGROUPED = 'Ungrouped';

/** Identity of one ordered sequence — labels only, no pins. */
export const patternKeyOf = (labels: readonly string[]): string => JSON.stringify(labels);

export function kindOf(set: { kind?: IxPatternSetKind } | null | undefined): IxPatternSetKind {
  return set?.kind === 'templates' ? 'templates' : 'exact';
}

/** Overlay list this set's kind classifies as. Never dump. */
export function tapeListForKind(kind: IxPatternSetKind): TapeList {
  return kind === 'templates' ? 'working' : 'tagged';
}

export function toPatternRow(p: IxPattern): IxPatternRow {
  const row: IxPatternRow = { labels: [...p.ix_labels] };
  for (const f of FEE_FIELDS) {
    if (p[f] != null) row[f] = p[f];
  }
  return row;
}

export function fromPatternRow(row: IxPatternRow, group: string | null): IxPattern {
  const p: IxPattern = { group, ix_labels: [...row.labels] };
  for (const f of FEE_FIELDS) {
    if (row[f] != null) p[f] = row[f];
  }
  return p;
}

/** Group names in first-seen order, with {@link UNGROUPED} standing in for null. */
export function patternGroups(patterns: readonly IxPattern[]): string[] {
  const out: string[] = [];
  const seen = new Set<string>();
  for (const p of patterns) {
    const g = p.group ?? UNGROUPED;
    if (!seen.has(g)) {
      seen.add(g);
      out.push(g);
    }
  }
  return out;
}

function pickedPatterns(
  patterns: readonly IxPattern[],
  enabled: ReadonlySet<string> | null,
): readonly IxPattern[] {
  return enabled ? patterns.filter((p) => enabled.has(p.group ?? UNGROUPED)) : patterns;
}

/**
 * Classification keys for the enabled groups only — how the lens narrows to one
 * launch client without touching the stored set. `null` ⇒ every group.
 * Keys are labels-only (fee match uses {@link patternRowsForGroups}).
 */
export function patternKeysForGroups(
  patterns: readonly IxPattern[],
  enabled: ReadonlySet<string> | null,
): ReadonlySet<string> | null {
  const keys = patternKeysFrom(pickedPatterns(patterns, enabled).map((p) => p.ix_labels));
  return keys.size > 0 ? keys : null;
}

/** Whole rows for overlay match (an unpinned row is a fee wildcard). */
export function patternRowsForGroups(
  patterns: readonly IxPattern[],
  enabled: ReadonlySet<string> | null,
): IxPatternRow[] | null {
  const rows = pickedPatterns(patterns, enabled).map(toPatternRow);
  return rows.length > 0 ? rows : null;
}

/** Overlay key set for the selected set's kind. */
export function keysForSet(
  set: Pick<IxPatternSet, 'kind' | 'patterns' | 'working_templates'>,
  enabled: ReadonlySet<string> | null,
): ReadonlySet<string> | null {
  if (kindOf(set) === 'templates') {
    const grains = set.working_templates ?? [];
    return grains.length > 0 ? new Set(grains) : null;
  }
  return patternKeysForGroups(set.patterns, enabled);
}

/** Add/remove one exact row (labels + optional pins), keeping groups. */
export function toggleExactPattern(
  patterns: readonly IxPattern[],
  labels: readonly string[],
  fee: IxPatternFee | undefined,
  activeGroup: string | null,
): IxPattern[] {
  const rows = patterns.map(toPatternRow);
  const nextRow: IxPatternRow = { labels: [...labels] };
  if (fee) {
    for (const f of FEE_FIELDS) {
      if (fee[f] != null) nextRow[f] = fee[f];
    }
  }
  const next = togglePatternRow(rows, nextRow);
  const groupByRowKey = new Map(
    patterns.map((p) => [patternRowKey(toPatternRow(p)), p.group] as const),
  );
  const groupByShape = new Map<string, string | null>();
  for (const p of patterns) {
    const k = patternKey(p.ix_labels);
    if (!groupByShape.has(k)) groupByShape.set(k, p.group);
  }
  return next.map((row) => {
    const rowKey = patternRowKey(row);
    const shape = patternKey(row.labels);
    const group = groupByRowKey.has(rowKey)
      ? (groupByRowKey.get(rowKey) ?? null)
      : groupByShape.has(shape)
        ? (groupByShape.get(shape) ?? null)
        : activeGroup;
    return fromPatternRow(row, group);
  });
}

/** Add/remove one ordered sequence as an unpinned row. */
export function toggleIxPattern(
  patterns: readonly IxPattern[],
  labels: readonly string[],
  group: string | null,
): IxPattern[] {
  return toggleExactPattern(patterns, labels, undefined, group);
}

export interface PatternParseResult {
  patterns: IxPattern[];
  /** Sequences kept (post-dedupe). */
  accepted: number;
  /** Sequences dropped as an exact duplicate of an earlier one. */
  duplicates: number;
  /** Entries that carried no usable `ix_labels` array. */
  skipped: number;
  /** Fatal — nothing was parsed. `patterns` is then empty. */
  error: string | null;
}

/** One entry of a parsed payload, before dedupe. */
interface RawEntry {
  group: string | null;
  labels: string[];
  fee: IxPatternFee;
}

const isStringArray = (v: unknown): v is string[] =>
  Array.isArray(v) && v.every((x) => typeof x === 'string');

function feeOf(o: Record<string, unknown>): IxPatternFee {
  const fee: IxPatternFee = {};
  for (const f of FEE_FIELDS) {
    const v = o[f];
    if (typeof v === 'number' && Number.isInteger(v) && v >= 0) fee[f] = v;
  }
  return fee;
}

function feeKey(fee: IxPatternFee): string {
  return FEE_FIELDS.map((f) => (fee[f] != null ? `${f}:${fee[f]}` : '')).join('|');
}

function entryKey(e: RawEntry): string {
  return `${patternKeyOf(e.labels)}\u{2}${feeKey(e.fee)}`;
}

/** Group name off an object entry — `group` is ours, `tool` is what a derived
 *  study file usually calls it, and `label`/`name` are the other two spellings
 *  seen in hand-written JSON. */
function groupOf(o: Record<string, unknown>): string | null {
  for (const k of ['group', 'tool', 'label', 'name']) {
    const v = o[k];
    if (typeof v === 'string' && v.trim()) return v.trim();
  }
  return null;
}

/** Pull entries out of any of the accepted JSON shapes; `null` ⇒ not one. */
function entriesFromJson(value: unknown): RawEntry[] | null {
  // `{ "patterns": [...] }` — a derived study file, notes and counts included.
  if (value && typeof value === 'object' && !Array.isArray(value)) {
    const inner = (value as Record<string, unknown>).patterns;
    return inner === undefined ? null : entriesFromJson(inner);
  }
  if (!Array.isArray(value)) return null;
  // A single sequence pasted bare.
  if (isStringArray(value)) return [{ group: null, labels: value, fee: {} }];
  const out: RawEntry[] = [];
  for (const item of value) {
    if (isStringArray(item)) {
      out.push({ group: null, labels: item, fee: {} });
      continue;
    }
    if (item && typeof item === 'object') {
      const o = item as Record<string, unknown>;
      const labels = o.ix_labels ?? o.instruction_labels ?? o.labels;
      if (isStringArray(labels)) {
        out.push({ group: groupOf(o), labels, fee: feeOf(o) });
        continue;
      }
    }
    // Anything else is counted as skipped by the caller (length difference).
    out.push({ group: null, labels: [], fee: {} });
  }
  return out;
}

/**
 * Parse a pasted exact-pattern payload.
 *
 * Accepts, in order: a `{ "patterns": [...] }` wrapper, an array of
 * `{ ix_labels, group|tool, cu_limit? }` objects, an array of label arrays, one
 * bare label array, or one JSON array per line. Deliberately does NOT accept the
 * `"A > B > C"` display form: those are shortened ACTION names, and a pattern
 * built from them would match no trade while looking correct.
 */
export function parsePastedPatterns(text: string): PatternParseResult {
  const trimmed = text.trim();
  const empty: PatternParseResult = {
    patterns: [],
    accepted: 0,
    duplicates: 0,
    skipped: 0,
    error: null,
  };
  if (!trimmed) return empty;

  let entries: RawEntry[] | null = null;
  try {
    entries = entriesFromJson(JSON.parse(trimmed) as unknown);
  } catch {
    // Not one JSON document — try one array per line.
    const lines = trimmed.split(/\r?\n/).map((l) => l.trim().replace(/,$/, ''));
    const perLine: RawEntry[] = [];
    for (const line of lines) {
      if (!line) continue;
      try {
        const parsed: unknown = JSON.parse(line);
        const one = entriesFromJson(parsed);
        if (one) perLine.push(...one);
        else perLine.push({ group: null, labels: [], fee: {} });
      } catch {
        return {
          ...empty,
          error:
            'Not JSON. Paste [["Label A","Label B"], …], a [{ "tool": …, "ix_labels": […] }] list, or one JSON array per line — the "A > B" display form is lossy and cannot be matched.',
        };
      }
    }
    entries = perLine;
  }

  if (!entries) {
    return {
      ...empty,
      error: 'JSON parsed, but carried no ix_labels — expected arrays of label strings.',
    };
  }

  const seen = new Set<string>();
  const patterns: IxPattern[] = [];
  let duplicates = 0;
  let skipped = 0;
  for (const e of entries) {
    const labels = e.labels.map((l) => l.trim()).filter(Boolean);
    if (labels.length === 0) {
      skipped += 1;
      continue;
    }
    const key = entryKey({ ...e, labels });
    if (seen.has(key)) {
      duplicates += 1;
      continue;
    }
    seen.add(key);
    patterns.push({ group: e.group, ix_labels: labels, ...e.fee });
  }

  if (patterns.length === 0 && skipped === 0 && duplicates === 0) {
    return { ...empty, error: 'No patterns found in that paste.' };
  }
  return { patterns, accepted: patterns.length, duplicates, skipped, error: null };
}

export interface GrainParseResult {
  grains: string[];
  accepted: number;
  duplicates: number;
  error: string | null;
}

/**
 * Parse pasted working-template grain ids. Accepts a JSON string array, or
 * newline/comma-separated ids. Rejects ix_labels payloads so a templates lens
 * cannot silently store exact sequences as fake grains.
 */
export function parsePastedGrains(text: string): GrainParseResult {
  const trimmed = text.trim();
  const empty: GrainParseResult = { grains: [], accepted: 0, duplicates: 0, error: null };
  if (!trimmed) return empty;

  try {
    const parsed: unknown = JSON.parse(trimmed);
    if (Array.isArray(parsed) && parsed.every((x) => typeof x === 'string')) {
      const grains = parseGrainIds(parsed.join('\n'));
      const raw = parsed.map((s) => s.trim()).filter(Boolean);
      return {
        grains,
        accepted: grains.length,
        duplicates: Math.max(0, raw.length - grains.length),
        error: grains.length === 0 ? 'No grain ids in that JSON array.' : null,
      };
    }
    if (parsed !== undefined) {
      return {
        ...empty,
        error:
          'This set is templates — paste grain ids (program|CU|ATA|N|S|F), not ix_labels sequences.',
      };
    }
  } catch {
    // not JSON — newline/comma list
  }

  const grains = parseGrainIds(trimmed);
  if (grains.length === 0) {
    return {
      ...empty,
      error: 'No grain ids found. Paste program|CU|ATA|N|S|F ids, one per line or as a JSON string array.',
    };
  }
  const rawCount = trimmed.split(/[\n,]+/).map((s) => s.trim()).filter(Boolean).length;
  return {
    grains,
    accepted: grains.length,
    duplicates: Math.max(0, rawCount - grains.length),
    error: null,
  };
}

/** The set as re-pastable JSON, groups and pins preserved. */
export function formatPatternsJson(patterns: readonly IxPattern[]): string {
  return JSON.stringify(
    patterns.map((p) => {
      const pinned = FEE_FIELDS.some((f) => p[f] != null);
      if (!p.group && !pinned) return p.ix_labels;
      const out: Record<string, unknown> = { ix_labels: p.ix_labels };
      if (p.group) out.tool = p.group;
      for (const f of FEE_FIELDS) {
        if (p[f] != null) out[f] = p[f];
      }
      return out;
    }),
    null,
    2,
  );
}

/** Bare label arrays — groups and pins drop. Prefer {@link toPatternRow} when
 *  promoting into a fingerprint so fees survive. */
export function toIxPatterns(patterns: readonly IxPattern[]): string[][] {
  return patterns.map((p) => [...p.ix_labels]);
}
