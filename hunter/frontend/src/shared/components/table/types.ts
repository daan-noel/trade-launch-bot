import type { ReactNode } from 'react';
import type { AmountStorageUnit } from 'lib/priceUnitSnapshot';
import type { FilterSpec } from './numericFilter';

export type SortDir = 'asc' | 'desc';

export type SortValue = number | string | null;

/** One level in a multi-key sort (primary, secondary, …). */
export type SortEntry = { col: string; dir: SortDir };

/**
 * The full view-state a server-side DataTable emits when anything that affects
 * the result set changes (paging, sort, search, per-column filters). The parent
 * translates this into API query args. `search`/`colFilters` are already
 * debounced by the table before emission.
 */
export interface TableQuery {
  page: number;
  pageSize: number;
  /** Ordered sort keys (index 0 = primary). Empty = unsorted. */
  sortKeys: SortEntry[];
  search: string;
  colFilters: Record<string, string>;
  /**
   * Pre-structured filters injected by a wrapper around the table (NOT typed into
   * the per-column text inputs) — e.g. a pasted mint set as an `in` op. Merged into
   * the request `filters` alongside the parsed `colFilters` (structured wins on key
   * collision). `DataTable` never sets this itself; it stays token-agnostic.
   */
  structuredFilters?: Record<string, FilterSpec>;
}

/**
 * Live sort state + the toggle action, handed to a column's `renderHeader` so a
 * custom header can drive the table's sort (e.g. a merged "metrics" cell whose
 * header offers one sort toggle per underlying field). `toggleSort(key)` cycles
 * the key through asc → desc → none (removed) exactly like a header click — and
 * `key` may be any column in the table, including a hidden sort-only column.
 */
export interface SortCtx {
  /** Ordered sort keys (index 0 = primary). Empty = unsorted. */
  sortKeys: SortEntry[];
  toggleSort: (key: string) => void;
}

export interface ColumnDef<R> {
  key: string;
  label: string;
  /** Optional hover tooltip for the column header (native title). */
  tooltip?: string;
  render: (row: R) => ReactNode;
  /**
   * Optional per-row class for this column's `<td>` (e.g. a value-based cell
   * tint). Applied on top of the group even/odd tint, so it controls the cell's
   * full background — not just the rendered content's bounding box, the way a
   * `render`-level span would. Return undefined to leave the cell untinted.
   */
  cellClassName?: (row: R) => string | undefined;
  /**
   * Custom header content. When set, replaces the default label + sort-arrow and
   * the header's click-to-sort (the column is expected to be non-sortable on its
   * own key). Receives the table's sort state so it can render its own controls.
   */
  renderHeader?: (ctx: SortCtx) => ReactNode;
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
   *
   * For PriceUnit-converted amount columns, return
   * `amountInDisplayUnit(storage, filterAmount)` and set {@link filterAmount}
   * so server-side serializers can convert the typed operand back to storage.
   */
  filterNumber?: (row: R) => number | null;
  /**
   * Storage currency for amount columns whose cells convert via PriceUnit.
   * When set, `toTableRequest` treats typed filter operands as *displayed*
   * units and converts them to this storage unit before the server compare.
   */
  filterAmount?: AmountStorageUnit;
  /**
   * Whether the column may join the multi-key sort. Defaults off unless the
   * column also provides `sortValue` (which implies client-side sortability).
   * Set `true` explicitly for server-side-only sorts that have no client
   * comparator; set `false` to keep a `sortValue` column unsortable.
   */
  sortable?: boolean;
  defaultVisible?: boolean;
  /**
   * Sort-key-only column: participates in multi-key sort (via `sortValue`) but
   * stays out of the Columns panel and defaults hidden. Used with a sibling
   * column's `renderHeader` that calls `toggleSort(this.key)`.
   */
  sortOnly?: boolean;
  width?: string;
  group?: string;
}
