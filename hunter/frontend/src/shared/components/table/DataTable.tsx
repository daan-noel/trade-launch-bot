import { Fragment, memo, useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { cn } from 'lib/cn';
import { Checkbox } from 'components/ui/Checkbox';
import { Input } from 'components/ui/Input';
import { Select } from 'components/ui/Select';
import { PinIcon } from 'components/ui/icons';
import { Pagination, DEFAULT_PAGE_SIZE } from './Pagination';
import { parseNumericPredicate, type FilterSpec } from './numericFilter';
import { computeSameValueCellClasses } from 'lib/sameValueCellColors';
import { usePinnedRows } from './usePinnedRows';
import {
  getTableCols,
  setTableCols,
  getTableKnownCols,
  setTableKnownCols,
  getTablePrefs,
  setTablePrefs,
} from 'lib/storage';
import type { ColumnDef, SortDir, SortEntry, SortValue, TableQuery } from './types';

/**
 * Column-hover highlight, done in pure CSS instead of React state.
 *
 * Previously a shared `hoveredCol` number was threaded into every (memoized)
 * row, so moving the mouse across columns changed that prop for *all* rows and
 * re-rendered the entire visible page on each hover-move — defeating the row
 * memoization precisely during interaction. A `:has()` + `nth-child` rule keeps
 * the highlight entirely in the style engine: hovering any cell in column N
 * tints that column's header + body cells, at zero React cost. Injected once.
 * (Column 1 is the `#` index column and is deliberately left out, matching the
 * old behaviour.)
 */
const HOVER_STYLE_ID = 'dt-colhover-style';
if (typeof document !== 'undefined' && !document.getElementById(HOVER_STYLE_ID)) {
  // `:not(.dt-nohover)` excludes the spanning group-header banner row, whose
  // `colSpan` cells make `nth-child(n)` point at the wrong visual column — without
  // this, hovering a column highlights a misaligned banner cell.
  const sels: string[] = [];
  for (let n = 2; n <= 48; n++) {
    sels.push(
      `.dt-colhover:has(:is(td,th):not(.dt-nohover):nth-child(${n}):hover) :is(td,th):not(.dt-nohover):nth-child(${n})`,
    );
  }
  const el = document.createElement('style');
  el.id = HOVER_STYLE_ID;
  // Column-hover tint = `bg-primary/12`, but resolved from the live token so it
  // follows the per-app primary (teal on live, blue on lab) instead of a
  // hardcoded teal.
  el.textContent = `${sels.join(',')}{background-color:color-mix(in srgb, var(--color-primary) 12%, transparent)}`;
  document.head.appendChild(el);
}

/** Drop empty/whitespace entries so they don't churn the server query. */
function cleanColFilters(map: Record<string, string>): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [k, v] of Object.entries(map)) {
    if (v.trim()) out[k] = v;
  }
  return out;
}

/**
 * Columns with `filterOptions` are exclusive choices — emit them as `{op:'eq'}`
 * structured filters (not substring `contains`) so any server table that keys
 * off exact values (flags, status, mode) gets the right wire shape. Remaining
 * keys stay as raw per-column text for `toTableRequest` to parse.
 */
function splitEnumColFilters<R>(
  colFilters: Record<string, string>,
  columns: ColumnDef<R>[],
): { textFilters: Record<string, string>; structured: Record<string, FilterSpec> } {
  const enumKeys = new Set(
    columns.filter((c) => c.filterOptions && c.filterOptions.length > 0).map((c) => c.key),
  );
  if (enumKeys.size === 0) return { textFilters: colFilters, structured: {} };
  const textFilters: Record<string, string> = {};
  const structured: Record<string, FilterSpec> = {};
  for (const [k, v] of Object.entries(colFilters)) {
    if (enumKeys.has(k)) structured[k] = { op: 'eq', val: v };
    else textFilters[k] = v;
  }
  return { textFilters, structured };
}

/**
 * Per-table override of a column's own `defaultVisible`, keyed by column key
 * (`true` = start shown, `false` = start hidden). Several tables are built from
 * one shared column array (`simColumns`, `positionColumns`, the appended
 * token-info set) but want different columns up front; this is the per-call-site
 * escape hatch for that, so the shared array keeps ONE default and each page
 * states only its own deviations. `sortOnly` columns are never toggleable and
 * ignore this map.
 */
export type ColVisibilityOverrides = Readonly<Record<string, boolean>>;

/** A column's initial visibility: the table's override if it names the key, else
 *  the column's own `defaultVisible`. The ONE decider — used for the persisted
 *  path, the unpersisted path, and the new-column merge below. */
function defaultVisibleFor(
  c: ColumnDef<unknown>,
  overrides?: ColVisibilityOverrides,
): boolean {
  if (c.sortOnly) return false;
  const o = overrides?.[c.key];
  return o !== undefined ? o : c.defaultVisible !== false;
}

/**
 * Merge the saved visible-column set with the current column defs.
 *
 * `tableCols` persists a **visible** set, so a key's absence is ambiguous on its
 * own — the user hid it, or the column didn't exist when they saved. The
 * companion `tableKnownCols` set resolves that: a column absent from *both* is
 * genuinely new, so its own `defaultVisible` applies. Without this, every column
 * added to an existing table stays invisible forever for anyone who had ever
 * touched that table's toggles — silently shipping a feature nobody can see.
 */
function loadVisibleCols(
  tableId: string,
  columns: ColumnDef<unknown>[],
  overrides?: ColVisibilityOverrides,
): Set<string> {
  const isDefaultVisible = (c: ColumnDef<unknown>) => defaultVisibleFor(c, overrides);
  const defaults = new Set(columns.filter(isDefaultVisible).map((c) => c.key));
  const stored = getTableCols(tableId);
  if (!stored) return defaults;
  // Schema drift (renamed/removed column keys): stored prefs are stale — reset.
  if (stored.some((k) => !columns.some((c) => c.key === k))) return defaults;

  const known = getTableKnownCols(tableId);
  const sortOnlyKeys = new Set(columns.filter((c) => c.sortOnly).map((c) => c.key));
  const set = new Set(
    stored.filter((k) => columns.some((c) => c.key === k) && !sortOnlyKeys.has(k)),
  );
  if (!known) {
    // Legacy blob written before known-columns tracking: new columns can't be
    // distinguished from hidden ones, so union in the defaults once. This may
    // re-show something the user had hidden — a one-time cost, paid only on the
    // first load after upgrading, in exchange for never losing a new column.
    for (const k of defaults) set.add(k);
    return set;
  }
  // Columns that didn't exist at save time are new — honour their own default.
  for (const c of columns) {
    if (!known.includes(c.key) && isDefaultVisible(c)) set.add(c.key);
  }
  return set.size ? set : defaults;
}

function saveVisibleCols(tableId: string, cols: Set<string>, colKeys: string[]) {
  setTableCols(tableId, [...cols]);
  // Always stamped together with the visible set, so the next load knows exactly
  // which columns the user has actually been offered.
  setTableKnownCols(tableId, colKeys);
}

function compareSort(a: SortValue, b: SortValue, dir: SortDir): number {
  if (a == null && b == null) return 0;
  if (a == null) return 1;
  if (b == null) return -1;
  let cmp: number;
  if (typeof a === 'number' && typeof b === 'number') cmp = a - b;
  else cmp = String(a).localeCompare(String(b));
  return dir === 'asc' ? cmp : -cmp;
}

/** Cheap stable signature for sort keys (avoids JSON.stringify on every render). */
function sortKeysSignature(keys: SortEntry[]): string {
  if (keys.length === 0) return '';
  let s = `${keys[0].col}:${keys[0].dir}`;
  for (let i = 1; i < keys.length; i++) s += `|${keys[i].col}:${keys[i].dir}`;
  return s;
}

