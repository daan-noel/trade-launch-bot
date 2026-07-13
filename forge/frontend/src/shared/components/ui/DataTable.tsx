import { memo, useCallback, useRef, type ReactNode } from 'react';
import clsx from 'clsx';

export interface Column<T> {
  header: ReactNode;
  render: (row: T) => ReactNode;
  /** Right-align numeric columns. */
  align?: 'left' | 'right';
  className?: string;
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
 */
export function DataTable<T>({
  columns,
  rows,
  rowKey,
  loading,
  empty,
  onRowClick,
  maxHeight,
}: {
  columns: Column<T>[];
  rows: T[];
  rowKey: (row: T, i: number) => string;
  loading?: boolean;
  empty?: ReactNode;
  onRowClick?: (row: T) => void;
  maxHeight?: number;
}) {
  // Keep a stable click handler identity across renders so the memoized rows
  // don't re-render just because the parent handed a fresh inline `onRowClick`.
  const clickRef = useRef(onRowClick);
  clickRef.current = onRowClick;
  const handleClick = useCallback((row: T) => clickRef.current?.(row), []);
  const clickable = !!onRowClick;

  return (
    <div className="overflow-auto rounded-lg border border-[var(--color-line)]" style={{ maxHeight }}>
      <table className="dt">
        <thead>
          <tr>
            {columns.map((c, i) => (
              <th key={i} className={clsx(c.align === 'right' && 'text-right', c.className)}>
                {c.header}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {loading && rows.length === 0 && (
            <tr>
              <td colSpan={columns.length} className="muted py-6 text-center">
                Loading…
              </td>
            </tr>
          )}
          {!loading &&
            rows.length === 0 &&
            (
              <tr>
                <td colSpan={columns.length} className="muted py-6 text-center">
                  {empty ?? 'No rows yet.'}
                </td>
              </tr>
            )}
          {rows.map((row, i) => (
            <DataRow
              key={rowKey(row, i)}
              row={row}
              columns={columns}
              clickable={clickable}
              onClick={handleClick}
            />
          ))}
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
}: {
  row: T;
  columns: Column<T>[];
  clickable: boolean;
  onClick: (row: T) => void;
}) {
  return (
    <tr
      className={clsx(clickable && 'clickable')}
      onClick={clickable ? () => onClick(row) : undefined}
    >
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
