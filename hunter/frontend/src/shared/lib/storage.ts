/**
 * Single localStorage layer for the app.
 *
 * Every persisted UI preference lives here: one namespaced key registry
 * ({@link STORAGE_KEYS}), one set of JSON/string/blob-field accessors (so the
 * read-parse-fallback / write-stringify try/catch boilerplate isn't copied into
 * every page), and a one-time {@link migrateLegacyStorage} that folds the old
 * flat keys into the current shapes and purges what it replaced. All app keys
 * share the `mt:` prefix, so the whole set is greppable, enumerable, and
 * collision-proof against third-party libs.
 *
 * Rules (see `docs/plans/frontend/frontend-patterns.md`):
 *  - **One gate.** Components never touch `localStorage` directly — go through
 *    this module or `hooks/useLocalStorage`. The one exception is
 *    `live/lib/desktopNotify.ts` (an ephemeral cross-tab claim stamp, not a pref).
 *  - **One prefix.** Everything durable is `mt:`.
 *  - **Registry = truth.** Every durable key appears in {@link STORAGE_KEYS}.
 *  - **Few blobs, not many flat keys.** Related prefs group into one object
 *    (`ui.accordion`, `ui.toggles`, `table.*`, `page.creationStats`, form drafts).
 *  - **Merge with defaults, never `_v2` keys.** A shape change reads old, merges
 *    over defaults, writes new.
 */

const PREFIX = 'mt:';

/** The complete set of app localStorage keys (already prefixed). */
export const STORAGE_KEYS = {
  // ── app / global ──────────────────────────────────────────────────────────
  timezone: `${PREFIX}app.timezone`,
  priceUnit: `${PREFIX}app.priceUnit`,
  chartPrefs: `${PREFIX}chart.prefs`,
  /** Map of `{ [chartId]: boolean }` — compact-chrome Tools panel open/closed. */
  chartToolbarOpen: `${PREFIX}chart.toolbarOpen`,
  /** Notification preferences (real/paper toggles, status filter, fp-param display). */
  notificationPrefs: `${PREFIX}notifications`,
  /** Map of `{ [accordionId]: boolean }` — every collapsible chrome panel.
   *  Ids come from {@link ACCORDION_IDS}; read/write via `useAccordionOpen`. */
  uiAccordion: `${PREFIX}ui.accordion`,
  /** Object of app-wide show/hide switches — see {@link UiToggles}. */
  uiToggles: `${PREFIX}ui.toggles`,
  /** PnL distribution bin density (Sparse / Default / Dense). */
  pnlDistDensity: `${PREFIX}ui.pnlDistDensity`,
  /** Metric-panes selection prefs (panes, windows, rule, autoPanes). */
  metricPanes: `${PREFIX}ui.metricPanes`,

  // ── tables (one map per concern, keyed by `tableId`) ───────────────────────
  /** Map of `{ [tableId]: visibleColumnKey[] }` — all DataTable column toggles. */
  tableCols: `${PREFIX}table.cols`,
  /** Map of `{ [tableId]: everyColumnKey[] }` — the column set that existed when
   *  `tableCols` was last written, so newly added columns can be told apart from
   *  user-hidden ones. See `getTableKnownCols`. */
  tableKnownCols: `${PREFIX}table.knownCols`,
  /** Map of `{ [tableId]: { pageSize, sortKeys, pinsCollapsed, filtersOpen } }`. */
  tablePrefs: `${PREFIX}table.prefs`,
  /** Map of `{ [tableId]: { keys, rows } }` — DataTable pinned rows (keys + a
   *  snapshot of each pinned row so it renders on every page without a refetch). */
  tablePins: `${PREFIX}table.pins`,
  /** Map of `{ [tableId]: boolean }` — TokenTable charts-grid on/off. */
  tableCharts: `${PREFIX}table.charts`,

  // ── pages / form drafts ───────────────────────────────────────────────────
  /** Creation Stats page + its grouped section (one blob, see `creationStatsPrefs`). */
  pageCreationStats: `${PREFIX}page.creationStats`,
  tokenFilters: `${PREFIX}tokens.filters`,
  tokensLive: `${PREFIX}tokens.live`,
  swingCriteria: `${PREFIX}swing.criteria`,
  /** Flow-discovery form draft. */
  flowDiscoveryConfig: `${PREFIX}form.flowDiscovery`,
  /** Metric-discovery form draft. */
  metricDiscoveryConfig: `${PREFIX}form.metricDiscovery`,
  /** FlowPreviewChart toolbar toggle state (flow-discovery). */
  flowPreviewChartPrefs: `${PREFIX}flow.previewChart.prefs`,

  // ── strategy surfaces ─────────────────────────────────────────────────────
  sweepConfig: `${PREFIX}sweep.config`,
  /** Pending discovery → sweep seed (sessionStorage). Cleared once the sweep form applies it. */
  sweepDiscoverySeed: `${PREFIX}sweep.discoverySeed`,
  /** Base key for persisted sweep run selection; append `.${strategyId}` for per-strategy key. */
  sweepSel: `${PREFIX}sweep.sel`,
  /** Sweep results "show not-fired tokens" toggle. Deliberately NOT shared with
   *  {@link simShowNotFired} — a sweep row is a combo token, a sim row a position. */
  sweepShowNotFired: `${PREFIX}sweep.showNotFired`,
  /** "show not-fired (NoEntry) tokens" — one product preference, shared by
   *  Simulate Positions and the Dry-run detail. */
  simShowNotFired: `${PREFIX}simulate.showNotFired`,

  // ── URL-mirrored view filters (base keys; append `.${pageId}`) ─────────────
  /** Rule-tag filter mirror; the URL stays authoritative (see `useTagFilter`). */
  filterTags: `${PREFIX}filter.tags`,
  /** Trade-mode scope mirror; the URL stays authoritative (see `useModeFilter`). */
  filterMode: `${PREFIX}filter.mode`,

  // ── data (deliberately not a view pref) ───────────────────────────────────
  /** Manual-trade log on Console — survives reloads; positions table stays truth. */
  consoleTradeLog: `${PREFIX}console.tradeLog`,
} as const;