/** Cheap stable signature for a col-filter map (sorted keys → deterministic). */
function colFiltersSignature(map: Record<string, string>): string {
  const keys = Object.keys(map);
  if (keys.length === 0) return '';
  keys.sort();
  let s = `${keys[0]}=${map[keys[0]]}`;
  for (let i = 1; i < keys.length; i++) s += `|${keys[i]}=${map[keys[i]]}`;
  return s;
}

/** localStorage prefs coalesce window — rapid sort/page toggles write once. */
const PREFS_PERSIST_MS = 150;

/**
 * The canonical string a cell groups by for `sameValueTints`. Prefers the
 * column's own numeric/sort accessors over its display text so `1.5` and
 * `1.50` group together; returns null (never tinted) for unset values and for
 * columns that expose no comparable value at all (chip blobs, action columns).
 */
function cellGroupValue<R>(col: ColumnDef<R>, row: R): string | null {
  if (col.filterNumber) {
    const n = col.filterNumber(row);
    return n == null || !Number.isFinite(n) ? null : String(n);
  }
  if (col.sortValue) {
    const v = col.sortValue(row);
    if (v == null) return null;
    if (typeof v === 'number') return Number.isFinite(v) ? String(v) : null;
    return v === '' ? null : v;
  }
  const text = (col.filterValue ?? col.searchValue)(row).trim();
  return text === '' ? null : text;
}

/**
 * Can this column join `sortKeys`? Bare columns (no `sortable: true`, no
 * `sortValue`) must not — a header click used to append a ghost level that
 * showed no real reorder, so the *next* click looked like sort priority "2".
 */
function columnAcceptsSort<R>(col: ColumnDef<R>): boolean {
  if (col.sortable === false) return false;
  return col.sortable === true || col.sortValue != null;
}

function pruneSortKeys<R>(keys: SortEntry[], columns: ColumnDef<R>[]): SortEntry[] {
  const allowed = new Set(columns.filter(columnAcceptsSort).map((c) => c.key));
  return keys.filter((s) => allowed.has(s.col));
}

interface DataTableProps<R> {
  columns: ColumnDef<R>[];
  rows: R[];
  rowKey: (row: R) => string;
  rowDetail?: (row: R) => ReactNode;
  rowActions?: (row: R) => ReactNode;
  selectedKey?: string | null;
  onSelect?: (key: string | null) => void;
  defaultPageSize?: number;
  pageSizeOptions?: number[];
  searchable?: boolean;
  /** Placeholder for the global search box. Default `Search…`. */
  searchPlaceholder?: string;
  /**
   * Extra controls rendered before the search box (domain wrappers — e.g. a
   * scope toggle). Keep token/history vocabulary out of this primitive; pass
   * nodes from the caller.
   */
  toolbarLeading?: ReactNode;
  /**
   * Extra controls rendered after the flex spacer, before Filters/Columns
   * (domain wrappers — e.g. Charts toggle). Expanding panels should live
   * outside the toolbar row, not inside this slot.
   */
  toolbarTrailing?: ReactNode;
  /** Per-column filter row toggle (toolbar "Filters"). Default on. */
  colFilters?: boolean;
  /** Column-visibility panel toggle (toolbar "Columns"). Default on. */
  colToggle?: boolean;
  hoverable?: boolean;
  /** Stable id for persisting this table's column visibility (folded into the
   *  shared `mt:table.cols` map). Omit to not persist column toggles. */
  tableId?: string;
  /** Per-table deviations from the shared column array's own `defaultVisible`
   *  (`{ [colKey]: shown }`). See {@link ColVisibilityOverrides}. Applies to the
   *  first render only — once the user touches the Columns panel, the persisted
   *  set wins (a column added later still falls back to this map). */
  defaultCols?: ColVisibilityOverrides;
  emptyMessage?: string;
  selectable?: boolean;
  paginate?: boolean;
  /**
   * Opt-in spanning group-header row: a banner above the column headers labeling
   * each `group` run (e.g. `{ entry: 'Entry', exit: 'Exit' }`). Maps a column's
   * `group` key → its banner text; a group absent from the map (or mapped to '')
   * renders a blank banner. Omit the prop entirely to skip the row (default).
   */
  groupLabels?: Record<string, string>;
  /**
   * Initial sort column + direction (client-side mode). Sets the table's starting
   * order without locking it — the user can still re-sort by clicking headers.
   * Omit to start unsorted (rows render in the order passed).
   */
  defaultSort?: { col: string; dir?: SortDir };
  /**
   * Server-side mode: the table stops filtering/sorting/slicing locally and
   * renders `rows` as the current page verbatim. It emits its view-state via
   * `onQueryChange` (debounced) so the parent can fetch; `serverTotal` drives
   * the pager. Defaults off — every other table keeps its client-side path.
   */
  serverSide?: boolean;
  serverTotal?: number;
  onQueryChange?: (q: TableQuery) => void;
  loading?: boolean;
  /**
   * Server mode only: change this whenever an *external* control that affects
   * the result set changes (e.g. a parent-owned global filter panel). The table
   * snaps back to page 1 and re-emits, exactly as it does for its own
   * search/sort/filter changes.
   */
  resetKey?: string | number;
  /** Optional extra className(s) applied to each data row's `<tr>`. Useful for
   *  per-row highlights (e.g. marking entry/exit trades). */
  rowClassName?: (row: R) => string | undefined;
  /** Optional extra className(s) applied to each data `<td>` based on its
   *  column group key. Called once per visible cell; return undefined to skip. */
  cellGroupClassName?: (group: string | undefined, row: R) => string | undefined;
  /**
   * Opt-in same-value cell tints (the fingerprint-table look): in every visible
   * column, a value shared by ≥2 rows on the page gets a stable background from
   * a fixed palette, so repeated values read as bands at a glance. Values unique
   * on the page stay untinted. Derived from each column's `sortValue` /
   * `filterNumber` / `filterValue`, so a column with none of those is skipped.
   * A column's own `cellClassName` still wins (it merges after this).
   */
  sameValueTints?: boolean;
  /** Fires with the rows currently on screen (the processed + paginated page)
   *  whenever they change — lets a sibling view (e.g. a synced charts grid)
   *  mirror the table's current sort/filter/page. Works in client + server mode.
   *  Pass a stable (useCallback) handler; `pageRows` is memoized so it only fires
   *  on a real change, not every render. */
  onVisibleRowsChange?: (rows: R[]) => void;
  /**
   * Keep the selected row in view when the result set changes (re-sort,
   * re-filter, search, page-size, poll/refresh): the table re-pages to wherever
   * that row moved and scrolls it back into view. Following pauses while the
   * user has paged away by hand, and resumes on the next selection change.
   * Default on; pass `false` for tables where the page must never move on its
   * own. Cross-page follow needs client-side mode (server mode can't locate a
   * row it hasn't fetched) — the scroll-into-view half still applies there.
   */
  followSelected?: boolean;
  /** Fires with the FULL filtered result set (client mode: after search/column
   *  filters/sort, before pagination — every matching row, not just the page).
   *  Lets a sibling summary recompute over the whole matching cohort the way a
   *  server-side summary would. In server mode this equals the current page.
   *  Pass a stable (useCallback) handler; `processed` is memoized. */
  onFilteredRowsChange?: (rows: R[]) => void;
  /**
   * Opt-in row pinning. Adds a pin toggle in the `#` cell and floats pinned rows in
   * a section above the page on every page, deduped from the paged body. Pinned rows
   * render from snapshots (persisted per `tableId`), so they show even when their
   * page isn't the one currently loaded. Requires `tableId` (pins are stored under
   * it); a no-op without one. Omit to render exactly as before.
   */
  pinnable?: boolean;
}

