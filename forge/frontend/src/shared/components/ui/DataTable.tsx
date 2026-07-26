import { memo, useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import clsx from 'clsx';
import { Icon } from './Icon';
import { parseNumericPredicate } from './numericFilter';
import type { PinnedRows } from './usePinnedRows';

export interface Column<T> {
  header: ReactNode;
  render: (row: T) => ReactNode;
  /** Right-align numeric columns. */
  align?: 'left' | 'right';
  className?: string;
  /**
   * Opt-in per-column filter (rendered only when the table is `filterable`).
   * Return the substring text this column matches on — the filter input then
   * does a case-insensitive `includes` against it.
   */
  filterValue?: (row: T) => string;
  /**
   * Opt-in numeric per-column filter (rendered only when the table is
   * `filterable`). Return the value in DISPLAYED units (SOL as SOL, percent as
   * percent, counts as integers) or `null` for "no value". Enables the compare/
   * range grammar (`>5`, `<=10`, `1..5`, `=`, `!=`); non-numeric text falls back
   * to substring matching on the stringified value.
   */
  filterNumber?: (row: T) => number | null;
  /** Placeholder for this column's filter input (defaults per accessor kind). */
  filterPlaceholder?: string;
}

/**
 * The list-table shell every page shares: sticky header, hover rows, loading +
 * empty states, optional row click. `colSpan` on the empty/loading row is derived
 * from `columns.length` so it never drifts when a column is added.
 *
 * Rows are extracted into a memoized {@link DataRow} so a table re-render (most
 * often a poll handing back a fresh page) only re-renders the rows that actually
 * changed: RTK Query's structural sharing keeps the object identity of unchanged
 * rows across fetches, so their `row` prop stays referentially equal and `memo`
 * skips them. For this to bite, the caller must pass a **referentially stable**
 * `columns` array (wrap it in `useMemo`) — otherwise every row sees a new
 * `columns` identity each render. `onRowClick` identity is stabilized here via a
 * ref, so callers may pass an inline handler without breaking the memo.
 *
 * Pass `pinning` (from {@link usePinnedRows}) to add a leading pin-toggle column and
 * a sticky pinned section that floats above the paged body on every page. Pinned
 * rows are rendered from the hook's snapshots and deduped out of the paged body, so
 * a row that also lands on the current page is never shown twice. Omit `pinning` and
 * the table renders exactly as before.
 *
 * Pass `filterable` to render an opt-in per-column filter row under the header:
 * every column that declares `filterValue` / `filterNumber` gets a debounced input
 * and the body is filtered client-side (numeric columns understand `>5`, `1..10`,
 * `=`, `!=`, …; others substring-match). Columns with no filter accessor get an
 * empty filter cell. Default off — an existing table is byte-for-byte unchanged
 * unless it opts in AND at least one column declares a filter accessor. Pinned rows
 * always float above the (filtered) body, unaffected by the filter.
 */
export function DataTable<T>({
  columns,
  rows,
  rowKey,
  loading,
  empty,
  onRowClick,
  maxHeight,
  pinning,
  filterable = false,
}: {
  columns: Column<T>[];
  rows: T[];
  rowKey: (row: T, i: number) => string;
  loading?: boolean;
  empty?: ReactNode;
  onRowClick?: (row: T) => void;
  maxHeight?: number;
  pinning?: PinnedRows<T>;
  filterable?: boolean;
}) {
  // Keep a stable click handler identity across renders so the memoized rows
  // don't re-render just because the parent handed a fresh inline `onRowClick`.
  const clickRef = useRef(onRowClick);
  clickRef.current = onRowClick;
  const handleClick = useCallback((row: T) => clickRef.current?.(row), []);
  const clickable = !!onRowClick;

  // Same trick for the pin toggle so the pin cell stays memo-stable.
  const toggleRef = useRef(pinning?.onToggle);
  toggleRef.current = pinning?.onToggle;
  const handleToggle = useCallback((row: T) => toggleRef.current?.(row), []);

  const showPin = !!pinning;
  const pinnedRows = pinning?.pinnedRows ?? [];
  const isPinned = pinning?.isPinned;
  const colCount = columns.length + (showPin ? 1 : 0);

  // Per-column filter state, indexed by column POSITION (forge columns have no
  // key). `filters` mirrors the live inputs; `debouncedFilters` drives the actual
  // filtering so a burst of keystrokes coalesces into one recompute (~275ms).
  const [filters, setFilters] = useState<Record<number, string>>({});
  const [debouncedFilters, setDebouncedFilters] = useState<Record<number, string>>({});
  useEffect(() => {
    const id = setTimeout(() => setDebouncedFilters(filters), 275);
    return () => clearTimeout(id);
  }, [filters]);

  // The feature only exists when the caller opts in AND at least one column can
  // be filtered — otherwise the whole filter row is skipped (nothing to change).
  const hasFilterable =
    filterable && columns.some((c) => c.filterValue || c.filterNumber);

  // Resolve each active column filter once (not per row): a numeric column whose
  // text parses as a comparison/range gets a numeric predicate; otherwise it's a
  // substring needle on the column's displayed text (or stringified number).
  const activeFilters = useMemo(() => {
    const out: {
      col: Column<T>;
      needle: string;
      numeric: ((n: number) => boolean) | null;
    }[] = [];
    if (!hasFilterable) return out;
    for (const [idx, raw] of Object.entries(debouncedFilters)) {
      const text = raw.trim();
      if (!text) continue;
      const col = columns[Number(idx)];
      if (!col || (!col.filterValue && !col.filterNumber)) continue;
      const numeric = col.filterNumber ? parseNumericPredicate(text) : null;
      out.push({ col, needle: text.toLowerCase(), numeric });
    }
    return out;
  }, [hasFilterable, debouncedFilters, columns]);

  // Client-side body filter. Pinned rows are untouched (they float above from
  // snapshots); only the paged body reduces to the matching set.
  const displayRows = useMemo(() => {
    if (activeFilters.length === 0) return rows;
    return rows.filter((row) => {
      for (const { col, needle, numeric } of activeFilters) {
        if (numeric && col.filterNumber) {
          const n = col.filterNumber(row);
          if (n == null || !numeric(n)) return false;
        } else if (col.filterValue) {
          if (!col.filterValue(row).toLowerCase().includes(needle)) return false;
        } else if (col.filterNumber) {
          // Numeric column, but the text isn't a numeric expression — substring
          // match against the stringified value so "12" still narrows.
          const n = col.filterNumber(row);
          if (n == null || !String(n).toLowerCase().includes(needle)) return false;
        }
      }
      return true;
    });
  }, [rows, activeFilters]);

  return (
    <div className="overflow-auto rounded-lg border border-[var(--color-line)]" style={{ maxHeight }}>
      <table className="dt">
        <thead>
          <tr>
            {showPin && <th className="dt-pin-col" aria-label="Pin" />}
            {columns.map((c, i) => (
              <th key={i} className={clsx(c.align === 'right' && 'text-right', c.className)}>
                {c.header}
              </th>
            ))}
          </tr>
          {hasFilterable && (
            <tr>
              {showPin && <th className="dt-filter-th dt-pin-col" aria-hidden="true" />}
              {columns.map((c, i) => {
                const canFilter = !!(c.filterValue || c.filterNumber);
                return (
                  <th key={i} className={clsx('dt-filter-th', c.className)}>
                    {canFilter && (
                      <input
                        type="text"
                        className="input dt-filter-input"
                        value={filters[i] ?? ''}
                        placeholder={
                          c.filterPlaceholder ?? (c.filterNumber ? '>0  1..5' : 'filter…')
                        }
                        title={
                          c.filterNumber
                            ? 'Text matches; or use >  >=  <  <=  =  !=  or a range like 1..5'
                            : undefined
                        }
                        aria-label={
                          typeof c.header === 'string' ? `Filter ${c.header}` : 'Filter column'
                        }
                        onChange={(e) =>
                          setFilters((m) => ({ ...m, [i]: e.target.value }))
                        }
                      />
                    )}
                  </th>
                );
              })}
            </tr>
          )}
        </thead>
        {pinnedRows.length > 0 && (
          <tbody className="dt-pinned">
            {pinnedRows.map((row, i) => (
              <DataRow
                key={`pin:${rowKey(row, i)}`}
                row={row}
                columns={columns}
                clickable={clickable}
                onClick={handleClick}
                showPin
                pinned
                onTogglePin={handleToggle}
              />
            ))}
          </tbody>
        )}
        <tbody>
          {loading && displayRows.length === 0 && (
            <tr>
              <td colSpan={colCount} className="muted py-6 text-center">
                Loading…
              </td>
            </tr>
          )}
          {!loading && displayRows.length === 0 && pinnedRows.length === 0 && (
            <tr>
              <td colSpan={colCount} className="muted py-6 text-center">
                {activeFilters.length > 0 ? 'No rows match the filters.' : empty ?? 'No rows yet.'}
              </td>
            </tr>
          )}
          {displayRows.map((row, i) => {
            // A pinned row already shows in the sticky section above — skip its
            // paged copy so it never renders twice.
            if (isPinned?.(row)) return null;
            return (
              <DataRow
                key={rowKey(row, i)}
                row={row}
                columns={columns}
                clickable={clickable}
                onClick={handleClick}
                showPin={showPin}
                pinned={false}
                onTogglePin={handleToggle}
              />
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

function DataRowInner<T>({
  row,
  columns,
  clickable,
  onClick,
  showPin,
  pinned,
  onTogglePin,
}: {
  row: T;
  columns: Column<T>[];
  clickable: boolean;
  onClick: (row: T) => void;
  showPin: boolean;
  pinned: boolean;
  onTogglePin: (row: T) => void;
}) {
  return (
    <tr
      className={clsx(clickable && 'clickable')}
      onClick={clickable ? () => onClick(row) : undefined}
    >
      {showPin && (
        <td className="dt-pin-col">
          <button
            type="button"
            className={clsx('dt-pin', pinned && 'dt-pin--on')}
            title={pinned ? 'Unpin row' : 'Pin row'}
            aria-label={pinned ? 'Unpin row' : 'Pin row'}
            aria-pressed={pinned}
            onClick={(e) => {
              e.stopPropagation(); // don't trigger row navigation
              onTogglePin(row);
            }}
          >
            <Icon name="pin" size={14} />
          </button>
        </td>
      )}
      {columns.map((c, j) => (
        <td key={j} className={clsx(c.align === 'right' && 'text-right', c.className)}>
          {c.render(row)}
        </td>
      ))}
    </tr>
  );
}

// `memo` erases the generic; the cast restores the parameterized call signature.
const DataRow = memo(DataRowInner) as typeof DataRowInner;
