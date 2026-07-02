// Records + request shapes for the GROUPED param-sweep endpoints
// (`/api/strategies/sweeps[...]`). Generic across strategies (the `strategy_id`
// resolves the per-strategy tables on the backend); the drill-in combo rows
// reuse the flat `SweepResultRecord` shape, so the existing `buildSweepColumns`
// renders them unchanged.

import type { SweepResultRecord } from './types';
// swing1 axis metadata + the generic `AxisDef`/`AxisSubgroup` shapes now live in
// `@shared` so the live swing1 rule page can import them (a `@live` page can't
// import `@lab`). Re-exported below so existing `@lab` imports keep working.
import type { AxisDef, Swing1AxesSpec } from '@shared/lib/swing1Axes';
export type { AxisDef, AxisSubgroup, Swing1AxesSpec } from '@shared/lib/swing1Axes';
export { SWING1_AXES, SWING1_SUBGROUPS, groupAxesBySubgroup } from '@shared/lib/swing1Axes';

// --- grouping fields --------------------------------------------------------

/** The selectable fingerprint fields, matching the backend `GroupField` serde
 *  tags (snake_case). Selection order is the compound-key order. */
// Note: `token_program_id` (effectively constant on pump.fun → one group) is a
// poor grouping key, so it's deliberately not offered here. `creator_wallet` was
// removed from the backend `GroupField` enum entirely — creators rotate wallets,
// so a creator key is un-trackable and only ever yields singleton groups.
export const GROUP_FIELDS = [
  'cu_limit',
  'cu_price',
  'max_sol_cost',
  'spendable_sol_in',
  'initial_buy_sol',
  'is_cashback_enabled',
  'ix_labels',
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
  entry_min_age_secs?: (number | null)[];
  entry_max_age_secs?: (number | null)[];
  entry_min_alive_sol?: (number | null)[];
  entry_min_organic_sol?: (number | null)[];
  entry_pullback_pct?: (number | null)[];
  entry_higher_low_secs?: (number | null)[];
  entry_min_liquidity_sol?: (number | null)[];
  entry_min_organic_liq?: (number | null)[];
}

/** The page-editable param grid for TPSL1 — the exit ladder only (no scalp
 *  entry gates). Mirrors the backend tpsl1 `AxesSpec`. */
export interface Tpsl1AxesSpec {
  take_profit?: number[];
  stop_loss?: number[];
  trailing_stop_pct?: (number | null)[];
  time_stop_secs?: (number | null)[];
  stall_secs?: (number | null)[];
  liquidity_drop_pct?: (number | null)[];
}

// `AxisDef` / `AxisSubgroup` are defined in `@shared/lib/swing1Axes` and
// re-exported at the top of this file — the TPSL axis lists below consume them.

// Order + grouping mirror the TPSL2 rule modal field-by-field (Entry Gates ·
// Scalp, then Exit Gates), so the sweep param grid reads the same as the modal.
// The high-leverage knobs ship a real candidate grid; every other knob defaults
// to `[null]` ("off"/unbounded) so it doesn't expand the grid until you type
// values for it — matching the backend `Tpsl2Axes::default`. A blank box → that
// knob stays unbounded (disabled).
export const TPSL2_AXES: AxisDef[] = [
  // Entry gates · scalp — when to buy (matches the modal's Entry section order).
  { key: 'entry_min_age_secs', label: 'Entry min age (s)', group: 'entry', nullable: true, default: [10, 30] },
  { key: 'entry_max_age_secs', label: 'Entry max age (s)', group: 'entry', nullable: true, default: [null] },
  { key: 'entry_min_alive_sol', label: 'Entry min alive (SOL)', group: 'entry', nullable: true, default: [null] },
  { key: 'entry_min_organic_sol', label: 'Entry min organic (SOL)', group: 'entry', nullable: true, default: [null] },
  { key: 'entry_min_organic_liq', label: 'Entry min organic liq (SOL)', group: 'entry', nullable: true, default: [null] },
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
];

// --- TPSL1 editable axes ----------------------------------------------------

