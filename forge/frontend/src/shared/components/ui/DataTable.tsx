import { ReactNode } from 'react';
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
            <tr
              key={rowKey(row, i)}
              className={clsx(onRowClick && 'clickable')}
              onClick={onRowClick ? () => onRowClick(row) : undefined}
            >
              {columns.map((c, j) => (
                <td key={j} className={clsx(c.align === 'right' && 'text-right', c.className)}>
                  {c.render(row)}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
