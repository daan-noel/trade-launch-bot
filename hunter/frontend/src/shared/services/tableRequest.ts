// Single serializer from a `DataTable` view-state (`TableQuery`) to the unified
// server-side table request body (POST + JSON), shared by every strategy token
// table (Positions / Paper / Matched / Simulated). One shape, so the frontend
// never forks Matched/Simulated onto a different wire format from Positions.
//
// The key part is turning each raw per-column filter string into a structured
// `FilterSpec`: for a **numeric** column, `parseFilterSpec` reads `>5` / `1..10`
// into `{op:'gt',val:5}` / `{op:'between',min,max}` so the compare happens
// numerically server-side; everything else is `{op:'contains', val}` (the current
// ILIKE substring behavior).

import type { ColumnDef } from 'components/table/types';
import type { TableQuery } from 'components/table/types';
import { parseFilterSpec, type FilterSpec } from 'components/table/numericFilter';
import {
  amountToStorageUnit,
  type AmountStorageUnit,
} from 'lib/priceUnitSnapshot';
import {
  isExitReasonFilterKey,
  normalizeExitReasonFilter,
} from 'lib/strategy/exitReason';

/** The unified request body (mirrors `trading_core::api::table_query::TableRequest`). */
export interface TableRequestBody {
  pagination: { page: number; pageSize: number };
  sorting: { col: string; dir: 'asc' | 'desc' }[];
  search: string;
  filters: Record<string, FilterSpec>;
  /** Matched / simulated only — the analysis window. */
  range?: { from?: string; to?: string };
  /** Tokens list only — restrict to the live cache-tracked subset. */
  trackedOnly?: boolean;
}

/** Extras merged into the body (e.g. the matched/simulated analysis `range`). */
export interface TableRequestExtras {
  range?: { from?: string; to?: string };
  /**
   * Column key → storage currency for PriceUnit-converted amount columns.
   * Typed filter operands are interpreted in the *displayed* unit and converted
   * to storage before the server compare. Use {@link amountColKeys}.
   * Structured filters (wrapper-injected) are left alone — they are already in
   * storage units (e.g. wallet dust `$1`).
   */
  amountCols?: ReadonlyMap<string, AmountStorageUnit>;
}

/** Convert a structured numeric filter's operands from display → storage units. */
function specToStorage(spec: FilterSpec, storageUnit: AmountStorageUnit): FilterSpec {
  if (spec.op === 'between') {
    const min = typeof spec.min === 'number' ? amountToStorageUnit(spec.min, storageUnit) : spec.min;
    const max = typeof spec.max === 'number' ? amountToStorageUnit(spec.max, storageUnit) : spec.max;
    return { op: 'between', min, max };
  }
  if (spec.op === 'in') return spec;
  if (typeof spec.val === 'number') {
    return { ...spec, val: amountToStorageUnit(spec.val, storageUnit) };
  }
  return spec;
}

/**
 * Serialize a `DataTable`'s emitted `TableQuery` into the POST body.
 *
 * `numericCols` is the set of column keys that filter numerically (columns whose
 * `ColumnDef` declares `filterNumber`) — for those, a recognized numeric
 * expression becomes a structured `{op,val}` / `between` spec; anything else (and
 * every non-numeric column) becomes `{op:'contains', val}`. Use
 * {@link numericColKeys} to derive the set from a column list.
 *
 * Reason / Exit Reason columns also expand badge aliases (`TP` → `TakeProfit`,
 * `Open` → `Open`) so the server matches the persisted `exit_reason` string.
 */
export function toTableRequest(
  query: TableQuery,
  numericCols: ReadonlySet<string>,
  extras?: TableRequestExtras,
): TableRequestBody {
  const filters: Record<string, FilterSpec> = {};
  const amountCols = extras?.amountCols;
  for (const [key, raw] of Object.entries(query.colFilters)) {
    const text = raw.trim();
    if (!text) continue;
    // Numeric column + a recognized numeric expression → structured spec.
    // Amount columns also try a numeric parse when listed in `amountCols` even
    // if `numericCols` was left empty (Tokens list path), so PriceUnit conversion
    // can still rewrite the operand before the backend re-parse.
    const storageUnit = amountCols?.get(key);
    const spec =
      numericCols.has(key) || storageUnit != null ? parseFilterSpec(text) : null;
    if (spec) {
      filters[key] = storageUnit != null ? specToStorage(spec, storageUnit) : spec;
    } else if (isExitReasonFilterKey(key)) {
      filters[key] = { op: 'contains', val: normalizeExitReasonFilter(text) };
    } else {
      filters[key] = { op: 'contains', val: text };
    }
  }
  // Wrapper-injected structured filters (e.g. a mint-set `in` op) AND with the
  // per-column filters; on a key collision the structured spec wins.
  if (query.structuredFilters) {
    for (const [key, spec] of Object.entries(query.structuredFilters)) {
      filters[key] = spec;
    }
  }
  const body: TableRequestBody = {
    pagination: { page: query.page, pageSize: query.pageSize },
    sorting: query.sortKeys.map((s) => ({ col: s.col, dir: s.dir })),
    search: query.search,
    filters,
  };
  if (extras?.range) body.range = extras.range;
  return body;
}

/** Filter/search-only slice of a table POST body — for summary endpoints that
 *  aggregate the whole filtered population (pagination/sort ignored). */
export function toSummaryBody(
  query: TableQuery,
  numericCols: ReadonlySet<string>,
  extras?: TableRequestExtras,
): TableRequestBody {
  const full = toTableRequest(
    { ...query, page: 1, pageSize: 1, sortKeys: [] },
    numericCols,
    extras,
  );
  return {
    pagination: { page: 1, pageSize: 1000 },
    sorting: [],
    search: full.search,
    filters: full.filters,
    ...(full.range ? { range: full.range } : {}),
  };
}

/** The set of column keys that filter numerically (declare `filterNumber`) — the
 *  `numericCols` argument to {@link toTableRequest}. */
export function numericColKeys<R>(columns: ColumnDef<R>[]): Set<string> {
  return new Set(columns.filter((c) => c.filterNumber).map((c) => c.key));
}

/** Column key → storage currency for PriceUnit amount columns (`filterAmount`). */
export function amountColKeys<R>(
  columns: ColumnDef<R>[],
): Map<string, AmountStorageUnit> {
  const m = new Map<string, AmountStorageUnit>();
  for (const c of columns) {
    if (c.filterAmount) m.set(c.key, c.filterAmount);
  }
  return m;
}

/**
 * Whether the query reduces its result set — i.e. a search string, any per-column
 * filter, or a wrapper-injected structured filter (e.g. a mint set) is active.
 * Sorting/paging don't count (they don't shrink the population). Callers that
 * hide an empty server-side table must keep it mounted while this is true, or the
 * table (with its filter controls) vanishes the moment a filter yields 0 rows and
 * the user can no longer change the filter.
 */
export function isTableQueryActive(query: TableQuery): boolean {
  return (
    query.search.trim().length > 0 ||
    Object.values(query.colFilters).some((v) => v.trim().length > 0) ||
    Object.keys(query.structuredFilters ?? {}).length > 0
  );
}
