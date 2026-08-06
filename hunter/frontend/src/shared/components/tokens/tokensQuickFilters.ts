/**
 * Page-owned Tokens quick filters (Created range + Dead / Migrated). Domain
 * chrome for the All Tokens page — NOT part of DataTable. Column filters cover
 * everything else; this bar only keeps the controls that are awkward as raw
 * per-column text (datetime range + common flag cuts).
 */
import type { FilterSpec } from 'components/table/numericFilter';
import { STORAGE_KEYS, getString, setString, remove } from 'lib/storage';
import { datetimeLocalToUtcWallClock } from 'utils/date';

export type TriState = '' | 'yes' | 'no';

export interface TokensQuickFilters {
  /** Wall-clock `YYYY-MM-DDTHH:mm` in the project timezone (picker wire). */
  created_from: string;
  created_to: string;
  dead: TriState;
  migrated: TriState;
}

const LS_KEY = STORAGE_KEYS.tokenFilters;
const TRI = new Set<TriState>(['', 'yes', 'no']);

export function defaultQuickFilters(): TokensQuickFilters {
  return { created_from: '', created_to: '', dead: '', migrated: '' };
}

function merge(partial?: Partial<TokensQuickFilters>): TokensQuickFilters {
  const base = defaultQuickFilters();
  if (!partial || typeof partial !== 'object') return base;
  if (typeof partial.created_from === 'string') base.created_from = partial.created_from;
  if (typeof partial.created_to === 'string') base.created_to = partial.created_to;
  if (TRI.has(partial.dead as TriState)) base.dead = partial.dead as TriState;
  if (TRI.has(partial.migrated as TriState)) base.migrated = partial.migrated as TriState;
  return base;
}

export function loadStoredQuickFilters(): TokensQuickFilters {
  try {
    const raw = getString(LS_KEY);
    if (!raw) return defaultQuickFilters();
    // Old mega-panel JSON is fine — `merge` only lifts the four quick keys.
    return merge(JSON.parse(raw) as Partial<TokensQuickFilters>);
  } catch {
    return defaultQuickFilters();
  }
}

export function saveStoredQuickFilters(f: TokensQuickFilters): void {
  if (quickFiltersEmpty(f)) remove(LS_KEY);
  else setString(LS_KEY, JSON.stringify(f));
}

export function quickFiltersEmpty(f: TokensQuickFilters): boolean {
  return !f.created_from && !f.created_to && !f.dead && !f.migrated;
}

export function activeQuickFilterCount(f: TokensQuickFilters): number {
  return [
    f.created_from || f.created_to,
    f.dead,
    f.migrated,
  ].filter(Boolean).length;
}

/**
 * Fold quick filters into the unified `FilterSpec` map (backend column keys).
 * Datetime bounds are normalized from project-tz wall-clock → UTC instant.
 */
export function quickFiltersToSpecs(
  f: TokensQuickFilters,
  timezone: string,
): Record<string, FilterSpec> {
  const out: Record<string, FilterSpec> = {};
  const lo = f.created_from ? datetimeLocalToUtcWallClock(f.created_from, timezone, 'lower') : '';
  const hi = f.created_to ? datetimeLocalToUtcWallClock(f.created_to, timezone, 'upper') : '';
  if (lo && hi) out.created = { op: 'between', min: lo, max: hi };
  else if (lo) out.created = { op: 'gte', val: lo };
  else if (hi) out.created = { op: 'lte', val: hi };

  if (f.dead === 'yes' || f.dead === 'no') out.dead = { op: 'eq', val: f.dead };
  if (f.migrated === 'yes' || f.migrated === 'no') out.migrated = { op: 'eq', val: f.migrated };
  return out;
}
