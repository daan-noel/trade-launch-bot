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
  searchValue: (row: R) => string;
  sortable?: boolean;
  defaultVisible?: boolean;
  width?: string;
  group?: string;
}
