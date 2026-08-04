// Rule tags — free-form labels on `strategy_rules`, used to slice the Rules
// board. Presentational only: a tag never affects arming or execution (that is
// `is_enabled` / Active-Idle, deliberately orthogonal).
//
// **The server owns tag shape.** `trading_core::strategies::rules::normalize_tags`
// is the SSOT (lowercase, `-`-separated, `[a-z0-9:-]`, deduped, sorted) and this
// file deliberately does NOT reimplement that grammar — `tagInputToChip` only
// trims + lowercases so the chip you type looks roughly like what will be saved,
// and every surface re-renders from the mutation response. See
// `docs/plans/strategies/rule-tags.md`.

import type { CSSProperties } from 'react';
import { chipColorsFromHue, hashHue } from './metricColors';

/** Server caps (`MAX_TAGS_PER_RULE` / `MAX_TAG_LEN`) — mirrored for the input's
 *  soft warning only. A mismatch just means the server 400s, which the editor
 *  already surfaces; it is not a second validator. */
export const RULE_TAG_LIMIT = 8;
export const RULE_TAG_MAX_LEN = 24;

/** Cosmetic-only shaping for the editor's chip input. NOT the canonical grammar. */
export function tagInputToChip(raw: string): string {
  return raw.trim().toLowerCase();
}

/** The `fam:` in `fam:scalper` — used to group chips in the filter bar. */
export function tagNamespace(tag: string): string | null {
  const i = tag.indexOf(':');
  return i > 0 ? tag.slice(0, i) : null;
}

/** Ready-to-spread chip colors. Same hue for the same tag on every surface. */
export function tagChipStyle(tag: string): CSSProperties {
  return chipColorsFromHue(hashHue(tag), 62, 60).style;
}

// ── Filter state ────────────────────────────────────────────────────────────

/**
 * Tri-state chip selection. Include chips OR together (an empty `include` means
 * "no include constraint"); exclude chips AND together — a rule carrying ANY
 * excluded tag is hidden, and exclude wins over include.
 */
export interface TagFilterState {
  include: string[];
  exclude: string[];
}

export const EMPTY_TAG_FILTER: TagFilterState = { include: [], exclude: [] };

export type TagChipState = 'off' | 'include' | 'exclude';

export function tagChipState(filter: TagFilterState, tag: string): TagChipState {
  if (filter.exclude.includes(tag)) return 'exclude';
  if (filter.include.includes(tag)) return 'include';
  return 'off';
}

/** One click: off → include → exclude → off. */
export function cycleTag(filter: TagFilterState, tag: string): TagFilterState {
  const without = {
    include: filter.include.filter((t) => t !== tag),
    exclude: filter.exclude.filter((t) => t !== tag),
  };
  switch (tagChipState(filter, tag)) {
    case 'off':
      return { ...without, include: [...without.include, tag] };
    case 'include':
      return { ...without, exclude: [...without.exclude, tag] };
    default:
      return without;
  }
}

/** Force a tag to `include` (row-chip click — a shortcut, not a cycle). */
export function includeOnly(tag: string): TagFilterState {
  return { include: [tag], exclude: [] };
}

export function isEmptyTagFilter(f: TagFilterState): boolean {
  return f.include.length === 0 && f.exclude.length === 0;
}

/** Does a rule's tag set survive the filter? Untagged rules pass unless an
 *  include chip is active (nothing to match). */
export function matchesTagFilter(tags: string[] | undefined, f: TagFilterState): boolean {
  const has = tags ?? [];
  if (f.exclude.some((t) => has.includes(t))) return false;
  if (f.include.length === 0) return true;
  return f.include.some((t) => has.includes(t));
}

// ── Catalog ─────────────────────────────────────────────────────────────────

export interface TagCount {
  tag: string;
  count: number;
}

/**
 * Every tag present in `rules` with its rule count, ordered namespaced-first then
 * alphabetically — so `fam:*` chips sit together and bare labels trail them.
 */
export function tagCounts(rules: { tags?: string[] }[]): TagCount[] {
  const counts = new Map<string, number>();
  for (const r of rules) {
    for (const t of r.tags ?? []) counts.set(t, (counts.get(t) ?? 0) + 1);
  }
  return [...counts.entries()]
    .map(([tag, count]) => ({ tag, count }))
    .sort((a, b) => {
      const na = tagNamespace(a.tag);
      const nb = tagNamespace(b.tag);
      if (na !== nb) {
        if (na === null) return 1;
        if (nb === null) return -1;
        return na.localeCompare(nb);
      }
      return a.tag.localeCompare(b.tag);
    });
}

/** Distinct tags across rules — the editor's autocomplete source. */
export function allTags(rules: { tags?: string[] }[]): string[] {
  return tagCounts(rules).map((t) => t.tag);
}

// ── URL codec ───────────────────────────────────────────────────────────────

/** `?tags=` / `?notags=`. Comma is safe as the separator: the server grammar
 *  strips commas, so no canonical tag can contain one. */
export const TAG_PARAMS = { include: 'tags', exclude: 'notags' } as const;

function splitParam(raw: string | null): string[] {
  if (!raw) return [];
  return [...new Set(raw.split(',').map((s) => s.trim()).filter(Boolean))];
}

export function parseTagFilter(params: URLSearchParams): TagFilterState {
  return {
    include: splitParam(params.get(TAG_PARAMS.include)),
    exclude: splitParam(params.get(TAG_PARAMS.exclude)),
  };
}

/** Serialize for the URL / localStorage. Empty sides are omitted entirely so a
 *  cleared filter leaves no stale param behind. */
export function serializeTagFilter(f: TagFilterState): Record<string, string> {
  const out: Record<string, string> = {};
  if (f.include.length) out[TAG_PARAMS.include] = f.include.join(',');
  if (f.exclude.length) out[TAG_PARAMS.exclude] = f.exclude.join(',');
  return out;
}
