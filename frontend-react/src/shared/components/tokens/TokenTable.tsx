import { useMemo, useState, type ReactNode } from 'react';
import { DataTable } from 'components/table/DataTable';
import { DEFAULT_PAGE_SIZE } from 'components/table/Pagination';
import type { ColumnDef, TableQuery } from 'components/table/types';
import { appendedTokenColumns, mergeTokenData } from './sharedTokenColumns';
import { numericColKeys, toTableRequest } from 'services/tableRequest';
import { applyTableRequest, columnResolver } from 'services/tableEval';
import type { TokenRecord } from 'types';

/**
 * Client-resident, token-enriched table — the composition layer that sits ON TOP
 * of the pure {@link DataTable} primitive and owns the "token recipe":
 *
 *   1. append the shared token-info columns (`appendedTokenColumns`, SSOT) after
 *      the caller's own columns;
 *   2. overlay full token metadata onto each row (`mergeTokenData`, row wins);
 *   3. evaluate the unified server-table contract IN BROWSER (`applyTableRequest`
 *      via `columnResolver`) — for tables whose live columns (value/price/…) can't
 *      be server-paged, so paging/sort/filter run client-side with the SAME
 *      op/sort/search semantics as the SQL + Rust paths.
 *
 * `DataTable` stays a general-purpose primitive with NO token vocabulary; every
 * token concern lives here (or in the sibling column/filter helpers). This one-way
 * dependency (`tokens/` → `table/`, never the reverse) is asserted by the guard
 * test `components/table/DataTable.boundary.test.ts`.
 *
 * Use this for any table whose rows carry a `mint` and should show the full token
 * field set from a client-fetched `tokenMap` (e.g. Wallet Holdings). Server-side
 * tables whose rows arrive already enriched from the backend (positions/matched/
 * sim) render on the raw `DataTable` and do NOT need this wrapper.
 */
const INITIAL_QUERY: TableQuery = {
  page: 1,
  pageSize: DEFAULT_PAGE_SIZE,
  sortKeys: [],
  search: '',
  colFilters: {},
};

export interface TokenTableProps<R extends { mint: string }> {
  /** The caller's own (bespoke) columns. The shared token-info columns are
   *  appended after these; keys already present are skipped via `existingKeys`. */
  columns: ColumnDef<R>[];
  /** Rows carrying a `mint`; enriched from `tokenMap` before display. */
  rows: R[];
  /** mint → full token record, used to enrich each row (row fields win on overlap). */
  tokenMap: Map<string, TokenRecord>;
  /** Column keys already rendered by `columns` (plus semantic aliases) — excluded
   *  from the appended token columns so enrichment never duplicates a column. */
  existingKeys: Set<string>;
  rowKey: (row: R) => string;
  rowActions?: (row: R) => ReactNode;
  selectedKey?: string | null;
  onSelect?: (key: string | null) => void;
  selectable?: boolean;
  searchable?: boolean;
  colFilters?: boolean;
  colToggle?: boolean;
  hoverable?: boolean;
  loading?: boolean;
  tableId?: string;
  emptyMessage?: string;
  defaultPageSize?: number;
  pageSizeOptions?: number[];
}

export function TokenTable<R extends { mint: string }>({
  columns,
  rows,
  tokenMap,
  existingKeys,
  rowKey,
  ...rest
}: TokenTableProps<R>) {
  // Caller's columns + the shared token-info columns (single definition, overlaid
  // show/hide). The runtime rows are merged to carry these fields (see below).
  const cols = useMemo<ColumnDef<R>[]>(
    () => [...columns, ...appendedTokenColumns(existingKeys)],
    [columns, existingKeys],
  );
  // Overlay full token metadata onto each row; the row's own fields win, so a
  // fresher live value (price, flags) always supersedes the DB copy.
  const enriched = useMemo(() => mergeTokenData(rows, tokenMap), [rows, tokenMap]);

  // Client-side evaluation of the unified table contract: the DataTable emits its
  // view-state, we serialize it to the `TableRequest` body and evaluate in-browser
  // with the same grammar the Rust/SQL paths use. `columnResolver` derives that
  // grammar straight from the (already-appended) columns, so the token columns are
  // sortable/filterable too.
  const [query, setQuery] = useState<TableQuery>(INITIAL_QUERY);
  const numericCols = useMemo(() => numericColKeys(cols), [cols]);
  const resolver = useMemo(() => columnResolver(cols), [cols]);
  const { items, total } = useMemo(
    () => applyTableRequest(enriched, toTableRequest(query, numericCols), resolver),
    [enriched, query, numericCols, resolver],
  );

  return (
    <DataTable
      columns={cols}
      rows={items}
      rowKey={rowKey}
      serverSide
      serverTotal={total}
      onQueryChange={setQuery}
      {...rest}
    />
  );
}