export function DataTable<R>({
  columns,
  rows,
  rowKey,
  rowDetail,
  rowActions,
  selectedKey: externalSelected,
  onSelect,
  defaultPageSize = DEFAULT_PAGE_SIZE,
  pageSizeOptions,
  searchable = true,
  searchPlaceholder = 'Search…',
  toolbarLeading,
  toolbarTrailing,
  colFilters = true,
  colToggle = true,
  hoverable = true,
  tableId,
  defaultCols,
  emptyMessage = 'No data.',
  selectable = true,
  paginate = true,
  groupLabels,
  defaultSort,
  serverSide = false,
  serverTotal,
  onQueryChange,
  loading = false,
  resetKey,
  rowClassName,
  cellGroupClassName,
  onVisibleRowsChange,
  onFilteredRowsChange,
  sameValueTints = false,
  followSelected = true,
  pinnable = false,
}: DataTableProps<R>) {
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(() => {
    if (tableId) {
      const stored = getTablePrefs(tableId).pageSize;
      if (stored != null) return stored;
    }
    return defaultPageSize;
  });
  const [sortKeys, setSortKeys] = useState<SortEntry[]>(() => {
    let initial: SortEntry[] = [];
    if (tableId) {
      const prefs = getTablePrefs(tableId);
      if (prefs.sortKeys) initial = prefs.sortKeys;
    }
    if (initial.length === 0 && defaultSort?.col) {
      initial = [{ col: defaultSort.col, dir: defaultSort.dir ?? 'desc' }];
    }
    return pruneSortKeys(initial, columns);
  });
  // Pinned-section collapse. Persisted alongside sort/page-size (the pins
  // themselves live in a separate `tablePins` entry, so collapsing never loses
  // them). Collapsing only hides the *section* — the pinned rows fall back into
  // the paged body at their natural position (see `hidePinnedInBody`), so no row
  // silently disappears.
  const [pinsCollapsed, setPinsCollapsed] = useState(() =>
    tableId ? getTablePrefs(tableId).pinsCollapsed ?? false : false,
  );
  const [search, setSearch] = useState('');
  const [colFiltersMap, setColFiltersMap] = useState<Record<string, string>>({});
  const [visibleCols, setVisibleCols] = useState<Set<string>>(() =>
    tableId
      ? loadVisibleCols(tableId, columns as ColumnDef<unknown>[], defaultCols)
      : new Set(
          columns
            .filter((c) => defaultVisibleFor(c as ColumnDef<unknown>, defaultCols))
            .map((c) => c.key),
        ),
  );
  // The column-key set, and a stable string signature of it. Callers don't all
  // memoize their `columns` array, so keying the persist effect on the array
  // identity would rewrite localStorage on every render — the signature fires it
  // only when a column is actually added or removed. Join with `\0` so
  // `['a','bc']` and `['ab','c']` don't collide.
  const colKeys = useMemo(() => (columns as ColumnDef<unknown>[]).map((c) => c.key), [columns]);
  const colKeysSig = useMemo(() => colKeys.join(' '), [colKeys]);
  const [internalSelected, setInternalSelected] = useState<string | null>(null);
  const [showColPanel, setShowColPanel] = useState(false);
  // Revealing the filter row is page chrome, so it persists with the table's
  // other prefs — a table you always filter opens ready to filter.
  const [showFilterRow, setShowFilterRow] = useState(() =>
    tableId ? getTablePrefs(tableId).filtersOpen ?? false : false,
  );
  // Debounced mirrors of the search box / per-column filter inputs. Both the
  // server-side emit and the client-side `processed` filter read these so a
  // burst of keystrokes coalesces into a single query/recompute instead of one
  // per character (the client list can be large — see TOKENS_LIST_LIMIT).
  const [debouncedSearch, setDebouncedSearch] = useState('');
  const [debouncedColFilters, setDebouncedColFilters] = useState<Record<string, string>>({});

  // Controlled callers pass `selectedKey` (string | null); an explicit null then
  // means "nothing selected" and must win over any stale internal selection (e.g.
  // sibling tables sharing one selection). Uncontrolled callers omit it
  // (undefined) and fall back to the table's own internal selection.
  const selectedKey =
    externalSelected !== undefined ? externalSelected : internalSelected;

  // ---------------------------------------------------------------------
  // Stabilize callback props for memoized rows
  // ---------------------------------------------------------------------
  // Callers often pass inline `rowActions` / `rowClassName` / `rowKey` / etc.
  // Default `memo` is shallow, so a fresh closure every parent render would
  // re-render every visible row — defeating TableRow memo on live/polled tables.
  // Refs always call the latest handler; the wrappers keep a stable identity.
  const rowKeyRef = useRef(rowKey);
  rowKeyRef.current = rowKey;
  const rowActionsRef = useRef(rowActions);
  rowActionsRef.current = rowActions;
  const rowDetailRef = useRef(rowDetail);
  rowDetailRef.current = rowDetail;
  const rowClassNameRef = useRef(rowClassName);
  rowClassNameRef.current = rowClassName;
  const cellGroupClassNameRef = useRef(cellGroupClassName);
  cellGroupClassNameRef.current = cellGroupClassName;
  const onSelectRef = useRef(onSelect);
  onSelectRef.current = onSelect;

  const stableRowActions = useMemo(
    () => (rowActions ? (row: R) => rowActionsRef.current!(row) : undefined),
    // Re-create only when the *presence* of actions changes (adds/removes the column).
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [!!rowActions],
  );
  const stableRowDetail = useMemo(
    () => (rowDetail ? (row: R) => rowDetailRef.current!(row) : undefined),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [!!rowDetail],
  );
  const stableRowClassName = useMemo(
    () => (rowClassName ? (row: R) => rowClassNameRef.current!(row) : undefined),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [!!rowClassName],
  );
  const stableCellGroupClassName = useMemo(
    () =>
      cellGroupClassName
        ? (group: string | undefined, row: R) => cellGroupClassNameRef.current!(group, row)
        : undefined,
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [!!cellGroupClassName],
  );

  useEffect(() => {
    if (!tableId) return;
    const id = window.setTimeout(
      () => saveVisibleCols(tableId, visibleCols, colKeys),
      PREFS_PERSIST_MS,
    );
    return () => window.clearTimeout(id);
  }, [visibleCols, tableId, colKeys, colKeysSig]);

  useEffect(() => {
    // `setTablePrefs` replaces this table's whole entry — every persisted pref
    // must be written here, or the omitted one is wiped on the next sort/page change.
    if (!tableId) return;
    const id = window.setTimeout(
      () =>
        setTablePrefs(tableId, {
          pageSize,
          sortKeys,
          pinsCollapsed,
          filtersOpen: showFilterRow,
        }),
      PREFS_PERSIST_MS,
    );
    return () => window.clearTimeout(id);
  }, [tableId, pageSize, sortKeys, pinsCollapsed, showFilterRow]);

  // Drop sort levels for columns that disappeared or never accepted sort
  // (stale localStorage / renamed keys), so the next click is primary again.
  useEffect(() => {
    setSortKeys((prev) => {
      const next = pruneSortKeys(prev, columns);
      return next.length === prev.length && next.every((s, i) => s.col === prev[i].col && s.dir === prev[i].dir)
        ? prev
        : next;
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [colKeysSig]);

  const visCols = useMemo(
    () => columns.filter((c) => visibleCols.has(c.key)),
    [columns, visibleCols],
  );

  // Assign each visible column a group index (consecutive same-group cols share
  // it; the next group flips). Drives the even/odd tint + boundary divider.
  // The Actions column is treated as its own trailing group, so its tint
  // continues the alternation from the last data column.
  const { colGroups, actionsTinted } = useMemo(() => {
    let groupIdx = -1;
    let prevGroup: string | undefined;
    let started = false;
    const groups = visCols.map((col) => {
      const isStart = !started || col.group !== prevGroup;
      if (isStart) groupIdx += 1;
      started = true;
      prevGroup = col.group;
      return { isStart, tinted: groupIdx % 2 === 1 };
    });
    return { colGroups: groups, actionsTinted: (groupIdx + 1) % 2 === 1 };
  }, [visCols]);

  // Consecutive same-`group` runs of visible columns, for the optional spanning
  // banner row. Each run carries its colSpan and the resolved banner label (only
  // computed when `groupLabels` is set — otherwise the row isn't rendered).
  const groupRuns = useMemo(() => {
    if (!groupLabels) return [];
    const runs: { key: string; span: number; label: string; tinted: boolean }[] = [];
    visCols.forEach((col, ci) => {
      const last = runs[runs.length - 1];
      if (last && !colGroups[ci]?.isStart) {
        last.span += 1;
      } else {
        runs.push({
          key: col.group ?? `__${ci}`,
          span: 1,
          label: groupLabels[col.group ?? ''] ?? '',
          tinted: colGroups[ci]?.tinted ?? false,
        });
      }
    });
    return runs;
  }, [groupLabels, visCols, colGroups]);

  // Actions is always a new trailing group → always gets the boundary divider.
  // Memoized so its string identity is stable across renders — it's passed to
  // every memoized TableRow, so an inline `cn(...)` here would defeat their
  // React.memo on every parent re-render (worst on ~4×/sec live-trade tables).
  const actionsCellCls = useMemo(
    () =>
      cn(
        'border-l border-white/10',
        actionsTinted && 'shadow-[inset_0_0_0_1000px_rgba(255,255,255,0.02)]',
      ),
    [actionsTinted],
  );

  // Per-visible-column group class (boundary divider + tint), precomputed once
  // and held referentially stable so the memoized rows below can skip
  // re-rendering when only the polled data — not the column layout — changes.
  const groupClasses = useMemo(
    () =>
      colGroups.map((g, ci) =>
        cn(
          g?.isStart && ci > 0 && 'border-l border-white/10',
          g?.tinted && 'shadow-[inset_0_0_0_1000px_rgba(255,255,255,0.02)]',
        ),
      ),
    [colGroups],
  );

  // Resolve each active column filter once (not per row): custom `filterMatch`
  // wins; else a numeric column whose filter text is a comparison/range
  // (`>5`, `1..10`) gets a numeric predicate; else substring on displayed text.
  const activeColFilters = useMemo(() => {
    const out: {
      col: ColumnDef<R>;
      raw: string;
      needle: string;
      numeric: ((n: number) => boolean) | null;
    }[] = [];
    // Server mode applies the per-column filters itself; resolving them locally
    // would be dead work (`processed` short-circuits to `rows` below).
    if (serverSide) return out;
    for (const [key, raw] of Object.entries(debouncedColFilters)) {
      const text = raw.trim();
      if (!text) continue;
      const col = columns.find((c) => c.key === key);
      if (!col) continue;
      const numeric =
        col.filterMatch || !col.filterNumber ? null : parseNumericPredicate(text);
      out.push({ col, raw: text, needle: text.toLowerCase(), numeric });
    }
    return out;
  }, [serverSide, debouncedColFilters, columns]);

  // Precompute per-row search text once when `rows`/`columns` change — typing in
  // the search box then only does substring checks against the blob, not
  // re-running every column's `searchValue` + `toLowerCase` (client lists can be
  // large; see TOKENS_LIST_LIMIT). WeakMap keys by row identity (RTK structural
  // sharing keeps unchanged rows stable across polls).
  const searchTextByRow = useMemo(() => {
    if (serverSide) return null;
    const searchCols = columns.filter((c) => !c.sortOnly);
    const map = new WeakMap<object, string>();
    for (const row of rows) {
      let blob = '';
      for (const col of searchCols) blob += col.searchValue(row).toLowerCase() + '\0';
      map.set(row as object, blob);
    }
    return map;
  }, [serverSide, rows, columns]);

  const processed = useMemo(() => {
    // Server mode: `rows` already IS the filtered/sorted page — never reduce it
    // locally (that would hide rows the server legitimately returned).
    if (serverSide) return rows;
    // Fast path: nothing is filtering or sorting (e.g. the live-trade tables,
    // which hand back a fresh `rows` up to 4×/sec). Skip the full-buffer filter
    // pass + array allocation and hand `rows` straight through.
    if (!debouncedSearch && activeColFilters.length === 0 && sortKeys.length === 0) return rows;
    const searchLower = debouncedSearch.toLowerCase();
    let list = rows.filter((row) => {
      if (searchLower) {
        const blob = searchTextByRow?.get(row as object);
        if (!blob || !blob.includes(searchLower)) return false;
      }
      for (const { col, raw, needle, numeric } of activeColFilters) {
        if (col.filterMatch) {
          if (!col.filterMatch(row, raw)) return false;
        } else if (col.filterOptions) {
          const optVal = col.filterOptionValue
            ? col.filterOptionValue(row)
            : (col.filterValue ?? col.searchValue)(row);
          if (optVal !== raw) return false;
        } else if (numeric) {
          const n = col.filterNumber!(row);
          if (n == null || !numeric(n)) return false;
        } else {
          const value = (col.filterValue ?? col.searchValue)(row);
          if (!value.toLowerCase().includes(needle)) return false;
        }
      }
      return true;
    });

    if (sortKeys.length > 0) {
      const levels = sortKeys
        .map(({ col, dir }) => ({ sv: columns.find((c) => c.key === col)?.sortValue, dir }))
        .filter((s): s is { sv: (row: R) => SortValue; dir: SortDir } => s.sv != null);
      if (levels.length > 0) {
        // Decorate-sort-undecorate: evaluate each sortValue once per row instead
        // of once per comparison (O(n log n) comparator calls otherwise).
        list = list
          .map((row, i) => ({
            row,
            i,
            keys: levels.map(({ sv }) => sv(row)),
          }))
          .sort((a, b) => {
            for (let li = 0; li < levels.length; li++) {
              const cmp = compareSort(a.keys[li], b.keys[li], levels[li].dir);
              if (cmp !== 0) return cmp;
            }
            return a.i - b.i;
          })
          .map(({ row }) => row);
      }
    }
    return list;
  }, [serverSide, rows, searchTextByRow, debouncedSearch, activeColFilters, sortKeys, columns]);

  // ---------------------------------------------------------------------
  // Selection follow
  // ---------------------------------------------------------------------
  // The selected row is *followed*: whenever the result set is re-ordered,
  // re-filtered or refreshed, the table re-pages to wherever that row moved and
  // scrolls it back into view, so the selection never silently ends up on a page
  // the user isn't looking at.
  //
  // `followRef` is the anchor that keeps this from fighting the user: it records
  // the key we're following and the page we last placed it on. While the visible
  // page still equals that page we are "attached" and keep following; the moment
  // the user pages away by hand the two diverge and following pauses (their
  // navigation wins) until the selection changes again.
  const followRef = useRef<{ key: string | null; page: number }>({ key: null, page: 1 });
  // Scope for the `tr[data-row-key]` lookup the scroll-into-view effect does, so
  // sibling tables on the same page can't match each other's rows.
  const tableWrapRef = useRef<HTMLDivElement | null>(null);
  // Key → index in the FULL processed list. Rebuilt with `processed` so follow /
  // scroll effects are O(1) lookups instead of a linear scan on every poll.
  const processedIndexByKey = useMemo(() => {
    const map = new Map<string, number>();
    const keyOf = rowKeyRef.current;
    for (let i = 0; i < processed.length; i++) map.set(keyOf(processed[i]), i);
    return map;
  }, [processed]);
  const indexOfSelected = (key: string) => processedIndexByKey.get(key) ?? -1;

  // Reset to page 1 when the filter/sort/pageSize view *changes* — but NOT on the
  // initial mount (page already starts at 1) and NOT on a redundant re-fire. We
  // compare a signature of the view against the last one we acted on, rather than
  // calling `setPage(1)` on every effect run. Signature-comparison also makes it
  // StrictMode-safe (the doubled setup sees an unchanged signature → no-op).
  // `processed`/`rowKey` are deliberately NOT dependencies — a poll hands back a
  // new identity, and re-running this on every refresh would reset the page out
  // from under the user. With a followed selection the reset target is that row's
  // NEW page instead of page 1: re-sorting a table must not strand the selection.
  const sortKeysSig = useMemo(() => sortKeysSignature(sortKeys), [sortKeys]);
  const debouncedColFiltersSig = useMemo(
    () => colFiltersSignature(debouncedColFilters),
    [debouncedColFilters],
  );
  const clientResetSig = useMemo(
    () => `${debouncedSearch}|${debouncedColFiltersSig}|${sortKeysSig}|${pageSize}`,
    [debouncedSearch, debouncedColFiltersSig, sortKeysSig, pageSize],
  );
  const prevClientResetSig = useRef<string | null>(null);
  useEffect(() => {
    if (!paginate || serverSide) return;
    if (prevClientResetSig.current === null) {
      prevClientResetSig.current = clientResetSig; // seed on mount; don't reset
      return;
    }
    if (prevClientResetSig.current === clientResetSig) return;
    prevClientResetSig.current = clientResetSig;
    // A re-sort/re-filter is an explicit view change, so it re-attaches the
    // follow even if the user had paged away — the selection is what they asked
    // to see re-ordered.
    const index = followSelected && selectedKey ? indexOfSelected(selectedKey) : -1;
    const target = index >= 0 ? Math.floor(index / pageSize) + 1 : 1;
    if (index >= 0) followRef.current = { key: selectedKey, page: target };
    setPage(target);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [clientResetSig, paginate, serverSide]);

  // Debounce the text inputs before they drive filtering: server mode emits the
  // view-state from these; client mode feeds them into `processed` above. Either
  // way a fast typist triggers one settle, not one per keystroke.
  useEffect(() => {
    const id = setTimeout(() => {
      setDebouncedSearch(search);
      setDebouncedColFilters(colFiltersMap);
    }, 300);
    return () => clearTimeout(id);
  }, [search, colFiltersMap]);

  // Signature of everything that changes the result set *except* the page. When
  // it changes we snap back to page 1; otherwise a plain page change emits as-is.
  // Emitting atomically here (rather than in separate reset + emit effects)
  // avoids a transient fetch for the old page against the new filters.
  const cleanedColFilters = useMemo(
    () => cleanColFilters(debouncedColFilters),
    [debouncedColFilters],
  );
  const cleanedColFiltersSig = useMemo(
    () => colFiltersSignature(cleanedColFilters),
    [cleanedColFilters],
  );
  const viewSig = useMemo(
    () =>
      `${resetKey ?? ''}|${pageSize}|${sortKeysSig}|${debouncedSearch}|${cleanedColFiltersSig}`,
    [resetKey, pageSize, sortKeysSig, debouncedSearch, cleanedColFiltersSig],
  );
  const prevViewSig = useRef(viewSig);
  useEffect(() => {
    if (!serverSide) return;
    let p = page;
    if (prevViewSig.current !== viewSig) {
      prevViewSig.current = viewSig;
      if (page !== 1) {
        setPage(1);
        p = 1;
      }
    }
    const { textFilters, structured } = splitEnumColFilters(cleanedColFilters, columns);
    onQueryChange?.({
      page: p,
      pageSize,
      sortKeys,
      search: debouncedSearch,
      colFilters: textFilters,
      structuredFilters: Object.keys(structured).length > 0 ? structured : undefined,
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [serverSide, viewSig, page, columns]);

  // Clamp the page when the filtered total shrinks below the current range.
  useEffect(() => {
    if (!serverSide || serverTotal == null) return;
    const tp = Math.max(1, Math.ceil(serverTotal / pageSize));
    if (page > tp) setPage(tp);
  }, [serverSide, serverTotal, pageSize, page]);

  // In server mode `rows` is the page and `serverTotal` is the filtered count;
  // in client mode we slice the locally-processed list.
  const totalFiltered = serverSide ? serverTotal ?? rows.length : processed.length;
  const totalPages = paginate
    ? Math.max(1, Math.ceil(totalFiltered / pageSize))
    : 1;
  const pageVal = paginate ? Math.min(page, totalPages) : 1;
  const start = paginate ? (pageVal - 1) * pageSize : 0;
  const pageRows = useMemo(
    () =>
      serverSide ? rows : paginate ? processed.slice(start, start + pageSize) : processed,
    [serverSide, rows, paginate, processed, start, pageSize],
  );
  // Follow the selection across (a) a selection change and (b) any change to the
  // result set — a poll/refresh that reorders rows, an upstream filter, rows
  // arriving late. `processed` is a dependency precisely so a refresh re-locates
  // the row; the follow anchor above is what keeps that from yanking a user who
  // has navigated elsewhere.
  //
  // Depending on `processed` also covers cross-page navigation (clicking a
  // fingerprint / rule chip lands here with `selectedKey` already set from the
  // URL): the table mounts before the RTK Query rows resolve, so the first run
  // finds nothing and the jump re-fires once the rows arrive.
  useEffect(() => {
    if (!selectedKey) {
      followRef.current = { key: null, page: followRef.current.page };
      return;
    }
    if (!followSelected || !paginate || serverSide) return;
    const index = indexOfSelected(selectedKey);
    if (index < 0) return; // not in the current result set — nothing to follow
    const target = Math.floor(index / pageSize) + 1;
    const selectionChanged = followRef.current.key !== selectedKey;
    // Still attached? (a brand-new selection always re-attaches)
    if (!selectionChanged && followRef.current.page !== pageVal) return;
    followRef.current = { key: selectedKey, page: target };
    if (target !== pageVal) setPage(target);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedKey, processed, pageVal, pageSize, paginate, serverSide, followSelected]);

  // Scroll the selected row back into view — the other half of "follow": paging
  // to the right page is useless if the row sits below the fold. Fires only when
  // the row's identity or its position actually changed (new selection, a jump,
  // or a re-sort that moved it), never on an idle poll, so a user who has
  // scrolled away from a still-put selection is left alone. `block: 'nearest'`
  // makes it a no-op when the row is already fully visible, which is exactly the
  // "re-focus" semantic; the row's `scroll-margin` clears the sticky header.
  // Runs in server mode too (the page is whatever the backend returned, but the
  // row is still on screen and still worth re-focusing).
  const scrollAnchor = useRef<string | null>(null);
  useEffect(() => {
    if (!followSelected || !selectedKey) {
      scrollAnchor.current = null;
      return;
    }
    const keyOf = rowKeyRef.current;
    const idx = pageRows.findIndex((row) => keyOf(row) === selectedKey);
    if (idx < 0) return;
    const anchor = `${selectedKey}\0${start + idx}`;
    if (scrollAnchor.current === anchor) return;
    scrollAnchor.current = anchor;
    const el = tableWrapRef.current?.querySelector<HTMLElement>(
      `tr[data-row-key="${CSS.escape(selectedKey)}"]`,
    );
    el?.scrollIntoView({ block: 'nearest', inline: 'nearest' });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedKey, pageRows, start, followSelected]);

  // Surface the on-screen rows so a synced sibling (e.g. the Trader Analysis
  // charts grid) can mirror the current sort/filter/page. Memoized `pageRows`
  // keeps this from firing every render.
  useEffect(() => {
    onVisibleRowsChange?.(pageRows);
  }, [pageRows, onVisibleRowsChange]);
  // Surface the full filtered cohort (not just the page) so a sibling summary
  // can aggregate over every matching row, mirroring a server-side summary.
  useEffect(() => {
    onFilteredRowsChange?.(processed);
  }, [processed, onFilteredRowsChange]);
  // Same-value tints (opt-in): computed over the on-screen page only, so the
  // palette stays scoped to what the eye is actually comparing. `rowKey` is
  // deliberately excluded from the deps — callers pass it inline, so depending
  // on it would rebuild the map on every render for no change in output.
  const valueTints = useMemo(() => {
    if (!sameValueTints) return null;
    return computeSameValueCellClasses(
      pageRows,
      rowKey,
      visCols.map((col) => ({ key: col.key, valueOf: (row: R) => cellGroupValue(col, row) })),
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sameValueTints, pageRows, visCols]);

  const colCount = visCols.length + 1 + (stableRowActions ? 1 : 0);

  const activeFilters = Object.values(colFiltersMap).filter(Boolean).length;

  const toggleSort = (key: string) => {
    const col = columns.find((c) => c.key === key);
    if (!col || !columnAcceptsSort(col)) return;
    setSortKeys((prev) => {
      const idx = prev.findIndex((s) => s.col === key);
      if (idx === -1) {
        // Not yet in sort list: append as lowest-priority at desc.
        return [...prev, { col: key, dir: 'desc' }];
      }
      if (prev[idx].dir === 'desc') {
        // desc → asc (in-place, same priority).
        return prev.map((s, i) => (i === idx ? { ...s, dir: 'asc' as SortDir } : s));
      }
      // asc → none: remove from list.
      return prev.filter((_, i) => i !== idx);
    });
  };

  // Stable identity so the memoized rows below don't re-render just because the
  // table re-rendered (e.g. on a poll). Reads `onSelect` through a ref so a
  // fresh parent callback never changes this identity.
  const selectRow = useCallback((key: string, isSelected: boolean) => {
    const next = isSelected ? null : key;
    setInternalSelected(next);
    onSelectRef.current?.(next);
  }, []);

  // Row pinning (opt-in via `pinnable` + `tableId`). The hook is inert without a
  // tableId, so this single call stays rules-of-hooks-safe for every table.
  const pinningEnabled = pinnable && !!tableId;
  // rowKeyRef keeps the hook from depending on an inline `rowKey` identity.
  const pinning = usePinnedRows(
    pinningEnabled ? tableId : undefined,
    rowKeyRef.current,
    pageRows,
  );
  const togglePin = pinning.onToggle; // stable (useCallback in the hook)

  // Pinned rows render (from snapshots) in a section above the paged body, on
  // every page. Any that also land on the current page are skipped below so a row
  // never shows twice. Kept as its own array so `pageRows`/`processed` — and every
  // callback that reads them — stay untouched; the dedup is display-only.
  const pinnedDisplay = pinning.pinnedRows;
  const pinsOpen = !pinsCollapsed;
  // The body only hides a pinned row while the section that re-shows it is open.
  // Collapsed, the row renders in its normal paged position instead of vanishing.
  const hidePinnedInBody = pinningEnabled && pinsOpen;
  const pageBodyCount = hidePinnedInBody
    ? pageRows.reduce((n, r) => {
        const k = rowKeyRef.current(r);
        return n + (pinning.isPinnedKey(k) ? 0 : 1);
      }, 0)
    : pageRows.length;
  const shownPinned = pinsOpen ? pinnedDisplay.length : 0;
  const bodyEmpty = shownPinned === 0 && pageBodyCount === 0;

  const columnPanelGroups = useMemo(() => {
    if (!showColPanel) return null;
    const grouped: { group: string; cols: ColumnDef<R>[] }[] = [];
    for (const col of columns) {
      if (col.sortOnly) continue;
      const g = col.group ?? '';
      const last = grouped[grouped.length - 1];
      if (last && last.group === g) last.cols.push(col);
      else grouped.push({ group: g, cols: [col] });
    }
    return grouped;
  }, [showColPanel, columns]);

  const toggleColVisibility = useCallback((key: string, checked: boolean) => {
    setVisibleCols((prev) => {
      const next = new Set(prev);
      if (checked) next.add(key);
      else next.delete(key);
      return next;
    });
  }, []);

  return (
    <div className="flex flex-col gap-0">
      <div className="mb-2 flex flex-wrap items-center gap-2">
        {toolbarLeading}
        {searchable && (
          <Input
            type="search"
            fieldSize="lg"
            variant="card"
            placeholder={searchPlaceholder}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            className="min-w-[200px] max-w-[340px] flex-1 border-white/8 focus:border-primary"
          />
        )}
        <span className="flex-1" />
        {toolbarTrailing}
        {colFilters && (
          <div className="flex items-center">
            <button
              type="button"
              onClick={() => setShowFilterRow((v) => !v)}
              className={cn(
                'rounded-md border border-white/8 bg-white/4 px-2.5 py-1 text-[11px] font-semibold uppercase tracking-wider text-text-dim transition hover:text-text',
                activeFilters > 0 ? 'rounded-r-none border-r-0' : '',
                (showFilterRow || activeFilters > 0) && 'border-primary/35 bg-primary/12 text-primary',
              )}
            >
              {activeFilters > 0 ? `Filters (${activeFilters})` : 'Filters'}
            </button>
            {activeFilters > 0 && (
              <button
                type="button"
                onClick={() => setColFiltersMap({})}
                title="Clear all filters"
                className="rounded-r-md border border-l-0 border-primary/35 bg-primary/12 px-1.5 py-1 text-[11px] text-primary transition hover:bg-primary/20 hover:text-white"
              >
                ✕
              </button>
            )}
          </div>
        )}
        {colToggle && (
          <button
            type="button"
            onClick={() => setShowColPanel((v) => !v)}
            className={cn(
              'rounded-md border border-white/8 bg-white/4 px-2.5 py-1 text-[11px] font-semibold uppercase tracking-wider text-text-dim transition hover:text-text',
              showColPanel && 'border-primary/35 bg-primary/12 text-primary',
            )}
          >
            Columns
          </button>
        )}
      </div>

      {colToggle && columnPanelGroups && (
          <div className="mb-2 flex flex-col gap-2 rounded-lg border border-white/7 bg-white/2 p-3">
            {columnPanelGroups.map(({ group, cols }) => (
              <div key={group || '__ungrouped'} className="flex flex-wrap items-center gap-x-4 gap-y-1">
                {group && (
                  <span className="w-20 shrink-0 text-[10px] font-bold uppercase tracking-wider text-secondary">
                    {group}
                  </span>
                )}
                {cols.map((col) => (
                  <label key={col.key} className="flex cursor-pointer items-center gap-2 text-xs text-text">
                    <Checkbox
                      checked={visibleCols.has(col.key)}
                      onChange={(e) => toggleColVisibility(col.key, e.target.checked)}
                    />
                    {col.label}
                  </label>
                ))}
              </div>
            ))}
          </div>
      )}

      <div
        ref={tableWrapRef}
        className="overflow-hidden rounded-lg shadow-[0_2px_12px_rgba(0,0,0,0.4),0_8px_32px_rgba(0,0,0,0.3)]"
      >
        <div className="overflow-x-auto border border-primary rounded-lg">
          <table className={cn('w-full border-collapse font-mono text-xs', hoverable && 'dt-colhover')}>
            <thead>
              {groupRuns.length > 0 && (
                // Not sticky (unlike the sort-header row below): on scroll it
                // slides up and the functional header pins at top-0 — two sticky
                // rows at top-0 would overlap.
                <tr>
                  <th className="dt-nohover bg-bg-panel" />
                  {groupRuns.map((run, ri) => (
                    <th
                      key={`g-${run.key}-${ri}`}
                      colSpan={run.span}
                      className={cn(
                        'dt-nohover bg-bg-panel px-2 pt-2 pb-1 text-center text-[10px] font-bold uppercase tracking-wider text-secondary',
                        ri > 0 && 'border-l border-white/10',
                        run.tinted && 'shadow-[inset_0_0_0_1000px_rgba(255,255,255,0.02)]',
                      )}
                    >
                      {run.label}
                    </th>
                  ))}
                  {stableRowActions && <th className="dt-nohover bg-bg-panel" />}
                </tr>
              )}
              <tr>
                <th className="sticky top-0 w-10 bg-bg-panel px-2 py-2.5 text-center text-[11px] font-semibold uppercase tracking-wider text-primary">
                  #
                </th>
                {visCols.map((col, ci) => (
                  <th
                    key={col.key}
                    style={col.width ? { width: col.width } : undefined}
                    title={col.tooltip}
                    className={cn(
                      'sticky top-0 bg-bg-panel px-2 py-2.5 text-[11px] font-semibold uppercase tracking-wider text-primary',
                      columnAcceptsSort(col) && !col.renderHeader && 'cursor-pointer hover:text-accent',
                      col.tooltip && 'decoration-dotted underline-offset-4 hover:underline',
                      groupClasses[ci],
                    )}
                    onClick={
                      columnAcceptsSort(col) && !col.renderHeader
                        ? () => toggleSort(col.key)
                        : undefined
                    }
                  >
                    {col.renderHeader ? (
                      col.renderHeader({ sortKeys, toggleSort })
                    ) : (
                      <>
                        {col.label}
                        {(() => {
                          const idx = sortKeys.findIndex((s) => s.col === col.key);
                          if (idx === -1) return null;
                          return (
                            <span className="ml-1 text-[10px]">
                              {sortKeys.length > 1 && (
                                <span className="opacity-50">{idx + 1}</span>
                              )}
                              {sortKeys[idx].dir === 'asc' ? '↑' : '↓'}
                            </span>
                          );
                        })()}
                      </>
                    )}
                  </th>
                ))}
                {stableRowActions && (
                  <th className={cn('sticky top-0 bg-bg-panel px-2 py-2.5 text-[11px] font-semibold uppercase tracking-wider text-primary', actionsCellCls)}>
                    Actions
                  </th>
                )}
              </tr>
              {colFilters && showFilterRow && (
                <tr>
                  <th className="bg-bg-panel px-1 py-1" />
                  {visCols.map((col, ci) => (
                    <th key={`f-${col.key}`} className={cn('bg-bg-panel px-1 py-1', groupClasses[ci])}>
                      {col.filterOptions ? (
                        <Select
                          fieldSize="table"
                          aria-label={`Filter ${col.label}`}
                          title={col.filterTitle}
                          value={colFiltersMap[col.key] ?? ''}
                          onChange={(e) =>
                            setColFiltersMap((m) => ({ ...m, [col.key]: e.target.value }))
                          }
                          className="border-white/8 focus:border-primary/40"
                        >
                          <option value="">All</option>
                          {col.filterOptions.map((opt) => (
                            <option key={opt.value} value={opt.value}>
                              {opt.label}
                            </option>
                          ))}
                        </Select>
                      ) : (
                        <Input
                          type="text"
                          fieldSize="table"
                          placeholder={
                            col.filterPlaceholder ??
                            (col.filterNumber ? '>0  1..5' : 'filter…')
                          }
                          title={
                            col.filterTitle ??
                            (col.filterNumber
                              ? 'Text matches; or use >  >=  <  <=  =  !=  or a range like 1..5'
                              : undefined)
                          }
                          value={colFiltersMap[col.key] ?? ''}
                          onChange={(e) =>
                            setColFiltersMap((m) => ({ ...m, [col.key]: e.target.value }))
                          }
                          className="border-white/8 focus:border-primary/40"
                        />
                      )}
                    </th>
                  ))}
                  {stableRowActions && <th className={cn('bg-bg-panel px-1 py-1', actionsCellCls)} />}
                </tr>
              )}
            </thead>
            <tbody>
              {/* Pinned section: floats above the page on every page. Rendered from
                  snapshots so a pinned row shows even when its page isn't loaded.
                  Its header row collapses the section (state persists per tableId). */}
              {pinnedDisplay.length > 0 && (
                <tr>
                  <td
                    // `dt-nohover`: a colSpan cell breaks the column-hover
                    // `nth-child` math, exactly as for the group-header banner.
                    className={cn(
                      'dt-nohover border-b border-border bg-primary/6 px-2 py-1',
                      // Collapsed, this row IS the bottom of the pinned section, so
                      // it carries the divider `pinnedLast` would otherwise draw.
                      !pinsOpen && 'border-b-2 border-b-primary/60',
                    )}
                    colSpan={colCount}
                  >
                    <button
                      type="button"
                      aria-expanded={pinsOpen}
                      title={pinsOpen ? 'Collapse pinned rows' : 'Expand pinned rows'}
                      onClick={() => setPinsCollapsed((v) => !v)}
                      className="flex items-center gap-1.5 font-sans text-[10px] font-semibold uppercase tracking-wider text-primary/75 transition-colors hover:text-primary"
                    >
                      <svg
                        className={cn(
                          'size-3 shrink-0 transition-transform duration-150',
                          pinsOpen && 'rotate-90',
                        )}
                        viewBox="0 0 20 20"
                        fill="none"
                        stroke="currentColor"
                        strokeWidth="2.5"
                        strokeLinecap="round"
                        strokeLinejoin="round"
                        aria-hidden
                      >
                        <polyline points="7,4 13,10 7,16" />
                      </svg>
                      <PinIcon className="size-3" />
                      {pinnedDisplay.length} pinned
                    </button>
                  </td>
                </tr>
              )}
              {pinsOpen && pinnedDisplay.map((row, i) => {
                const key = rowKeyRef.current(row);
                return (
                  <TableRow
                    key={`pin:${key}`}
                    row={row}
                    rowKeyValue={key}
                    index={-1}
                    visCols={visCols}
                    groupClasses={groupClasses}
                    selectable={selectable}
                    isSelected={selectedKey === key}
                    onSelectRow={selectRow}
                    rowActions={stableRowActions}
                    actionsCellCls={actionsCellCls}
                    rowDetail={stableRowDetail}
                    colCount={colCount}
                    rowClassName={stableRowClassName}
                    cellGroupClassName={stableCellGroupClassName}
                    valueTints={valueTints}
                    pinningEnabled
                    pinned
                    pinnedLast={i === pinnedDisplay.length - 1}
                    onTogglePin={togglePin}
                  />
                );
              })}
              {bodyEmpty ? (
                <tr>
                  <td colSpan={colCount} className="px-2 py-12 text-center font-sans text-text-dim">
                    {loading ? 'Loading…' : emptyMessage}
                  </td>
                </tr>
              ) : (
                pageRows.map((row, i) => {
                  // Skip the paged copy of a pinned row — it's shown in the section
                  // above. `index` stays the global position so `#` numbering is
                  // unchanged (a pinned row just leaves a gap).
                  const key = rowKeyRef.current(row);
                  if (hidePinnedInBody && pinning.isPinnedKey(key)) return null;
                  return (
                    <TableRow
                      key={key}
                      row={row}
                      rowKeyValue={key}
                      index={start + i}
                      visCols={visCols}
                      groupClasses={groupClasses}
                      selectable={selectable}
                      isSelected={selectedKey === key}
                      onSelectRow={selectRow}
                      rowActions={stableRowActions}
                      actionsCellCls={actionsCellCls}
                      rowDetail={stableRowDetail}
                      colCount={colCount}
                      rowClassName={stableRowClassName}
                      cellGroupClassName={stableCellGroupClassName}
                      valueTints={valueTints}
                      pinningEnabled={pinningEnabled}
                      onTogglePin={pinningEnabled ? togglePin : undefined}
                    />
                  );
                })
              )}
            </tbody>
          </table>
        </div>
      </div>

      {paginate && (
        <Pagination
          currentPage={pageVal}
          totalPages={totalPages}
          totalItems={totalFiltered}
          pageSize={pageSize}
          pageSizeOptions={pageSizeOptions}
          onPageChange={setPage}
          onPageSizeChange={(s) => {
            setPageSize(s);
            setPage(1);
          }}
        />
      )}
    </div>
  );
}

interface TableRowProps<R> {
  row: R;
  rowKeyValue: string;
  /** Zero-based global index → the `#` column shows `index + 1`. */
  index: number;
  visCols: ColumnDef<R>[];
  /** Precomputed, referentially-stable group class per visible column. */
  groupClasses: string[];
  selectable: boolean;
  isSelected: boolean;
  onSelectRow: (key: string, isSelected: boolean) => void;
  rowActions?: (row: R) => ReactNode;
  actionsCellCls: string;
  rowDetail?: (row: R) => ReactNode;
  colCount: number;
  rowClassName?: (row: R) => string | undefined;
  cellGroupClassName?: (group: string | undefined, row: R) => string | undefined;
  /** `${rowKey}\0${colKey}` → same-value background class; null when the tints
   *  are off. Identity is stable while the page + columns are unchanged, so it
   *  doesn't defeat this row's `memo`. */
  valueTints?: Map<string, string> | null;
  /** Pinning is active for this table → the `#` cell shows a pin toggle on hover. */
  pinningEnabled?: boolean;
  /** This row is currently pinned (rendered in the section above the page). */
  pinned?: boolean;
  /** This is the last pinned row → draw the divider under the pinned section. */
  pinnedLast?: boolean;
  /** Flip this row's pinned state. Stable identity (see `togglePin`). */
  onTogglePin?: (row: R) => void;
}

/** Module-level so its identity is stable — it's a prop on a memoized row. */
const SELECTED_SCROLL_MARGIN = { scrollMarginTop: '7rem', scrollMarginBottom: '3rem' } as const;

/**
 * A single table row, extracted and memoized so that a re-render of the table
 * (most often a poll handing back a fresh page) only re-renders the rows that
 * actually changed. RTK Query's structural sharing preserves the object
 * identity of unchanged rows across fetches, so their `row` prop stays
 * referentially equal and `memo` skips them. Hover (shared `hoveredCol`) and
 * selection still re-render the affected rows, exactly as before.
 */
function TableRowInner<R>({
  row,
  rowKeyValue,
  index,
  visCols,
  groupClasses,
  selectable,
  isSelected,
  onSelectRow,
  rowActions,
  actionsCellCls,
  rowDetail,
  colCount,
  rowClassName,
  cellGroupClassName,
  valueTints,
  pinningEnabled,
  pinned,
  pinnedLast,
  onTogglePin,
}: TableRowProps<R>) {
  const selected = isSelected && selectable;
  // Selection and pinning must never read as the same thing. Selection rides the
  // ACCENT hue (orange — identical in both app skins, and used nowhere by
  // pinning): a solid left rail plus a full-width bottom edge. Pinning keeps its
  // faint PRIMARY wash + the primary pin glyph, so at a glance the two are a
  // different colour AND a different shape, not two strengths of one tint.
  //
  // Both accents live on the CELLS, not on the `<tr>`: with
  // `border-collapse: collapse` a row-level `box-shadow` is never painted, so the
  // old selected glow and the pinned-section divider were dead rules. Cell
  // borders and inset shadows do render — that's how the column-group tints work.
  const edgeCls = selected
    ? 'border-b-2 border-b-accent/70'
    : pinnedLast
      ? 'border-b-2 border-b-primary/60'
      : undefined;
  const railCls = selected ? 'shadow-[inset_4px_0_0_0_var(--color-accent)]' : undefined;
  return (
    <Fragment>
      <tr
        data-row-key={rowKeyValue}
        // The `thead` pins at `top-0` over the page scroll, so a plain
        // scroll-into-view can park the row underneath it. The margin reserves
        // the header band (group banner + sort row + filter row).
        style={isSelected ? SELECTED_SCROLL_MARGIN : undefined}
        onClick={
          selectable
            ? (e) => {
                // A button (or anything acting like one) inside a cell — e.g. a
                // row action rendered directly in a data column, not just the
                // dedicated Actions column — fires its own click semantics.
                // Without this guard that click bubbles to the row and toggles
                // selection underneath it.
                if ((e.target as HTMLElement).closest('button, a, [role="button"]')) return;
                onSelectRow(rowKeyValue, isSelected);
              }
            : undefined
        }
        className={cn(
          selectable && 'cursor-pointer',
          'transition-colors hover:bg-primary/12',
          pinned && 'bg-primary/4.5',
          // Ordered after `pinned` on purpose: `bg-*` collapses to one winner
          // under tailwind-merge, so a pinned row that is ALSO selected has to
          // resolve to the selection tint, not the (much fainter) pin wash.
          selected && 'bg-accent/16 hover:bg-accent/22',
          rowClassName?.(row),
        )}
      >
        {pinningEnabled ? (
          // The pin toggle lives in the `#` cell (an always-visible pushpin next to
          // the row number), so it needs no extra column — which would otherwise
          // shift the `nth-child` column-hover math and every column's group class.
          // Dim when unpinned (brightens on hover), primary when pinned.
          <td
            className={cn(
              'border-b border-border px-2 py-1.5 text-[11px] text-text-dim',
              edgeCls,
              railCls,
            )}
          >
            <div className="flex items-center justify-center gap-1.5">
              <button
                type="button"
                title={pinned ? 'Unpin row' : 'Pin row'}
                aria-label={pinned ? 'Unpin row' : 'Pin row'}
                aria-pressed={pinned}
                onClick={(e) => {
                  e.stopPropagation();
                  onTogglePin?.(row);
                }}
                className={cn(
                  'inline-flex shrink-0 items-center justify-center transition-colors',
                  pinned ? 'text-primary' : 'text-text-dim/45 hover:text-text',
                )}
              >
                <PinIcon className="size-3.5" />
              </button>
              <span className="tabular-nums">{index >= 0 ? index + 1 : ''}</span>
            </div>
          </td>
        ) : (
          <td
            className={cn(
              'border-b border-border px-2 py-1.5 text-center text-[11px] text-text-dim',
              edgeCls,
              railCls,
            )}
          >
            {index + 1}
          </td>
        )}
        {visCols.map((col, ci) => (
          <td
            key={col.key}
            className={cn(
              'border-b border-border px-2 py-1.5 text-center text-text',
              groupClasses[ci],
              cellGroupClassName?.(col.group, row),
              valueTints?.get(`${rowKeyValue}\0${col.key}`),
              col.cellClassName?.(row),
              // Last: `border-b-*` must outlive the group divider's `border-white/10`
              // (a whole-border colour, which tailwind-merge would otherwise drop it for).
              edgeCls,
            )}
          >
            {col.render(row)}
          </td>
        ))}
        {rowActions && (
          <td
            className={cn(
              'border-b border-border px-2 py-1.5 text-center align-middle',
              actionsCellCls,
              edgeCls,
            )}
            onClick={(e) => e.stopPropagation()}
          >
            {rowActions(row)}
          </td>
        )}
      </tr>
      {isSelected && rowDetail && (
        <tr className="bg-[rgba(15,23,42,0.88)]">
          <td colSpan={colCount} className="p-0">
            {/* The accent rail carries down the expanded detail, so the panel
                reads as belonging to the selected row above it. */}
            <div
              id={`detail-${rowKeyValue}`}
              className="border-t border-white/6 border-l-4 border-l-accent/70 bg-bg-panel p-3"
            >
              {rowDetail(row)}
            </div>
          </td>
        </tr>
      )}
    </Fragment>
  );
}

// `memo` erases the generic; the cast restores the parameterized call signature.
const TableRow = memo(TableRowInner) as typeof TableRowInner;
