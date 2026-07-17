// Same-value cell tints for table columns — values shared by ≥2 rows get a
// stable background from a fixed palette (order of first appearance). Unset /
// empty values stay untinted. Class names are spelled out statically so
// Tailwind's scanner includes them.

/** Soft fills — distinct enough to group, quiet enough for dense tables. */
const PALETTE = [
  'bg-blue-400/12',
  'bg-amber-400/12',
  'bg-pink-400/12',
  'bg-emerald-400/12',
  'bg-purple-400/12',
  'bg-orange-400/12',
  'bg-cyan-400/12',
  'bg-rose-400/12',
] as const;

export interface SameValueColumn<R> {
  key: string;
  /** Canonical string for grouping; return `null` for unset (never tinted). */
  valueOf: (row: R) => string | null;
}

/**
 * Build a lookup `${rowKey}\\0${colKey}` → bg class for every cell whose value
 * appears on ≥2 rows in that column.
 */
export function computeSameValueCellClasses<R>(
  rows: R[],
  rowKey: (row: R) => string,
  columns: SameValueColumn<R>[],
): Map<string, string> {
  const out = new Map<string, string>();

  for (const col of columns) {
    const counts = new Map<string, number>();
    for (const row of rows) {
      const v = col.valueOf(row);
      if (v == null) continue;
      counts.set(v, (counts.get(v) ?? 0) + 1);
    }

    const colorOf = new Map<string, string>();
    let next = 0;
    for (const row of rows) {
      const v = col.valueOf(row);
      if (v == null || (counts.get(v) ?? 0) < 2) continue;
      if (!colorOf.has(v)) {
        colorOf.set(v, PALETTE[next % PALETTE.length]);
        next++;
      }
      out.set(`${rowKey(row)}\0${col.key}`, colorOf.get(v)!);
    }
  }

  return out;
}
