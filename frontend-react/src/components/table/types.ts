import type { ReactNode } from 'react';

export type SortDir = 'asc' | 'desc';

export type SortValue = number | string | null;

export interface ColumnDef<R> {
  key: string;
  label: string;
  /** Optional hover tooltip for the column header (native title). */
  tooltip?: string;
  render: (row: R) => ReactNode;
  sortValue?: (row: R) => SortValue;
  /** Text used by the global search box. Numeric columns may return '' to opt out. */
  searchValue: (row: R) => string;
  /**
   * Text used by the per-column filter row. Falls back to `searchValue` when
   * omitted. Define this (rather than relying on `searchValue`) for columns
   * whose `searchValue` is '' but that should still be filterable on their
   * displayed value.
   */
  filterValue?: (row: R) => string;
  /**
   * Numeric value (in the column's *displayed* units) used by the per-column
   * filter row for comparison/range expressions like `>5`, `<=10`, `1..5`.
   * When set, such expressions filter numerically; plain text still falls back
   * to substring matching on `filterValue`/`searchValue`. Return null for rows
   * with no value (they are excluded by any numeric expression).
   */
  filterNumber?: (row: R) => number | null;
  sortable?: boolean;
  defaultVisible?: boolean;
  width?: string;
  group?: string;
}
