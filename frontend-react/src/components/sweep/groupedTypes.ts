// Records + request shapes for the GROUPED param-sweep endpoints
// (`/api/strategies/sweeps[...]`). Generic across strategies (the `strategy_id`
// resolves the per-strategy tables on the backend); the drill-in combo rows
// reuse the flat `SweepResultRecord` shape, so the existing `buildSweepColumns`
// renders them unchanged.

import type { SweepResultRecord } from './types';

// --- grouping fields --------------------------------------------------------

/** The selectable fingerprint fields, matching the backend `GroupField` serde
 *  tags (snake_case). Selection order is the compound-key order. */
// Note: `creator_wallet` (near-unique per token → singleton groups) and
// `token_program_id` (effectively constant on pump.fun → one group) are poor
// grouping keys, so they're deliberately not offered here. The backend
// `GroupField` enum still accepts them for any legacy run that stored them.
export const GROUP_FIELDS = [
  'cu_limit',
  'cu_price',
  'max_sol_cost',
  'spendable_sol_in',
  'initial_buy_sol',
  'ix_labels',
  'is_cashback_enabled',
] as const;

export type GroupField = (typeof GROUP_FIELDS)[number];

/** Human labels for the group-by picker + group-key chips. */
export const GROUP_FIELD_LABELS: Record<GroupField, string> = {
  cu_limit: 'CU limit',
  cu_price: 'CU price',
  max_sol_cost: 'Max SOL cost',
  spendable_sol_in: 'Spendable SOL in',
  initial_buy_sol: 'Initial buy SOL',
  ix_labels: 'Instruction labels',
  is_cashback_enabled: 'Cashback on',
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
  liquidity_drop_pct?: (number | null)[];
  cohort_ratio?: (number | null)[];
  entry_min_age_secs?: (number | null)[];
  entry_min_alive_sol?: (number | null)[];
  entry_min_organic_sol?: (number | null)[];
  entry_pullback_pct?: (number | null)[];
  entry_higher_low_secs?: (number | null)[];
  entry_max_cohort_held?: (number | null)[];
  entry_min_liquidity_sol?: (number | null)[];
  entry_min_organic_liq?: (number | null)[];
}

/** The page-editable param grid for TPSL1 — the exit ladder only (no scalp
 *  entry gates, no cohort-dump exit). Mirrors the backend tpsl1 `AxesSpec`. */
export interface Tpsl1AxesSpec {
  take_profit?: number[];
  stop_loss?: number[];
  trailing_stop_pct?: (number | null)[];
  time_stop_secs?: (number | null)[];
  stall_secs?: (number | null)[];
  liquidity_drop_pct?: (number | null)[];
}

/** One editable axis: its key, label, the param role it belongs to (drives the
 *  entry/exit grouping in the sweep param grid), whether `null` (disabled) is a
 *  valid option, and the default candidate list (mirrors the strategy's
 *  `Axes::default` on the backend, so the projected combo count is accurate and
 *  the grid is prefilled). `key` is a plain string so one `AxisDef[]` shape
 *  serves any strategy's axes ([`TPSL2_AXES`], [`TPSL1_AXES`]). */
export interface AxisDef {
  key: string;
  label: string;
  group: 'entry' | 'exit';
  nullable: boolean;
  default: (number | null)[];
}

// Order + grouping mirror the TPSL2 rule modal field-by-field (Entry Gates ·
// Scalp, then Exit Gates), so the sweep param grid reads the same as the modal.
// The high-leverage knobs ship a real candidate grid; every other knob defaults
// to `[null]` ("off"/unbounded) so it doesn't expand the grid until you type
// values for it — matching the backend `Tpsl2Axes::default`. A blank box → that
// knob stays unbounded (disabled).
export const TPSL2_AXES: AxisDef[] = [
  // Entry gates · scalp — when to buy (matches the modal's Entry section order).
  { key: 'entry_min_age_secs', label: 'Entry min age (s)', group: 'entry', nullable: true, default: [10, 30] },
  { key: 'entry_min_alive_sol', label: 'Entry min alive (SOL)', group: 'entry', nullable: true, default: [null] },
  { key: 'entry_min_organic_sol', label: 'Entry min organic (SOL)', group: 'entry', nullable: true, default: [null] },
  { key: 'entry_min_organic_liq', label: 'Entry min organic liq (SOL)', group: 'entry', nullable: true, default: [null] },
  { key: 'entry_max_cohort_held', label: 'Entry max cohort held %', group: 'entry', nullable: true, default: [null] },
  { key: 'entry_min_liquidity_sol', label: 'Entry min liq (SOL)', group: 'entry', nullable: true, default: [null, 5] },
  { key: 'entry_pullback_pct', label: 'Entry pullback %', group: 'entry', nullable: true, default: [null, 10] },
  { key: 'entry_higher_low_secs', label: 'Entry higher-low (s)', group: 'entry', nullable: true, default: [null] },
  // Exit gates — when to sell (matches the modal's Exit section order).
  { key: 'take_profit', label: 'Take profit %', group: 'exit', nullable: false, default: [50, 100, 200] },
  { key: 'stop_loss', label: 'Stop loss %', group: 'exit', nullable: false, default: [30, 50] },
  { key: 'trailing_stop_pct', label: 'Trailing stop %', group: 'exit', nullable: true, default: [null, 20, 35] },
  { key: 'time_stop_secs', label: 'Time stop (s)', group: 'exit', nullable: true, default: [null, 120, 300] },
  { key: 'stall_secs', label: 'Stall (s)', group: 'exit', nullable: true, default: [null, 30, 60] },
  { key: 'liquidity_drop_pct', label: 'Liq-drop exit %', group: 'exit', nullable: true, default: [null] },
  { key: 'cohort_ratio', label: 'Cohort-dump exit %', group: 'exit', nullable: true, default: [null] },
];

