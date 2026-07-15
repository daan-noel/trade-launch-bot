/**
 * Single localStorage layer for the app.
 *
 * Every persisted UI preference lives here: one namespaced key registry
 * ({@link STORAGE_KEYS}), one set of JSON/string accessors (so the
 * read-parse-fallback / write-stringify try/catch boilerplate isn't copied into
 * every page), and a one-time cleanup ({@link cleanupLegacyStorage}) that purges
 * the old flat keys this structure replaced. All app keys share the `mt:`
 * prefix, so the whole set is greppable, enumerable, and collision-proof against
 * third-party libs.
 *
 * Forward-compat for shape changes follows the codebase's existing pattern:
 * read into a merge-with-defaults function (see swing/chart/filters loaders),
 * not version-suffixed keys.
 */

const PREFIX = 'mt:';

/** The complete set of app localStorage keys (already prefixed). */
export const STORAGE_KEYS = {
  timezone: `${PREFIX}app.timezone`,
  priceUnit: `${PREFIX}app.priceUnit`,
  chartPrefs: `${PREFIX}chart.prefs`,
  tokenFilters: `${PREFIX}tokens.filters`,
  tokensLive: `${PREFIX}tokens.live`,
  swingCriteria: `${PREFIX}swing.criteria`,
  sweepConfig: `${PREFIX}sweep.config`,
  /** Base key for persisted sweep run selection; append `.${strategyId}` for per-strategy key. */
  sweepSel: `${PREFIX}sweep.sel`,
  /** Map of `{ [tableId]: visibleColumnKey[] }` — all DataTable column toggles. */
  tableCols: `${PREFIX}table.cols`,
  /** Map of `{ [tableId]: { pageSize, sortKeys } }` — DataTable sort + page-size. */
  tablePrefs: `${PREFIX}table.prefs`,
  /** Notification preferences (real/paper toggles, status filter, fp-param display). */
  notificationPrefs: `${PREFIX}notifications`,
  /** Base key for the per-strategy-page "show pending positions" toggle; append `.${pageId}` for a per-page key. */
  showPending: `${PREFIX}strategy.showPending`,
  // Dashboard page controls
  dashboardMetric: `${PREFIX}dashboard.metric`,
  dashboardSegment: `${PREFIX}dashboard.segment`,
  dashboardBucket: `${PREFIX}dashboard.bucket`,
  dashboardRange: `${PREFIX}dashboard.range`,
  // Grouped creation section draft controls
  groupedBy: `${PREFIX}dashboard.grouped.by`,
  groupedTop: `${PREFIX}dashboard.grouped.top`,
  groupedBucketWidth: `${PREFIX}dashboard.grouped.bucketWidth`,
  groupedBucket: `${PREFIX}dashboard.grouped.bucket`,
  groupedRange: `${PREFIX}dashboard.grouped.range`,
  groupedFilters: `${PREFIX}dashboard.grouped.filters`,
  groupedCashback: `${PREFIX}dashboard.grouped.cashback`,
  groupedIxLabels: `${PREFIX}dashboard.grouped.ixLabels`,
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

// ── table sort + page-size preferences ─────────────────────────────────────

export interface TablePrefs {
  pageSize?: number;
  /** Multi-key sort; index 0 = primary. */
  sortKeys?: { col: string; dir: 'asc' | 'desc' }[];
}

type TablePrefsMap = Record<string, TablePrefs>;

/** Sort/page-size preferences saved for `tableId`, or `{}` if none stored yet. */
export function getTablePrefs(tableId: string): TablePrefs {
  const map = getJSON<TablePrefsMap>(STORAGE_KEYS.tablePrefs, {});
  return map[tableId] ?? {};
}

/** Persist sort/page-size preferences for `tableId` into the shared map. */
export function setTablePrefs(tableId: string, prefs: TablePrefs): void {
  const map = getJSON<TablePrefsMap>(STORAGE_KEYS.tablePrefs, {});
  map[tableId] = prefs;
  setJSON(STORAGE_KEYS.tablePrefs, map);
}

// ── one-time cleanup of pre-namespace keys ──────────────────────────────────
// Everything the app persists now lives under the `mt:` prefix and the
// consolidated objects above (table.cols, sweep.config, swing.criteria, …).
// Earlier builds wrote ~25 flat keys: per-table `*_cols`, per-field
// `sweep_cfg_*`, and a handful of singletons. None are read anymore, so they're
// dead weight in localStorage — purge them once so the store only ever holds the
// `mt:` set. Pattern-matched (not an exact list) so renamed/versioned variants
// (`*_v2`/`*_v3`, peak_trough→swing, the pre-split `tpsl_rules_cols`, …) are all
// caught without per-key bookkeeping, while non-`mt:` third-party keys are left
// untouched.

/** Pre-namespace singleton keys (no shared suffix to pattern-match on). */
const LEGACY_SINGLETON_KEYS = [
  'app_timezone',
  'price_unit',
  'token_price_chart_prefs',
  'tokens_global_filters',
  'tokens_live',
  'swing_detection_criteria',
];

/** True for any flat legacy app key — a `*_cols` toggle, a `sweep_cfg_*` field,
 *  or a known singleton. `mt:`-prefixed keys are current and never match. */
function isLegacyStorageKey(key: string): boolean {
  if (key.startsWith(PREFIX)) return false;
  return (
    key.endsWith('_cols') ||
    key.startsWith('sweep_cfg_') ||
    LEGACY_SINGLETON_KEYS.includes(key)
  );
}

let cleaned = false;

/**
 * Remove every pre-namespace app key from localStorage. Idempotent (a no-op once
 * the flat keys are gone) and wrapped so a storage failure never breaks boot.
 * Runs once on module import, before any consumer reads.
 */
export function cleanupLegacyStorage(): void {
  if (cleaned) return;
  cleaned = true;
  try {
    if (typeof localStorage === 'undefined') return;
    // Snapshot keys first — removing while iterating by index skips entries.
    const stale: string[] = [];
    for (let i = 0; i < localStorage.length; i++) {
      const key = localStorage.key(i);
      if (key && isLegacyStorageKey(key)) stale.push(key);
    }
    for (const key of stale) localStorage.removeItem(key);
  } catch {
    /* never let a storage cleanup break app boot */
  }
}

// Run before any consumer reads (every persister imports this module).
cleanupLegacyStorage();
