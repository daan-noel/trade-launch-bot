import { memo, useCallback, useRef, type ReactNode } from 'react';
import clsx from 'clsx';
import { Icon } from './Icon';
import type { PinnedRows } from './usePinnedRows';

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
 *
 * Pass `pinning` (from {@link usePinnedRows}) to add a leading pin-toggle column and
 * a sticky pinned section that floats above the paged body on every page. Pinned
 * rows are rendered from the hook's snapshots and deduped out of the paged body, so
 * a row that also lands on the current page is never shown twice. Omit `pinning` and
 * the table renders exactly as before.
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
}: {
  columns: Column<T>[];
  rows: T[];
  rowKey: (row: T, i: number) => string;
  loading?: boolean;
  empty?: ReactNode;
  onRowClick?: (row: T) => void;
  maxHeight?: number;
  pinning?: PinnedRows<T>;
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
          {loading && rows.length === 0 && (
            <tr>
              <td colSpan={colCount} className="muted py-6 text-center">
                Loading…
              </td>
            </tr>
          )}
          {!loading && rows.length === 0 && pinnedRows.length === 0 && (
            <tr>
              <td colSpan={colCount} className="muted py-6 text-center">
                {empty ?? 'No rows yet.'}
              </td>
            </tr>
          )}
          {rows.map((row, i) => {
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
