import { Fragment, memo, useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { cn } from 'lib/cn';
import { Checkbox } from 'components/ui/Checkbox';
import { Input } from 'components/ui/Input';
import { Pagination, DEFAULT_PAGE_SIZE } from './Pagination';
import { parseNumericPredicate } from './numericFilter';
import { getTableCols, setTableCols, getTablePrefs, setTablePrefs } from 'lib/storage';
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
  // Matches the old `bg-primary/12` tint (--color-primary #13ceaf at 12%).
  el.textContent = `${sels.join(',')}{background-color:rgba(19,206,175,0.12)}`;
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

function loadVisibleCols(tableId: string, columns: ColumnDef<unknown>[]): Set<string> {
  const defaults = new Set(columns.filter((c) => c.defaultVisible !== false).map((c) => c.key));
  const stored = getTableCols(tableId);
  if (!stored) return defaults;
  const set = new Set(stored.filter((k) => columns.some((c) => c.key === k)));
  return set.size ? set : defaults;
}

function saveVisibleCols(tableId: string, cols: Set<string>) {
  setTableCols(tableId, [...cols]);
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
  colFilters?: boolean;
  colToggle?: boolean;
  hoverable?: boolean;
  /** Stable id for persisting this table's column visibility (folded into the
   *  shared `mt:table.cols` map). Omit to not persist column toggles. */
  tableId?: string;
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
  colFilters = false,
  colToggle = false,
  hoverable = true,
  tableId,
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
    if (tableId) {
      const prefs = getTablePrefs(tableId);
      if (prefs.sortKeys) return prefs.sortKeys;
      // Backward compat: migrate old single-sort prefs written by a previous build.
      if (prefs.sortCol) return [{ col: prefs.sortCol, dir: prefs.sortDir ?? 'asc' }];
    }
    if (defaultSort?.col) return [{ col: defaultSort.col, dir: defaultSort.dir ?? 'asc' }];
    return [];
  });
  const [search, setSearch] = useState('');
  const [colFiltersMap, setColFiltersMap] = useState<Record<string, string>>({});
  const [visibleCols, setVisibleCols] = useState<Set<string>>(() =>
    tableId ? loadVisibleCols(tableId, columns as ColumnDef<unknown>[]) : new Set(columns.filter((c) => c.defaultVisible !== false).map((c) => c.key)),
  );
  const [internalSelected, setInternalSelected] = useState<string | null>(null);
  const [showColPanel, setShowColPanel] = useState(false);
  const [showFilterRow, setShowFilterRow] = useState(false);
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

  useEffect(() => {
    if (tableId) saveVisibleCols(tableId, visibleCols);
  }, [visibleCols, tableId]);

  useEffect(() => {
    if (tableId) setTablePrefs(tableId, { pageSize, sortKeys });
  }, [tableId, pageSize, sortKeys]);

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

  // Resolve each active column filter once (not per row): a numeric column whose
  // filter text is a comparison/range expression (`>5`, `1..10`) gets a numeric
  // predicate; everything else matches the displayed text as a substring.
  const activeColFilters = useMemo(() => {
    const out: {
      col: ColumnDef<R>;
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
      const numeric = col.filterNumber ? parseNumericPredicate(text) : null;
      out.push({ col, needle: text.toLowerCase(), numeric });
    }
    return out;
  }, [serverSide, debouncedColFilters, columns]);

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
        const hit = columns.some((col) =>
          col.searchValue(row).toLowerCase().includes(searchLower),
        );
        if (!hit) return false;
      }
      for (const { col, needle, numeric } of activeColFilters) {
        if (numeric) {
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
        list = [...list].sort((a, b) => {
          for (const { sv, dir } of levels) {
            const cmp = compareSort(sv(a), sv(b), dir);
            if (cmp !== 0) return cmp;
          }
          return 0;
        });
      }
    }
    return list;
  }, [serverSide, rows, columns, debouncedSearch, activeColFilters, sortKeys]);

  // Reset to page 1 when the filter/sort/pageSize view changes. Selection changes
  // are intentionally excluded: deselecting a row should not scroll the user back
  // to page 1, and jumping to a newly-selected row is handled by the effect below.
  // `processed` and `rowKey` are deliberately NOT dependencies — a poll hands back
  // a new `rows`/`processed` identity, so depending on them would reset the page
  // out from under the user on every refresh.
  useEffect(() => {
    if (!paginate || serverSide) return;
    setPage(1);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [debouncedSearch, debouncedColFilters, sortKeys, pageSize, paginate, serverSide]);

  // Jump to the selected row's page when a row is selected (selectedKey becomes
  // truthy or changes to a different key). Deselection (→ null) is a no-op here
  // so the page stays where it was. `processed` and `rowKey` are intentionally
  // excluded (poll-driven identity churn); `pageSize` is included so the jump
  // recalculates correctly if pageSize also changed in the same render.
  useEffect(() => {
    if (!paginate || serverSide || !selectedKey) return;
    const index = processed.findIndex((row) => rowKey(row) === selectedKey);
    if (index >= 0) {
      setPage(Math.floor(index / pageSize) + 1);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedKey, pageSize, paginate, serverSide]);

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
  const viewSig = useMemo(
    () =>
      `${resetKey ?? ''}|${pageSize}|${JSON.stringify(sortKeys)}|${debouncedSearch}|${JSON.stringify(cleanedColFilters)}`,
    [resetKey, pageSize, sortKeys, debouncedSearch, cleanedColFilters],
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
    onQueryChange?.({
      page: p,
      pageSize,
      sortKeys,
      search: debouncedSearch,
      colFilters: cleanedColFilters,
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [serverSide, viewSig, page]);

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
  const pageRows = serverSide
    ? rows
    : paginate
      ? processed.slice(start, start + pageSize)
      : processed;
  const colCount = visCols.length + 1 + (rowActions ? 1 : 0);

  const activeFilters = Object.values(colFiltersMap).filter(Boolean).length;

  const toggleSort = (key: string) => {
    setSortKeys((prev) => {
      const idx = prev.findIndex((s) => s.col === key);
      if (idx === -1) {
        // Not yet in sort list: append as lowest-priority at asc.
        return [...prev, { col: key, dir: 'asc' }];
      }
      if (prev[idx].dir === 'asc') {
        // asc → desc (in-place, same priority).
        return prev.map((s, i) => (i === idx ? { ...s, dir: 'desc' as SortDir } : s));
      }
      // desc → none: remove from list.
      return prev.filter((_, i) => i !== idx);
    });
  };

  // Stable identity so the memoized rows below don't re-render just because the
  // table re-rendered (e.g. on a poll).
  const selectRow = useCallback(
    (key: string, isSelected: boolean) => {
      const next = isSelected ? null : key;
      setInternalSelected(next);
      onSelect?.(next);
    },
    [onSelect],
  );

  return (
    <div className="flex flex-col gap-0">
      <div className="mb-2 flex flex-wrap items-center gap-2">
        {searchable && (
          <Input
            type="search"
            fieldSize="lg"
            variant="card"
            placeholder="Search…"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            className="min-w-[200px] max-w-[340px] flex-1 border-white/8 focus:border-primary"
          />
        )}
        <span className="flex-1" />
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

      {colToggle && showColPanel && (
        <div className="mb-2 flex flex-wrap gap-3 rounded-lg border border-white/7 bg-white/2 p-3">
          {columns.map((col) => (
            <label key={col.key} className="flex cursor-pointer items-center gap-2 text-xs text-text">
              <Checkbox
                checked={visibleCols.has(col.key)}
                onChange={(e) => {
                  setVisibleCols((prev) => {
                    const next = new Set(prev);
                    if (e.target.checked) next.add(col.key);
                    else next.delete(col.key);
                    return next;
                  });
                }}
              />
              {col.label}
            </label>
          ))}
        </div>
      )}

      <div className="overflow-hidden rounded-lg shadow-[0_2px_12px_rgba(0,0,0,0.4),0_8px_32px_rgba(0,0,0,0.3)]">
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
                  {rowActions && <th className="dt-nohover bg-bg-panel" />}
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
                      col.sortable !== false && !col.renderHeader && 'cursor-pointer hover:text-accent',
                      col.tooltip && 'decoration-dotted underline-offset-4 hover:underline',
                      groupClasses[ci],
                    )}
                    onClick={
                      col.sortable !== false && !col.renderHeader
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
                {rowActions && (
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
                      <Input
                        type="text"
                        fieldSize="table"
                        placeholder={col.filterNumber ? '>0  1..5' : 'filter…'}
                        title={col.filterNumber ? 'Text matches; or use >  >=  <  <=  =  !=  or a range like 1..5' : undefined}
                        value={colFiltersMap[col.key] ?? ''}
                        onChange={(e) =>
                          setColFiltersMap((m) => ({ ...m, [col.key]: e.target.value }))
                        }
                        className="border-white/8 focus:border-primary/40"
                      />
                    </th>
                  ))}
                  {rowActions && <th className={cn('bg-bg-panel px-1 py-1', actionsCellCls)} />}
                </tr>
              )}
            </thead>
            <tbody>
              {pageRows.length === 0 ? (
                <tr>
                  <td colSpan={colCount} className="px-2 py-12 text-center font-sans text-text-dim">
                    {loading ? 'Loading…' : emptyMessage}
                  </td>
                </tr>
              ) : (
                pageRows.map((row, i) => {
                  const key = rowKey(row);
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
                      rowActions={rowActions}
                      actionsCellCls={actionsCellCls}
                      rowDetail={rowDetail}
                      colCount={colCount}
                      rowClassName={rowClassName}
                      cellGroupClassName={cellGroupClassName}
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
}

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
}: TableRowProps<R>) {
  return (
    <Fragment>
      <tr
        onClick={selectable ? () => onSelectRow(rowKeyValue, isSelected) : undefined}
        className={cn(
          selectable && 'cursor-pointer',
          'transition-colors hover:bg-primary/12',
          isSelected && selectable && 'bg-primary/18 shadow-[0_14px_32px_rgba(2,192,118,0.06)]',
          rowClassName?.(row),
        )}
      >
        <td className="border-b border-border px-2 py-1.5 text-center text-[11px] text-text-dim">
          {index + 1}
        </td>
        {visCols.map((col, ci) => (
          <td
            key={col.key}
            className={cn(
              'border-b border-border px-2 py-1.5 text-center text-text',
              groupClasses[ci],
              cellGroupClassName?.(col.group, row),
              col.cellClassName?.(row),
            )}
          >
            {col.render(row)}
          </td>
        ))}
        {rowActions && (
          <td
            className={cn('border-b border-border px-2 py-1.5 text-center', actionsCellCls)}
            onClick={(e) => e.stopPropagation()}
          >
            {rowActions(row)}
          </td>
        )}
      </tr>
      {isSelected && rowDetail && (
        <tr className="bg-[rgba(15,23,42,0.88)]">
          <td colSpan={colCount} className="p-0">
            <div id={`detail-${rowKeyValue}`} className="border-t border-white/6 bg-bg-panel p-3">
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