/**
 * Logical ids for {@link STORAGE_KEYS.uiAccordion}. An `Accordion`'s `storageKey`
 * prop is one of these ids, never a full `mt:` key — the map owns the prefix.
 */
export const ACCORDION_IDS = {
  /** Shared position-summary shell (Evidence / Simulate / Sweep). */
  positionSummary: 'positionSummary.summary',
  /** Charts deck inside the shared position-summary shell. */
  positionCharts: 'positionSummary.charts',
  /** Wallet analytics charts deck. */
  walletCharts: 'wallet.charts',
  /** Console History charts deck. */
  historyCharts: 'history.charts',
  /** Lab token-inspect "Detail" panel. */
  inspectDetail: 'inspect.detail',
  /** Metric-pane selector (page layout). */
  metricSelector: 'metricSelector',
  /** Metric-pane selector (inspect-modal layout). */
  metricSelectorInspect: 'metricSelector.inspect',
  /** Generic sweep "Run details" panel. */
  sweepDetailsGeneric: 'sweep.detailsOpen.generic',
} as const;

/** App-wide show/hide switches, stored together under `mt:ui.toggles`. */
export interface UiToggles {
  /** Rules board + Simulate rules list: include soft-archived rules. */
  showDisabledRules?: boolean;
  /** My Wallet: hide sub-threshold holdings. */
  hideDust?: boolean;
  /** Console: the collapsible WAITING lane. */
  consoleWaitingOpen?: boolean;
  /** Console: the collapsible Manual-trade panel (forced open when `?mint=` prefills). */
  consoleManualOpen?: boolean;
}

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

// ── blob-field accessors ────────────────────────────────────────────────────
// Related prefs live in ONE object (`ui.accordion`, `ui.toggles`,
// `page.creationStats`, …) instead of one flat key each. Writes are
// read-modify-write against storage — never against a cached copy — so two
// fields of the same blob can never clobber each other.

type Blob = Record<string, unknown>;

/** Read one field of the blob at `key`; `fallback` when the blob or field is absent. */
export function getField<T>(key: string, field: string, fallback: T): T {
  const blob = getJSON<Blob>(key, {});
  const v = blob[field];
  return v === undefined ? fallback : (v as T);
}

