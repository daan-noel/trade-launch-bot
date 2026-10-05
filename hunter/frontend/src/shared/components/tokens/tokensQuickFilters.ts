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

/** Created-range shortcut. `1d` is a rolling last-24h look-back (the page
 *  default). `all` is an explicit open range. `custom` reads the wall-clock
 *  bounds. */
export type CreatedPreset = '1d' | 'all' | 'custom';

export interface TokensQuickFilters {
  created_preset: CreatedPreset;
  /** Wall-clock `YYYY-MM-DDTHH:mm` in the project timezone (picker wire).
   *  Read only when `created_preset === 'custom'`. */
  created_from: string;
  created_to: string;
  dead: TriState;
  migrated: TriState;
}

const LS_KEY = STORAGE_KEYS.tokenFilters;
const TRI = new Set<TriState>(['', 'yes', 'no']);

const CREATED_PRESETS = new Set<CreatedPreset>(['1d', 'all', 'custom']);

/** Page default: tokens created in the last day. Absent storage reads as this. */
export function defaultQuickFilters(): TokensQuickFilters {
  return {
    created_preset: '1d',
    created_from: '',
    created_to: '',
    dead: '',
    migrated: '',
  };
}

/** Every quick filter off — the Clear action. Distinct from the 1-day default
 *  so an explicit "any time" survives a reload. */
export function clearedQuickFilters(): TokensQuickFilters {
  return { ...defaultQuickFilters(), created_preset: 'all' };
}

function merge(partial?: Partial<TokensQuickFilters>): TokensQuickFilters {
  // A stored blob from before `created_preset` meant an open created range.
  const base = clearedQuickFilters();
  if (!partial || typeof partial !== 'object') return base;
  if (typeof partial.created_from === 'string') base.created_from = partial.created_from;
  if (typeof partial.created_to === 'string') base.created_to = partial.created_to;
  if (TRI.has(partial.dead as TriState)) base.dead = partial.dead as TriState;
  if (TRI.has(partial.migrated as TriState)) base.migrated = partial.migrated as TriState;
  if (CREATED_PRESETS.has(partial.created_preset as CreatedPreset)) {
    base.created_preset = partial.created_preset as CreatedPreset;
  } else if (base.created_from || base.created_to) {
    base.created_preset = 'custom';
  }
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

export function isDefaultQuickFilters(f: TokensQuickFilters): boolean {
  return f.created_preset === '1d' && !f.dead && !f.migrated;
}

export function saveStoredQuickFilters(f: TokensQuickFilters): void {
  // Dropping the key is how the next visit returns to the 1-day default.
  if (isDefaultQuickFilters(f)) remove(LS_KEY);
  else setString(LS_KEY, JSON.stringify(f));
}

export function quickFiltersEmpty(f: TokensQuickFilters): boolean {
  return f.created_preset === 'all' && !f.created_from && !f.created_to && !f.dead && !f.migrated;
}

export function activeQuickFilterCount(f: TokensQuickFilters): number {
  const created =
    f.created_preset === '1d' ||
    (f.created_preset === 'custom' && !!(f.created_from || f.created_to));
  return [created, f.dead, f.migrated].filter(Boolean).length;
}

/** Hour-floored UTC wall-clock (`YYYY-MM-DDTHH:mm:ss`, no `Z`) for "now minus
 *  one day". The tokens API reads that shape as UTC. Floored so the query key
 *  stays put inside the hour. */
export function rollingOneDayFromUtc(now = new Date()): string {
  const d = new Date(now);
  d.setMinutes(0, 0, 0);
  d.setHours(d.getHours() - 24);
  return d.toISOString().slice(0, 19);
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
  if (f.created_preset === '1d') {
    out.created = { op: 'gte', val: rollingOneDayFromUtc() };
  } else if (f.created_preset === 'custom') {
    const lo = f.created_from ? datetimeLocalToUtcWallClock(f.created_from, timezone, 'lower') : '';
    const hi = f.created_to ? datetimeLocalToUtcWallClock(f.created_to, timezone, 'upper') : '';
    if (lo && hi) out.created = { op: 'between', min: lo, max: hi };
    else if (lo) out.created = { op: 'gte', val: lo };
    else if (hi) out.created = { op: 'lte', val: hi };
  }

  if (f.dead === 'yes' || f.dead === 'no') out.dead = { op: 'eq', val: f.dead };
  if (f.migrated === 'yes' || f.migrated === 'no') out.migrated = { op: 'eq', val: f.migrated };
  return out;
}
