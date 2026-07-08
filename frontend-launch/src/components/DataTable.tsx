import { ReactNode } from 'react';

export interface Column<T> {
  header: ReactNode;
  render: (row: T) => ReactNode;
}

/**
 * The list-table shell every management tab repeated: header row, body,
 * loading state, and the "no rows yet" empty row. The empty row's `colSpan` is
 * derived from `columns.length` (the tabs hand-wrote it and it drifted after a
 * column was added). Provide a stable `rowKey`.
 */
export function DataTable<T>({
  columns,
  rows,
  rowKey,
  loading,
  empty,
}: {
  columns: Column<T>[];
  rows: T[];
  rowKey: (row: T) => string;
  loading?: boolean;
  empty?: ReactNode;
}) {
  if (loading) return <p className="muted">Loading…</p>;
  return (
    <table>
      <thead>
        <tr>
          {columns.map((c, i) => (
            <th key={i}>{c.header}</th>
          ))}
        </tr>
      </thead>
      <tbody>
        {rows.map((row) => (
          <tr key={rowKey(row)}>
            {columns.map((c, i) => (
              <td key={i}>{c.render(row)}</td>
            ))}
          </tr>
        ))}
        {rows.length === 0 && (
          <tr>
            <td colSpan={columns.length} className="muted">
              {empty ?? 'No rows yet.'}
            </td>
          </tr>
        )}
      </tbody>
    </table>
  );
}