/** Write one field of the blob at `key`, leaving its siblings untouched. */
export function setField(key: string, field: string, value: unknown): void {
  const blob = getJSON<Blob>(key, {});
  blob[field] = value;
  setJSON(key, blob);
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

/** Every column id that existed for `tableId` the last time prefs were saved, or
 *  `null` for a blob written before this was tracked. Because `tableCols` stores
 *  a *visible* set, a key's absence there is ambiguous — the user hid it, or it
 *  didn't exist yet. This list disambiguates: absent here ⇒ genuinely new ⇒ the
 *  column's own `defaultVisible` decides, instead of it being stuck hidden. */
export function getTableKnownCols(tableId: string): string[] | null {
  const map = getJSON<TableColsMap>(STORAGE_KEYS.tableKnownCols, {});
  const cols = map[tableId];
  return Array.isArray(cols) ? cols : null;
}

/** Persist the full known-column id set for `tableId`. Written alongside every
 *  `setTableCols` call so the two never disagree about which columns existed. */
export function setTableKnownCols(tableId: string, cols: string[]): void {
  const map = getJSON<TableColsMap>(STORAGE_KEYS.tableKnownCols, {});
  map[tableId] = cols;
  setJSON(STORAGE_KEYS.tableKnownCols, map);
}

// ── table sort + page-size preferences ─────────────────────────────────────

export interface TablePrefs {
  pageSize?: number;
  /** Multi-key sort; index 0 = primary. */
  sortKeys?: { col: string; dir: 'asc' | 'desc' }[];
  /** Pinned section collapsed (pins themselves live in `tablePins`, untouched). */
  pinsCollapsed?: boolean;
  /** Pinned rows hidden everywhere — section AND paged body (pins are kept). */
  pinsHidden?: boolean;
  /** Column-filter row revealed under the header. */
  filtersOpen?: boolean;
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

// ── table pinned-rows ───────────────────────────────────────────────────────
// Pins persist per table. Because tables are often server-paged, a pinned row is
// usually not in the page currently held — so we store a snapshot of each pinned
// row alongside its key and render from that, refreshing it whenever a page
// happens to contain the row. See `usePinnedRows`.

export interface TablePins {
  keys: string[];
  /** key → last-seen full row object (snapshot). */
  rows: Record<string, unknown>;
}

type TablePinsMap = Record<string, TablePins>;

/** Pinned-row keys + snapshots saved for `tableId`, or empty if none stored. */
export function getTablePins(tableId: string): TablePins {
  const map = getJSON<TablePinsMap>(STORAGE_KEYS.tablePins, {});
  const v = map[tableId];
  return v && Array.isArray(v.keys) ? { keys: v.keys, rows: v.rows ?? {} } : { keys: [], rows: {} };
}

/** Persist pinned-row keys + snapshots for `tableId` into the shared map. */
export function setTablePins(tableId: string, pins: TablePins): void {
  const map = getJSON<TablePinsMap>(STORAGE_KEYS.tablePins, {});
  map[tableId] = pins;
  setJSON(STORAGE_KEYS.tablePins, map);
}

// ── table charts-grid toggle ────────────────────────────────────────────────

/** Charts-grid on/off for `tableId`; `fallback` until the user toggles it once. */
export function getTableCharts(tableId: string | undefined, fallback: boolean): boolean {
  if (!tableId) return fallback;
  return getField<boolean>(STORAGE_KEYS.tableCharts, tableId, fallback);
}

/** Persist the charts-grid toggle for `tableId` into the shared map. */
export function setTableCharts(tableId: string | undefined, on: boolean): void {
  if (!tableId) return;
  setField(STORAGE_KEYS.tableCharts, tableId, on);
}

// ── one-time migration + cleanup of superseded keys ─────────────────────────
// Everything the app persists now lives under `mt:` and the consolidated blobs
// above. Earlier builds wrote ~25 pre-namespace flat keys, then a `hunter.*`
// second namespace, then one flat `mt:` key per pref. Fold what carries a user
// preference into its blob and purge the rest, once, before any consumer reads.
// Pattern-matched (not an exact list) so renamed/versioned variants
// (`*_v2`/`*_v3`, peak_trough→swing, the pre-split `tpsl_rules_cols`, …) are all
// caught without per-key bookkeeping, while third-party keys are left untouched.

/** Pre-namespace singleton keys (no shared suffix to pattern-match on). */
const LEGACY_SINGLETON_KEYS = [
  'app_timezone',
  'price_unit',
  'token_price_chart_prefs',
  'tokens_global_filters',
  'tokens_live',
  'swing_detection_criteria',
];

/** Flat `mt:` keys retired by the blob consolidation, and what replaced them. */
const LEGACY_JSON_MOVES: { from: string; to: string; field?: string }[] = [
  // Creation Stats page + grouped section → one `page.creationStats` blob.
  { from: `${PREFIX}dashboard.metric`, to: STORAGE_KEYS.pageCreationStats, field: 'metric' },
  { from: `${PREFIX}dashboard.segment`, to: STORAGE_KEYS.pageCreationStats, field: 'segment' },
  { from: `${PREFIX}dashboard.bucket`, to: STORAGE_KEYS.pageCreationStats, field: 'bucket' },
  { from: `${PREFIX}dashboard.range`, to: STORAGE_KEYS.pageCreationStats, field: 'range' },
  { from: `${PREFIX}dashboard.grouped.by`, to: STORAGE_KEYS.pageCreationStats, field: 'groupedBy' },
  { from: `${PREFIX}dashboard.grouped.top`, to: STORAGE_KEYS.pageCreationStats, field: 'groupedTop' },
  { from: `${PREFIX}dashboard.grouped.rankBy`, to: STORAGE_KEYS.pageCreationStats, field: 'groupedRankBy' },
  { from: `${PREFIX}dashboard.grouped.bucketWidth`, to: STORAGE_KEYS.pageCreationStats, field: 'groupedBucketWidth' },
  { from: `${PREFIX}dashboard.grouped.exactSol`, to: STORAGE_KEYS.pageCreationStats, field: 'groupedExactSol' },
  { from: `${PREFIX}dashboard.grouped.bucket`, to: STORAGE_KEYS.pageCreationStats, field: 'groupedBucket' },
  { from: `${PREFIX}dashboard.grouped.range`, to: STORAGE_KEYS.pageCreationStats, field: 'groupedRange' },
  { from: `${PREFIX}dashboard.grouped.filters`, to: STORAGE_KEYS.pageCreationStats, field: 'groupedFilters' },
  { from: `${PREFIX}dashboard.grouped.cashback`, to: STORAGE_KEYS.pageCreationStats, field: 'groupedCashback' },
  { from: `${PREFIX}dashboard.grouped.ixLabels`, to: STORAGE_KEYS.pageCreationStats, field: 'groupedIxLabels' },
  { from: `${PREFIX}dashboard.grouped.fingerprintId`, to: STORAGE_KEYS.pageCreationStats, field: 'groupedFingerprintId' },
  // Second-namespace escapees + one-off raw keys → the registry.
  { from: 'hunter.lab.flowDiscovery.config', to: STORAGE_KEYS.flowDiscoveryConfig },
  { from: 'hunter.lab.metricDiscovery.config', to: STORAGE_KEYS.metricDiscoveryConfig },
  { from: `${PREFIX}metric-panes`, to: STORAGE_KEYS.metricPanes },
  { from: `${PREFIX}console-trade-log`, to: STORAGE_KEYS.consoleTradeLog },
];

/** Raw (non-JSON) values that only needed a rename — copied verbatim. */
const LEGACY_STRING_MOVES: { from: string; to: string }[] = [
  { from: 'simulate.tagFilter', to: `${STORAGE_KEYS.filterTags}.simulate` },
  { from: 'rules.tagFilter', to: `${STORAGE_KEYS.filterTags}.rules` },
  { from: 'rules-control.tagFilter', to: `${STORAGE_KEYS.filterTags}.rules-control` },
  { from: 'simulate.modeFilter', to: `${STORAGE_KEYS.filterMode}.simulate` },
  { from: 'rules.modeFilter', to: `${STORAGE_KEYS.filterMode}.rules` },
  { from: 'rules-control.modeFilter', to: `${STORAGE_KEYS.filterMode}.rules-control` },
];

/** Accordion opens used to be one `'0'|'1'` key each; they are now ids in the map. */
const LEGACY_ACCORDION_MOVES: { from: string; id: string }[] = [
  { from: `${PREFIX}inspect-detail-open`, id: ACCORDION_IDS.inspectDetail },
  { from: `${PREFIX}metric-selector-open`, id: ACCORDION_IDS.metricSelector },
  { from: `${PREFIX}metric-selector-inspect-open`, id: ACCORDION_IDS.metricSelectorInspect },
  { from: `${PREFIX}sweep.detailsOpen.generic`, id: ACCORDION_IDS.sweepDetailsGeneric },
];

/** Per-`tableId` charts flags used to be one `'0'|'1'` key each. */
const LEGACY_TABLE_CHARTS_PREFIX = `${PREFIX}tablecharts:`;

/** True for any key this module has replaced and no longer reads. `mt:` keys in
 *  the registry never match; third-party keys never match. */
function isLegacyStorageKey(key: string): boolean {
  if (key.startsWith('hunter.')) return true;
  if (key.startsWith(LEGACY_TABLE_CHARTS_PREFIX)) return true;
  if (LEGACY_JSON_MOVES.some((m) => m.from === key)) return true;
  if (LEGACY_STRING_MOVES.some((m) => m.from === key)) return true;
  if (LEGACY_ACCORDION_MOVES.some((m) => m.from === key)) return true;
  if (key.startsWith(PREFIX)) return false;
  return (
    key.endsWith('_cols') ||
    key.startsWith('sweep_cfg_') ||
    LEGACY_SINGLETON_KEYS.includes(key)
  );
}

/** Move `from` → `to` only when the destination has nothing yet, so a migration
 *  that runs twice (or after the user already re-set the pref) never regresses. */
function moveJSON(from: string, to: string, field?: string): void {
  const raw = getString(from);
  if (raw == null) return;
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return;
  }
  if (field) {
    const blob = getJSON<Blob>(to, {});
    if (blob[field] === undefined) {
      blob[field] = parsed;
      setJSON(to, blob);
    }
  } else if (getString(to) == null) {
    setJSON(to, parsed);
  }
}

let migrated = false;

/**
 * Fold every superseded key into its current home, then purge the leftovers.
 * Idempotent (a no-op once the old keys are gone) and wrapped so a storage
 * failure never breaks boot. Runs once on module import, before any consumer
 * reads. This is deliberately a single function, not a migration framework —
 * shape changes merge with defaults at the reader instead of versioning keys.
 */
export function migrateLegacyStorage(): void {
  if (migrated) return;
  migrated = true;
  try {
    if (typeof localStorage === 'undefined') return;

    for (const m of LEGACY_JSON_MOVES) moveJSON(m.from, m.to, m.field);

    for (const m of LEGACY_STRING_MOVES) {
      const v = getString(m.from);
      if (v != null && getString(m.to) == null) setString(m.to, v);
    }

    // `hunter.pnlDistDensity` held a bare string; the registry key holds JSON.
    const density = getString('hunter.pnlDistDensity');
    if (density && getString(STORAGE_KEYS.pnlDistDensity) == null) {
      setJSON(STORAGE_KEYS.pnlDistDensity, density);
    }

    for (const m of LEGACY_ACCORDION_MOVES) {
      const v = getString(m.from);
      if (v !== '0' && v !== '1') continue;
      if (getField<boolean | undefined>(STORAGE_KEYS.uiAccordion, m.id, undefined) === undefined) {
        setField(STORAGE_KEYS.uiAccordion, m.id, v === '1');
      }
    }

    // Snapshot keys first — removing while iterating by index skips entries.
    const stale: string[] = [];
    for (let i = 0; i < localStorage.length; i++) {
      const key = localStorage.key(i);
      if (key && isLegacyStorageKey(key)) stale.push(key);
    }
    // Per-table charts flags carry a value worth keeping; fold before purging.
    // A `…-<runId>` / `…-<ruleId>` leftover from when the tableId embedded a
    // volatile id is dead weight — drop it instead of reviving it in the map.
    for (const key of stale) {
      if (!key.startsWith(LEGACY_TABLE_CHARTS_PREFIX)) continue;
      const tableId = key.slice(LEGACY_TABLE_CHARTS_PREFIX.length);
      if (isVolatileTableId(tableId)) continue;
      const v = getString(key);
      if (v === '0' || v === '1') setTableCharts(tableId, v === '1');
    }
    for (const key of stale) localStorage.removeItem(key);
  } catch {
    /* never let a storage migration break app boot */
  }
}

/** `simulate-positions-<runId>` / `rule-evidence-<ruleId>` — a fresh key accrued
 *  per run/rule ever visited and none were pruned. Those call sites now pass a
 *  stable `tableId`, so only the un-suffixed form is worth migrating. */
function isVolatileTableId(tableId: string): boolean {
  return (
    tableId.startsWith('simulate-positions-') || tableId.startsWith('rule-evidence-')
  );
}

// Run before any consumer reads (every persister imports this module).
migrateLegacyStorage();