// TPSL1 is the token-creation-filter strategy: its only per-trade swept knobs
// are the exit ladder (TP/SL lead, then the optional trailing/time/stall/liquidity
// exits). It has NO scalp entry gates, so this list is the TPSL2 exit block.
// Mirrors the backend `tpsl1::Tpsl1Axes::default`.
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
  /** The exact-set instruction-label corpus filter the run used (the JSON array
   *  the form submitted), or `null` when unfiltered / grouped by `ix_labels`. */
  ix_labels_filter: string[] | null;
  /** Per-field value filters the corpus was pinned to (`{"cu_price":[1000],…}`),
   *  or `null` when none. Values are numbers or booleans (cashback). */
  field_filters: Record<string, (number | boolean)[]> | null;
  /** The per-run token cap submitted (distinct from realized `token_count`);
   *  `null` for legacy runs. */
  token_cap: number | null;
  /** The per-group combo-cap override submitted (distinct from realized
   *  `combo_count`); `null` ⇒ backend default. */
  max_combos: number | null;
  /** Optional user-given name; `null` = unnamed (UI falls back to timestamp). */
  label: string | null;
  /** Notional (SOL) each simulated round-trip was priced at; `null` on legacy
   *  runs (backend defaulted to 1.0 SOL). */
  buy_amount_sol: number | null;
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

/** Per-token outcome for a single re-simulated combo (the token-results drill-in). */
export interface ComboTokenResult {
  mint: string;
  symbol: string;
  fired: boolean;
  pnl_sol: number;
  pnl_pct: number;
  holding_secs: number;
  /** `"TakeProfit"` | `"StopLoss"` | `"TrailingStop"` | `"Stall"` |
   *  `"TimeStop"` | `"LiquidityExit"` | `"Open"` | `"NoEntry"` */
  exit: string;
  // Simulation fill details
  entry_time: string | null;
  entry_price: number | null;
  /** Real base58 tx signature of the entry fill, resolved server-side from the
   *  trades table by (mint, slot, buy). Null when not fired or unresolved. */
  entry_tx: string | null;
  exit_time: string | null;
  exit_price: number | null;
  /** Real base58 tx signature of the exit fill (sell side). Null when open,
   *  not fired, or unresolved. */
  exit_tx: string | null;
  // Token metadata (from tokens + tokens_info)
  created_at: string | null;
  creator_wallet: string | null;
  ath_price: number | null;
  ath_timestamp: string | null;
  current_price: number | null;
  market_cap: number | null;
  volume_sol: number | null;
  trade_count: number | null;
  is_migrated: boolean | null;
  is_dead: boolean | null;
}

// --- start request ----------------------------------------------------------

export interface GroupedSweepStartArgs {
  strategy_id: string;
  /** RFC3339 UTC; selection lower bound (inclusive). */
  created_after?: string;
  /** RFC3339 UTC; selection upper bound (exclusive). */
  created_before?: string;
  curve_only?: boolean;
  group_by: GroupField[];
  /** Exact-set instruction-label filter — restrict the corpus to tokens whose
   *  `ix_labels` set equals these labels, then sweep. The page sends this only
   *  when grouping by `ix_labels` is OFF (the two are mutually exclusive: group
   *  by the label set, or pin a single set and sweep it). Omitted ⇒ no filter. */
  ix_labels_filter?: string[];
  /** Per-field value filters: restrict the corpus to tokens whose fingerprint
   *  value for the named field is in the allowed set. Map key = GroupField tag
   *  (e.g. `"cu_price"`); value = allowed numbers. Empty map or omitted ⇒ no
   *  filter. `"ix_labels"` is handled by `ix_labels_filter` above.
   *  Applied post-fingerprint, in-memory, alongside `ix_labels_filter`. */
  field_filters?: Record<string, (number | boolean)[]>;
  min_tokens?: number;
  /** `grid` | `random:N` | `lhs:N`. */
  method?: string;
  /** Strategy-specific axes — TPSL2's full grid, TPSL1's exit-ladder-only grid,
   *  or swing1's kill→volume swing-phase grid. Resolved by `strategy_id` on the
   *  backend. */
  axes?: Tpsl2AxesSpec | Tpsl1AxesSpec | Swing1AxesSpec;
  token_cap?: number;
  /** Per-group combo cap override. Omitted ⇒ backend default (5000); the backend
   *  clamps to its hard backstop. */
  max_combos?: number;
  /** Notional (SOL) to price every simulated round-trip at. Set to the live
   *  `buy_amount` so backtest PnL% matches live results. Omitted ⇒ 1.0 SOL. */
  buy_amount_sol?: number;
}
