/**
 * Single localStorage layer for the app.
 *
 * Every persisted UI preference lives here: one namespaced key registry
 * ({@link STORAGE_KEYS}), one set of JSON/string accessors (so the
 * read-parse-fallback / write-stringify try/catch boilerplate isn't copied into
 * every page), and a one-time migration that folds the old flat keys into this
 * structure. All app keys share the `mt:` prefix, so the whole set is
 * greppable, enumerable, and collision-proof against third-party libs.
 *
 * Forward-compat for shape changes follows the codebase's existing pattern:
 * read into a merge-with-defaults function (see swing/chart/filters loaders),
 * not version-suffixed keys. When a key must be *renamed*, add a migration
 * below instead of leaving an orphaned `_v2` key behind.
 */

export const PREFIX = 'mt:';

/** The complete set of app localStorage keys (already prefixed). */
export const STORAGE_KEYS = {
  timezone: `${PREFIX}app.timezone`,
  priceUnit: `${PREFIX}app.priceUnit`,
  chartPrefs: `${PREFIX}chart.prefs`,
  tokenFilters: `${PREFIX}tokens.filters`,
  tokensLive: `${PREFIX}tokens.live`,
  swingCriteria: `${PREFIX}swing.criteria`,
  sweepConfig: `${PREFIX}sweep.config`,
  /** Map of `{ [tableId]: visibleColumnKey[] }` — all DataTable column toggles. */
  tableCols: `${PREFIX}table.cols`,
} as const;

// ── raw string accessors ────────────────────────────────────────────────────

export function getString(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

export function setString(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* ignore (private mode / quota) */
  }
}

export function remove(key: string): void {
  try {
    localStorage.removeItem(key);
  } catch {
    /* ignore */
  }
}

// ── JSON accessors ──────────────────────────────────────────────────────────

/** Parse the stored JSON at `key`, or return `fallback` if absent/unparseable. */
export function getJSON<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    if (raw != null) return JSON.parse(raw) as T;
  } catch {
    /* ignore */
  }
  return fallback;
}

export function setJSON(key: string, value: unknown): void {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* ignore */
  }
}

// ── table column-visibility map ─────────────────────────────────────────────
// Collapses what used to be ~10 separate `*_cols` keys into one object map.

type TableColsMap = Record<string, string[]>;

/** Visible-column ids saved for `tableId`, or `null` if none stored yet. */
export function getTableCols(tableId: string): string[] | null {
  const map = getJSON<TableColsMap>(STORAGE_KEYS.tableCols, {});
  const cols = map[tableId];
  return Array.isArray(cols) ? cols : null;
}

/** Persist the visible-column ids for `tableId` into the shared map. */
export function setTableCols(tableId: string, cols: string[]): void {
  const map = getJSON<TableColsMap>(STORAGE_KEYS.tableCols, {});
  map[tableId] = cols;
  setJSON(STORAGE_KEYS.tableCols, map);
}

// ── one-time migration of pre-namespace keys ────────────────────────────────

/** Old flat key → new namespaced key (value format unchanged, raw move). */
const RENAMES: [string, string][] = [
  ['app_timezone', STORAGE_KEYS.timezone],
  ['price_unit', STORAGE_KEYS.priceUnit],
  ['token_price_chart_prefs', STORAGE_KEYS.chartPrefs],
  ['tokens_global_filters', STORAGE_KEYS.tokenFilters],
  ['tokens_live', STORAGE_KEYS.tokensLive],
  ['swing_detection_criteria', STORAGE_KEYS.swingCriteria],
];

/** Old per-table `*_cols` key → logical tableId (folded into the cols map). */
const TABLE_COL_RENAMES: [string, string][] = [
  ['tokens_visible_cols', 'tokens'],
  ['wallet_visible_cols', 'wallet'],
  ['swing_detection_visible_cols_v2', 'swing'],
  ['tpsl1_rules_cols', 'tpsl1_rules'],
  ['tpsl2_rules_cols', 'tpsl2_rules'],
  ['tpsl2_paper_cols', 'tpsl2_paper'],
  ['tpsl2_matched_cols', 'tpsl2_matched'],
  ['tpsl2_sim_cols', 'tpsl2_sim'],
  ['grouped_sweep_group_cols_v3', 'sweep_groups'],
  ['grouped_sweep_combo_cols', 'sweep_combos'],
];

/** Old flat `sweep_cfg_*` key → field in the consolidated sweep.config object. */
const SWEEP_CFG_RENAMES: [string, string][] = [
  ['sweep_cfg_created_after', 'createdAfter'],
  ['sweep_cfg_created_before', 'createdBefore'],
  ['sweep_cfg_group_by', 'groupBy'],
  ['sweep_cfg_axes_text', 'axesText'],
  ['sweep_cfg_method', 'methodKind'],
  ['sweep_cfg_random_n', 'randomN'],
  ['sweep_cfg_min_tokens', 'minTokens'],
  ['sweep_cfg_token_cap', 'tokenCap'],
  ['sweep_cfg_curve_only', 'curveOnly'],
];

let migrated = false;

/**
 * Fold the pre-namespace localStorage keys into the structure above. Idempotent
 * and gated on "old exists, new absent", so it never clobbers a fresh value and
 * is a no-op on every load after the first. Runs once on module import.
 */
export function runStorageMigrations(): void {
  if (migrated) return;
  migrated = true;
  try {
    if (typeof localStorage === 'undefined') return;

    for (const [oldKey, newKey] of RENAMES) {
      const val = localStorage.getItem(oldKey);
      if (val != null && localStorage.getItem(newKey) == null) {
        localStorage.setItem(newKey, val);
      }
      if (val != null) localStorage.removeItem(oldKey);
    }

    // Column visibility → single map (only seed ids not already present).
    const colsMap = getJSON<TableColsMap>(STORAGE_KEYS.tableCols, {});
    let colsDirty = false;
    for (const [oldKey, tableId] of TABLE_COL_RENAMES) {
      const raw = localStorage.getItem(oldKey);
      if (raw == null) continue;
      if (!(tableId in colsMap)) {
        try {
          const parsed = JSON.parse(raw);
          if (Array.isArray(parsed)) {
            colsMap[tableId] = parsed as string[];
            colsDirty = true;
          }
        } catch {
          /* drop unparseable legacy value */
        }
      }
      localStorage.removeItem(oldKey);
    }
    if (colsDirty) setJSON(STORAGE_KEYS.tableCols, colsMap);

    // Sweep form → single config object.
    const cfg = getJSON<Record<string, unknown>>(STORAGE_KEYS.sweepConfig, {});
    let cfgDirty = false;
    for (const [oldKey, field] of SWEEP_CFG_RENAMES) {
      const raw = localStorage.getItem(oldKey);
      if (raw == null) continue;
      if (!(field in cfg)) {
        try {
          cfg[field] = JSON.parse(raw);
          cfgDirty = true;
        } catch {
          /* drop unparseable legacy value */
        }
      }
      localStorage.removeItem(oldKey);
    }
    if (cfgDirty) setJSON(STORAGE_KEYS.sweepConfig, cfg);
  } catch {
    /* never let a storage migration break app boot */
  }
}

// Run before any consumer reads (every persister imports this module).
runStorageMigrations();
