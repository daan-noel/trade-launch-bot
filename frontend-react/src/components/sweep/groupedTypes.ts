// Records + request shapes for the GROUPED param-sweep endpoints
// (`/api/strategies/sweeps[...]`). Generic across strategies (the `strategy_id`
// resolves the per-strategy tables on the backend); the drill-in combo rows
// reuse the flat `SweepResultRecord` shape, so the existing `buildSweepColumns`
// renders them unchanged.

import type { SweepResultRecord } from './types';

// --- grouping fields --------------------------------------------------------

/** The selectable fingerprint fields, matching the backend `GroupField` serde
 *  tags (snake_case). Selection order is the compound-key order. */
export const GROUP_FIELDS = [
  'creator_wallet',
  'token_program_id',
  'cu_limit',
  'cu_price',
  'is_cashback_enabled',
  'max_sol_cost',
  'spendable_sol_in',
  'initial_buy_sol',
  'ix_labels',
] as const;

export type GroupField = (typeof GROUP_FIELDS)[number];

/** Human labels for the group-by picker + group-key chips. */
export const GROUP_FIELD_LABELS: Record<GroupField, string> = {
  creator_wallet: 'Creator wallet',
  token_program_id: 'Token program',
  cu_limit: 'CU limit',
  cu_price: 'CU price',
  is_cashback_enabled: 'Cashback on',
  max_sol_cost: 'Max SOL cost',
  spendable_sol_in: 'Spendable SOL in',
  initial_buy_sol: 'Initial buy SOL',
  ix_labels: 'Instruction labels',
};

// --- TPSL2 editable axes ----------------------------------------------------

/** The page-editable param grid for TPSL2 — one optional candidate list per
 *  swept knob. `null` inside a nullable axis is that knob's "disabled" option.
 *  Mirrors the backend `AxesSpec`. */
export interface Tpsl2AxesSpec {
  take_profit?: number[];
  stop_loss?: number[];
  trailing_stop_pct?: (number | null)[];
  time_stop_secs?: (number | null)[];
  stall_secs?: (number | null)[];
  entry_min_age_secs?: (number | null)[];
  entry_pullback_pct?: (number | null)[];
  entry_min_liquidity_sol?: (number | null)[];
}

/** One editable axis: its key, label, whether `null` (disabled) is a valid
 *  option, and the default candidate list (mirrors `Tpsl2Axes::default` on the
 *  backend, so the projected combo count is accurate and the grid is prefilled). */
export interface AxisDef {
  key: keyof Tpsl2AxesSpec;
  label: string;
  nullable: boolean;
  default: (number | null)[];
}

export const TPSL2_AXES: AxisDef[] = [
  { key: 'take_profit', label: 'Take profit %', nullable: false, default: [50, 100, 200] },
  { key: 'stop_loss', label: 'Stop loss %', nullable: false, default: [30, 50] },
  { key: 'trailing_stop_pct', label: 'Trailing stop %', nullable: true, default: [null, 20, 35] },
  { key: 'time_stop_secs', label: 'Time stop (s)', nullable: true, default: [null, 120, 300] },
  { key: 'stall_secs', label: 'Stall (s)', nullable: true, default: [null, 30, 60] },
  { key: 'entry_min_age_secs', label: 'Entry min age (s)', nullable: true, default: [10, 30] },
  { key: 'entry_pullback_pct', label: 'Entry pullback %', nullable: true, default: [null, 10] },
  { key: 'entry_min_liquidity_sol', label: 'Entry min liq (SOL)', nullable: true, default: [null, 5] },
];

// --- run / group / result records -------------------------------------------

export interface GroupedSweepRunRecord {
  id: string;
  strategy_id: string;
  rule_id: string | null;
  source: string;
  method: string;
  created_after: string | null;
  created_before: string | null;
  curve_only: boolean;
  /** The grouping fields, in selection order. */
  grouping_spec: GroupField[];
  /** The resolved param axes the run used. */
  axes_spec: Record<string, unknown>;
  min_tokens: number;
  token_count: number;
  group_count: number;
  combo_count: number;
  corpus_hash: string | null;
  created_at: string;
}

/** One group's summary row: its fingerprint key, sample size, and winning combo. */
export interface GroupedSweepGroupRecord {
  id: string;
  group_index: number;
  /** `{ "creator_wallet": "4f3a…", "max_sol_cost": "12345" }`; `{}` = the ALL group. */
  group_key: Record<string, string>;
  token_count: number;
  /** The best combo's `n_fired` — sample size behind `best_expectancy_sol`. */
  fired_count: number;
  best_combo_id: number;
  best_expectancy_sol: number;
  best_params: Record<string, number | null>;
}

/** Drill-in combo rows reuse the flat sweep-result shape (same metric set). */
export type GroupedSweepResultRecord = SweepResultRecord;

// --- start request ----------------------------------------------------------

export interface GroupedSweepStartArgs {
  strategy_id: string;
  rule_id?: string;
  /** RFC3339 UTC; selection lower bound (inclusive). */
  created_after?: string;
  /** RFC3339 UTC; selection upper bound (exclusive). */
  created_before?: string;
  curve_only?: boolean;
  group_by: GroupField[];
  min_tokens?: number;
  /** `grid` | `random:N` | `lhs:N`. */
  method?: string;
  /** Strategy-specific axes (TPSL2 today). */
  axes?: Tpsl2AxesSpec;
  token_cap?: number;
}
