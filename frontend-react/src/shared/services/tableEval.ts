// Client-side in-memory evaluator for the shared `TableRequest` contract — the TS
// half of the "one shared method" for token tables whose rows are already resident
// in the browser (Wallet Holdings today; any future client-fed token table).
//
// It embodies the SAME op semantics as the Rust `trading_core::api::table_eval`
// evaluator and the SQL path: numeric operators compare numerically, a numeric op
// on a text column is dropped, a null numeric field can't satisfy a numeric
// predicate, `contains` on a number degrades to equality, free-text search is
// mint/symbol only, and ties break by raw (case-sensitive) `mint` ASC so paging is
// stable. The two evaluators are held at parity by a shared-fixture conformance
// test (see the Rust module's tests + the planned Phase-8 check).
//
// Callers own the **grammar** (which frontend column key maps to which row value +
// type) and hand it in as a `resolve` closure. `columnResolver` derives that
// grammar straight from a table's `ColumnDef[]` — numeric iff the column declares
// `filterNumber`, sorted by its `sortValue` — so it mirrors exactly what the
// client-side `DataTable` did before, just routed through the unified request body.

import type { ColumnDef, SortValue } from 'components/table/types';
import type { FilterSpec } from 'components/table/numericFilter';
import type { TableRequestBody } from './tableRequest';

/** Column type for a row value — decides which filter ops are legal and how the
 *  value compares (string contains/eq vs numeric compare), mirroring the Rust
 *  `ColKind`. */
export type ColKind = 'text' | 'number';

/** How the evaluator reads one column's value from a row. `num` is present for
 *  `number` columns (display-unit numeric filter value), `text` for `text`
 *  columns; `sort` is the sort key (compared with {@link compareSort}). */
export interface ColGrammar<R> {
  kind: ColKind;
  num?: (row: R) => number | null;
  text?: (row: R) => string | null;
  sort: (row: R) => SortValue;
}

/** Resolves a frontend column key to its grammar, or `null` when the key isn't
 *  filterable/sortable (dropped, exactly as an unknown key is dropped server-side). */
export type ColResolver<R> = (key: string) => ColGrammar<R> | null;

/** The two fields free-text search scans, everywhere (locked project decision — see
 *  CLAUDE.md / Phase 6). Kept in lock-step with the Rust evaluator's `SEARCH_FIELDS`. */
const SEARCH_KEYS = ['mint', 'symbol'] as const;

/** Coerce a filter operand to a number (number or numeric string); null otherwise. */
function operandNum(v: unknown): number | null {
  if (typeof v === 'number') return Number.isFinite(v) ? v : null;
  if (typeof v === 'string') {
    const t = v.trim();
    if (!t) return null;
    const n = Number(t);
    return Number.isFinite(n) ? n : null;
  }
  return null;
}

/** Coerce a filter operand to a lowercased string (number stringified); null otherwise. */
function operandText(v: unknown): string | null {
  if (typeof v === 'string') return v.trim().toLowerCase();
  if (typeof v === 'number') return String(v);
  return null;
}

/**
 * Does `row` satisfy one structured filter? A shape/type mismatch (numeric op on a
 * text field, non-numeric operand, missing bound) is treated as **not a
 * constraint** → keeps the row, mirroring the SQL/Rust side dropping the predicate.
 */
function rowMatches<R>(row: R, g: ColGrammar<R>, spec: FilterSpec): boolean {
  if (g.kind === 'text') {
    if (spec.op === 'contains' || spec.op === 'eq') {
      const needle = operandText(spec.val);
      if (needle == null || needle === '') return true;
      const hay = g.text?.(row);
      if (hay == null) return false;
      const h = hay.toLowerCase();
      return spec.op === 'contains' ? h.includes(needle) : h === needle;
    }
    // Numeric op (incl. `between`) on a text field → not a constraint.
    return true;
  }
  // number
  if (spec.op === 'between') {
    const min = operandNum(spec.min);
    const max = operandNum(spec.max);
    if (min == null || max == null) return true;
    const v = g.num?.(row) ?? null;
    return v != null && v >= min && v <= max;
  }
  const target = operandNum(spec.val);
  if (target == null) return true;
  const v = g.num?.(row) ?? null;
  // Field absent/null can't satisfy a numeric predicate.
  if (v == null) return false;
  switch (spec.op) {
    case 'gt':
      return v > target;
    case 'gte':
      return v >= target;
    case 'lt':
      return v < target;
    case 'lte':
      return v <= target;
    // `eq`/`contains` on a numeric field → equality.
    default:
      return v === target;
  }
}

/** Does `row` match the free-text search over mint / symbol? Blank search (already
 *  trimmed+lowercased by the caller) matches everything. */
