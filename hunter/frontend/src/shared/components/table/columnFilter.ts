import type { ColumnDef } from './types';
import { parseNumericPredicate } from './numericFilter';

/**
 * One column filter as a row predicate, exactly as the client-side table applies
 * it: the column's own `filterMatch`, else option equality (`filterOptions`), else
 * a numeric predicate (`>5`, `1..10`) on `filterNumber`, else a substring of the
 * displayed text. `DataTable` filters with it, and a page that must reason about
 * the same filter (e.g. evaluate it on other data) reuses it, so the two can never
 * disagree. `raw` is the trimmed filter text; `null` = empty (no filter).
 */
export function columnFilterPredicate<R>(col: ColumnDef<R>, raw: string): ((row: R) => boolean) | null {
  const text = raw.trim();
  if (!text) return null;
  if (col.filterMatch) {
    const match = col.filterMatch;
    return (row) => match(row, text);
  }
  if (col.filterOptions) {
    const value = col.filterOptionValue ?? col.filterValue ?? col.searchValue;
    return (row) => value(row) === text;
  }
  const numeric = col.filterNumber ? parseNumericPredicate(text) : null;
  if (numeric) {
    const num = col.filterNumber!;
    return (row) => {
      const n = num(row);
      return n != null && numeric(n);
    };
  }
  const needle = text.toLowerCase();
  const value = col.filterValue ?? col.searchValue;
  return (row) => value(row).toLowerCase().includes(needle);
}