// --- TPSL1 editable axes ----------------------------------------------------

// TPSL1 is the token-creation-filter strategy: its only per-trade swept knobs
// are the exit ladder (TP/SL lead, then the optional trailing/time/stall/liquidity
// exits). It has NO scalp entry gates and NO cohort-dump exit, so this list is the
// TPSL2 exit block minus cohort. Mirrors the backend `tpsl1::Tpsl1Axes::default`.
export const TPSL1_AXES: AxisDef[] = [
  { key: 'take_profit', label: 'Take profit %', group: 'exit', nullable: false, default: [50, 100, 200] },
  { key: 'stop_loss', label: 'Stop loss %', group: 'exit', nullable: false, default: [30, 50] },
  { key: 'trailing_stop_pct', label: 'Trailing stop %', group: 'exit', nullable: true, default: [null, 20, 35] },
  { key: 'time_stop_secs', label: 'Time stop (s)', group: 'exit', nullable: true, default: [null, 120, 300] },
  { key: 'stall_secs', label: 'Stall (s)', group: 'exit', nullable: true, default: [null, 30, 60] },
  { key: 'liquidity_drop_pct', label: 'Liq-drop exit %', group: 'exit', nullable: true, default: [null] },
];

// --- run / group / result records -------------------------------------------

export interface GroupedSweepRunRecord {
  id: string;
  strategy_id: string;
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
  /** Lifecycle: `running` (in flight), `completed` (full sweep), or `cancelled`
   *  (cancelled / crash-recovered → only `groups_done` of `group_count` groups
   *  present). A `cancelled` run is honestly partial — render it with a banner so
   *  it's never mistaken for a complete sweep. */
  status: 'running' | 'completed' | 'cancelled';
  /** Groups persisted so far; equals `group_count` for a `completed` run. */
  groups_done: number;
}

/** One group's summary row: its fingerprint key, sample size, and winning combo. */
export interface GroupedSweepGroupRecord {
  id: string;
  group_index: number;
  /** `{ "creator_wallet": "4f3a…", "max_sol_cost": "12345" }`; `{}` = the ALL group. */
  group_key: Record<string, string>;
  token_count: number;
  /** The best combo's `n_fired` — sample size behind the headline pick. */
  fired_count: number;
  best_combo_id: number;
  /** Robust realized score (`μ−Z·σ/√n` over closed trades) of the winning combo
   *  — the headline ranking metric; `null` when it has < 2 closed trades. */
  best_score: number | null;
  best_expectancy_sol: number;
  best_params: Record<string, number | null>;
}

/** Drill-in combo rows reuse the flat sweep-result shape (same metric set). */
export type GroupedSweepResultRecord = SweepResultRecord;

// --- start request ----------------------------------------------------------

export interface GroupedSweepStartArgs {
  strategy_id: string;
  /** RFC3339 UTC; selection lower bound (inclusive). */
  created_after?: string;
  /** RFC3339 UTC; selection upper bound (exclusive). */
  created_before?: string;
  curve_only?: boolean;
  group_by: GroupField[];
  min_tokens?: number;
  /** `grid` | `random:N` | `lhs:N`. */
  method?: string;
  /** Strategy-specific axes — TPSL2's full grid or TPSL1's exit-ladder-only grid.
   *  Resolved by `strategy_id` on the backend. */
  axes?: Tpsl2AxesSpec | Tpsl1AxesSpec;
  token_cap?: number;
  /** Per-group combo cap override. Omitted ⇒ backend default (5000); the backend
   *  clamps to its hard backstop. */
  max_combos?: number;
}
