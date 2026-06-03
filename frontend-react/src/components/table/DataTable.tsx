import { Fragment, useEffect, useMemo, useState, type ReactNode } from 'react';
import { cn } from '../../lib/cn';
import { Pagination, DEFAULT_PAGE_SIZE } from './Pagination';
import type { ColumnDef, SortDir, SortValue } from './types';

function loadVisibleCols(storageKey: string, columns: ColumnDef<unknown>[]): Set<string> {
  const defaults = new Set(columns.filter((c) => c.defaultVisible !== false).map((c) => c.key));
  try {
    const raw = localStorage.getItem(storageKey);
    if (!raw) return defaults;
    const parsed = JSON.parse(raw) as string[];
    const set = new Set(parsed.filter((k) => columns.some((c) => c.key === k)));
    return set.size ? set : defaults;
  } catch {
    return defaults;
  }
}

function saveVisibleCols(storageKey: string, cols: Set<string>) {
  try {
    localStorage.setItem(storageKey, JSON.stringify([...cols]));
  } catch {
    /* ignore */
  }
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
  storageKey?: string;
  emptyMessage?: string;
  selectable?: boolean;
  paginate?: boolean;
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
  pageSizeOptions = [10, 25, 50, 100],
  searchable = true,
  colFilters = false,
  colToggle = false,
  hoverable = true,
  storageKey,
  emptyMessage = 'No data.',
  selectable = true,
  paginate = true,
}: DataTableProps<R>) {
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(defaultPageSize);
  const [sortCol, setSortCol] = useState<string | null>(null);
  const [sortDir, setSortDir] = useState<SortDir>('asc');
  const [search, setSearch] = useState('');
  const [colFiltersMap, setColFiltersMap] = useState<Record<string, string>>({});
  const [visibleCols, setVisibleCols] = useState<Set<string>>(() =>
    storageKey ? loadVisibleCols(storageKey, columns as ColumnDef<unknown>[]) : new Set(columns.filter((c) => c.defaultVisible !== false).map((c) => c.key)),
  );
  const [internalSelected, setInternalSelected] = useState<string | null>(null);
  const [showColPanel, setShowColPanel] = useState(false);
  const [showFilterRow, setShowFilterRow] = useState(false);
  const [hoveredCol, setHoveredCol] = useState<number | null>(null);

  const selectedKey = externalSelected ?? internalSelected;

  useEffect(() => {
    if (storageKey) saveVisibleCols(storageKey, visibleCols);
  }, [visibleCols, storageKey]);

  useEffect(() => setPage(1), [search, colFiltersMap, sortCol, sortDir]);

  const visCols = useMemo(
    () => columns.filter((c) => visibleCols.has(c.key)),
    [columns, visibleCols],
  );

  const processed = useMemo(() => {
    const searchLower = search.toLowerCase();
    let list = rows.filter((row) => {
      if (searchLower) {
        const hit = columns.some((col) =>
          col.searchValue(row).toLowerCase().includes(searchLower),
        );
        if (!hit) return false;
      }
      for (const [key, text] of Object.entries(colFiltersMap)) {
        if (!text) continue;
        const col = columns.find((c) => c.key === key);
        if (col && !col.searchValue(row).toLowerCase().includes(text.toLowerCase())) {
          return false;
        }
      }
      return true;
    });

    if (sortCol) {
      const col = columns.find((c) => c.key === sortCol);
      if (col?.sortValue) {
        const sv = col.sortValue;
        list = [...list].sort((a, b) => compareSort(sv(a), sv(b), sortDir));
      }
    }
    return list;
  }, [rows, columns, search, colFiltersMap, sortCol, sortDir]);

  const totalFiltered = processed.length;
  const totalPages = paginate
    ? Math.max(1, Math.ceil(totalFiltered / pageSize))
    : 1;
  const pageVal = paginate ? Math.min(page, totalPages) : 1;
  const start = paginate ? (pageVal - 1) * pageSize : 0;
  const pageRows = paginate ? processed.slice(start, start + pageSize) : processed;
  const colCount = visCols.length + 1 + (rowActions ? 1 : 0);

  const activeFilters = Object.values(colFiltersMap).filter(Boolean).length;

  const toggleSort = (key: string) => {
    if (sortCol === key) setSortDir((d) => (d === 'asc' ? 'desc' : 'asc'));
    else {
      setSortCol(key);
      setSortDir('asc');
    }
  };

  const selectRow = (key: string, isSelected: boolean) => {
    const next = isSelected ? null : key;
    setInternalSelected(next);
    onSelect?.(next);
  };

  return (
    <div className="flex flex-col gap-0">
      <div className="mb-2 flex flex-wrap items-center gap-2">
        {searchable && (
          <input
            type="search"
            placeholder="Search…"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            className="min-w-[200px] max-w-[340px] flex-1 rounded-lg border border-white/8 bg-bg-card px-3 py-1.5 text-[13px] text-text outline-none transition focus:border-primary"
          />
        )}
        <span className="flex-1" />
        {colFilters && (
          <button
            type="button"
            onClick={() => setShowFilterRow((v) => !v)}
            className={cn(
              'rounded-md border border-white/8 bg-white/4 px-2.5 py-1 text-[11px] font-semibold uppercase tracking-wider text-text-dim transition hover:text-text',
              (showFilterRow || activeFilters > 0) && 'border-primary/35 bg-primary/12 text-primary',
            )}
          >
            {activeFilters > 0 ? `Filters (${activeFilters})` : 'Filters'}
          </button>
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
              <input
                type="checkbox"
                checked={visibleCols.has(col.key)}
                onChange={(e) => {
                  setVisibleCols((prev) => {
                    const next = new Set(prev);
                    if (e.target.checked) next.add(col.key);
                    else next.delete(col.key);
                    return next;
                  });
                }}
                className="accent-primary"
              />
              {col.label}
            </label>
          ))}
        </div>
      )}

      <div className="overflow-hidden rounded-lg shadow-[0_2px_12px_rgba(0,0,0,0.4),0_8px_32px_rgba(0,0,0,0.3)]">
        <div className="overflow-x-auto border border-primary rounded-lg">
          <table className="w-full border-collapse font-mono text-xs">
            <thead>
              <tr>
                <th className="sticky top-0 w-10 bg-bg-panel px-2 py-2.5 text-center text-[11px] font-semibold uppercase tracking-wider text-primary">
                  #
                </th>
                {visCols.map((col, ci) => (
                  <th
                    key={col.key}
                    style={col.width ? { width: col.width } : undefined}
                    className={cn(
                      'sticky top-0 bg-bg-panel px-2 py-2.5 text-[11px] font-semibold uppercase tracking-wider text-primary',
                      col.sortable !== false && 'cursor-pointer hover:text-accent',
                      hoverable && hoveredCol === ci + 1 && 'bg-primary/12',
                    )}
                    onClick={col.sortable !== false ? () => toggleSort(col.key) : undefined}
                    onMouseEnter={hoverable ? () => setHoveredCol(ci + 1) : undefined}
                    onMouseLeave={hoverable ? () => setHoveredCol(null) : undefined}
                  >
                    {col.label}
                    {sortCol === col.key && (
                      <span className="ml-1 text-[10px]">{sortDir === 'asc' ? '↑' : '↓'}</span>
                    )}
                  </th>
                ))}
                {rowActions && (
                  <th className="sticky top-0 bg-bg-panel px-2 py-2.5 text-[11px] font-semibold uppercase tracking-wider text-primary">
                    Actions
                  </th>
                )}
              </tr>
              {colFilters && showFilterRow && (
                <tr>
                  <th className="bg-bg-panel px-1 py-1" />
                  {visCols.map((col) => (
                    <th key={`f-${col.key}`} className="bg-bg-panel px-1 py-1">
                      <input
                        type="text"
                        placeholder="filter…"
                        value={colFiltersMap[col.key] ?? ''}
                        onChange={(e) =>
                          setColFiltersMap((m) => ({ ...m, [col.key]: e.target.value }))
                        }
                        className="w-full rounded border border-white/8 bg-white/4 px-1.5 py-0.5 text-[11px] text-text outline-none focus:border-primary/40"
                      />
                    </th>
                  ))}
                  {rowActions && <th className="bg-bg-panel px-1 py-1" />}
                </tr>
              )}
            </thead>
            <tbody>
              {pageRows.length === 0 ? (
                <tr>
                  <td colSpan={colCount} className="px-2 py-12 text-center font-sans text-text-dim">
                    {emptyMessage}
                  </td>
                </tr>
              ) : (
                pageRows.map((row, i) => {
                  const key = rowKey(row);
                  const isSelected = selectedKey === key;
                  const globalI = start + i;
                  return (
                    <Fragment key={key}>
                      <tr
                        onClick={selectable ? () => selectRow(key, isSelected) : undefined}
                        className={cn(
                          selectable && 'cursor-pointer',
                          'transition-colors hover:bg-primary/12',
                          isSelected && selectable && 'bg-primary/18 shadow-[0_14px_32px_rgba(2,192,118,0.06)]',
                        )}
                      >
                        <td className="border-b border-border px-2 py-1.5 text-center text-[11px] text-text-dim">
                          {globalI + 1}
                        </td>
                        {visCols.map((col, ci) => (
                          <td
                            key={col.key}
                            className={cn(
                              'border-b border-border px-2 py-1.5 text-center text-text',
                              hoverable && hoveredCol === ci + 1 && 'bg-primary/12',
                            )}
                            onMouseEnter={hoverable ? () => setHoveredCol(ci + 1) : undefined}
                            onMouseLeave={hoverable ? () => setHoveredCol(null) : undefined}
                          >
                            {col.render(row)}
                          </td>
                        ))}
                        {rowActions && (
                          <td
                            className="border-b border-border px-2 py-1.5 text-center"
                            onClick={(e) => e.stopPropagation()}
                          >
                            {rowActions(row)}
                          </td>
                        )}
                      </tr>
                      {isSelected && rowDetail && (
                        <tr className="bg-[rgba(15,23,42,0.88)]">
                          <td colSpan={colCount} className="p-0">
                            <div id={`detail-${key}`} className="border-t border-white/6 bg-bg-panel p-3">
                              {rowDetail(row)}
                            </div>
                          </td>
                        </tr>
                      )}
                    </Fragment>
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