function matchesSearch<R>(row: R, needle: string, resolve: ColResolver<R>): boolean {
  if (!needle) return true;
  return SEARCH_KEYS.some((k) => {
    const hay = resolve(k)?.text?.(row);
    return hay != null && hay.toLowerCase().includes(needle);
  });
}

/**
 * Compare two sort keys with the SAME semantics as the client-side `DataTable`:
 * nulls last (both directions), numbers numerically, everything else by
 * `localeCompare`.
 */
export function compareSort(a: SortValue, b: SortValue, desc: boolean): number {
  if (a == null && b == null) return 0;
  if (a == null) return 1;
  if (b == null) return -1;
  let cmp: number;
  if (typeof a === 'number' && typeof b === 'number') cmp = a - b;
  else cmp = String(a).localeCompare(String(b));
  return desc ? -cmp : cmp;
}

/** Raw (case-sensitive) mint string for the stable tiebreak — mirrors the SQL/Rust
 *  `mint ASC` tail, which does NOT lowercase. `''` when absent. */
function mintRaw<R>(row: R, resolve: ColResolver<R>): string {
  const v = resolve('mint')?.sort(row);
  return typeof v === 'string' ? v : v == null ? '' : String(v);
}

/**
 * Apply the shared `TableRequest` (search → filters → sort → page) to in-memory
 * rows, using `resolve` for the column grammar. Returns one page of rows plus the
 * total match count (before paging) for the pager.
 *
 * Sort applies every resolved sort key in order (unknown keys dropped), then a
 * deterministic raw-`mint` ASC tail so ties don't shuffle across page seams. It
 * runs even with no sort key, so the default view pages stably too.
 */
export function applyTableRequest<R>(
  rows: R[],
  req: TableRequestBody,
  resolve: ColResolver<R>,
): { items: R[]; total: number } {
  const needle = req.search.trim().toLowerCase();

  // Resolve filters once (key → grammar), dropping unknown keys.
  const filters: [ColGrammar<R>, FilterSpec][] = [];
  for (const [key, spec] of Object.entries(req.filters)) {
    const g = resolve(key);
    if (g) filters.push([g, spec]);
  }

  const matched = rows.filter(
    (row) => matchesSearch(row, needle, resolve) && filters.every(([g, spec]) => rowMatches(row, g, spec)),
  );
  const total = matched.length;

  // Resolve all sort keys once (drop unknown), then sort by them in order followed
  // by the raw-`mint` ASC tiebreak.
  const sortKeys: [ColGrammar<R>, boolean][] = [];
  for (const { col, dir } of req.sorting) {
    const g = resolve(col);
    if (g) sortKeys.push([g, dir === 'desc']);
  }
  const sorted = [...matched].sort((a, b) => {
    for (const [g, desc] of sortKeys) {
      const cmp = compareSort(g.sort(a), g.sort(b), desc);
      if (cmp !== 0) return cmp;
    }
    const ma = mintRaw(a, resolve);
    const mb = mintRaw(b, resolve);
    return ma < mb ? -1 : ma > mb ? 1 : 0;
  });

  // Page — mirror `Page::bounds`: pageSize clamped 1..1000, offset = (page-1)*limit.
  const limit = Math.min(Math.max(req.pagination.pageSize, 1), 1000);
  const offset = (Math.max(req.pagination.page, 1) - 1) * limit;
  return { items: sorted.slice(offset, offset + limit), total };
}

/**
 * Build a {@link ColResolver} from a table's own `ColumnDef[]`. A column filters
 * **numerically** iff it declares `filterNumber` (matching `numericColKeys` /
 * `toTableRequest`); every other column filters as a substring on
 * `filterValue ?? searchValue`. Sorting always uses the column's `sortValue`, so a
 * numeric column keeps sorting numerically even when it isn't numeric-filterable —
 * preserving exactly what the client-side `DataTable` did.
 *
 * This is the single client evaluator's grammar source: the live-only Wallet
 * columns (value/price/liquidity/…) are handled uniformly here, no SQL builder to
 * exclude them from (the whole evaluation is client-resident).
 */
export function columnResolver<R>(columns: ColumnDef<R>[]): ColResolver<R> {
  const byKey = new Map(columns.map((c) => [c.key, c]));
  const noSort = () => null;
  return (key) => {
    const c = byKey.get(key);
    if (!c) return null;
    if (c.filterNumber) {
      return { kind: 'number', num: c.filterNumber, sort: c.sortValue ?? noSort };
    }
    const text = c.filterValue ?? c.searchValue;
    return { kind: 'text', text: (row) => text(row), sort: c.sortValue ?? noSort };
  };
}
