import type { SweepResultRecord } from 'components/sweep/types';

/** Per-param-column tinting plan for the grouped-sweep combo table. */
export interface ParamColumnColor {
  /** True when every combo in the group shares one value for this knob — the
   *  column carries no signal, so it renders dimmed with no per-cell tint. */
  constant: boolean;
  /** Distinct value → background class (varying columns only; empty when
   *  `constant`). `null`/unset values are not tinted (they render '—'). */
  byValue: Map<number, string>;
}

// Per-column LOCAL palette: each varying column assigns these from value index 0,
// so the same value in two unrelated knobs need not share a color. Low-opacity
// backgrounds (full-cell, applied on the <td>) so equal values read as a color
// band without drowning the text. Spelled out statically for Tailwind's scanner.
const PALETTE = [
  'bg-blue-400/12',
  'bg-amber-400/12',
  'bg-pink-400/12',
  'bg-emerald-400/12',
  'bg-purple-400/12',
  'bg-orange-400/12',
  'bg-cyan-400/12',
  'bg-rose-400/12',
];

/** Shared color-plan builder: distinct values get a palette slot, single value → constant. */
function buildColorPlan(seen: Set<number | null>): ParamColumnColor {
  if (seen.size <= 1) return { constant: true, byValue: new Map() };
  const values = [...seen].sort((a, b) => (a == null ? 1 : b == null ? -1 : a - b));
  const byValue = new Map<number, string>();
  let i = 0;
  for (const v of values) {
    if (v == null) continue;
    byValue.set(v, PALETTE[i % PALETTE.length]);
    i++;
  }
  return { constant: false, byValue };
}

/**
 * Build the per-column tint plan for one group's combo rows: a column whose value
 * is identical across every combo is marked `constant` (dimmed in the table so the
 * fixed knobs recede); a column that varies gets each distinct value a stable
 * palette color (assigned in ascending value order, so colors are sort-stable and
 * identical across renders). Mirrors `ruleColorGroups` but on the *column* axis —
 * here every row is already in the same group, so the signal is per-value, not
 * per-row-cluster.
 */
export function computeParamColumnColors(
  results: SweepResultRecord[],
  paramKeys: string[],
): Map<string, ParamColumnColor> {
  const out = new Map<string, ParamColumnColor>();
  for (const key of paramKeys) {
    const seen = new Set<number | null>();
    for (const r of results) seen.add(r.params[key] ?? null);
    out.set(key, buildColorPlan(seen));
  }
  return out;
}

// PnL metric fields to color — column keys in buildSweepColumns that live in
// the `pnl` group and are likely to repeat across combos (clustered results).
const PNL_COLOR_KEYS = [
  'score', 'win_rate', 'total_pnl_sol', 'expectancy_sol',
  'profit_factor', 'median_pnl_pct', 'mean_pnl_pct',
  'p90_pnl_pct', 'best_pnl_pct', 'worst_pnl_pct', 'std_pnl_pct',
] as const satisfies ReadonlyArray<keyof SweepResultRecord>;

/**
 * Build per-column tint plans for the PnL metric columns. Same palette/logic as
 * param columns — rows sharing an identical metric value get the same cell band so
 * clusters of equivalent combos are visible at a glance.
 */
export function computePnlColumnColors(
  results: SweepResultRecord[],
): Map<string, ParamColumnColor> {
  const out = new Map<string, ParamColumnColor>();
  for (const key of PNL_COLOR_KEYS) {
    const seen = new Set<number | null>();
    for (const r of results) seen.add(r[key] as number | null);
    out.set(key, buildColorPlan(seen));
  }
  return out;
}
