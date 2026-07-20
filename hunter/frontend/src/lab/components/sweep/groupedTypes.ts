// Records + request shapes for the GROUPED param-sweep endpoints
// (`/api/strategies/sweeps[...]`). One generic engine (`strategy_id="generic"`)
// drives these now; the swept `params` per combo is a `RuleParams` blob (rendered
// by `genericSweepColumns`), and the axes are the registry-driven `AxisSpec[]`
// (`genericAxes.ts`) — the old per-strategy static axis grids + fingerprint-blob
// serializers were removed with the legacy sweep pages (redesign FE5.4).

import type { SweepResultRecord } from './types';

// --- grouping fields --------------------------------------------------------

/** The selectable fingerprint fields, matching the backend `GroupField` serde
 *  tags (snake_case). Selection order is the compound-key order. */
export const GROUP_FIELDS = [
  'cu_limit',
  'cu_price',
  'max_cost_lamports',
  'spendable_lamports_in',
  'initial_buy_sol',
  'first_slot_buy_sol',
  'first_slot_sell_sol',
  'is_cashback_enabled',
  'ix_labels',
] as const;

export type GroupField = (typeof GROUP_FIELDS)[number];

/** Human labels for the group-by picker + group-key chips. */
export const GROUP_FIELD_LABELS: Record<GroupField, string> = {
  cu_limit: 'CU limit',
  cu_price: 'CU price',
  max_cost_lamports: 'Max SOL cost',
  spendable_lamports_in: 'Spendable SOL in',
  initial_buy_sol: 'Initial buy SOL',
  first_slot_buy_sol: 'First-slot buy SOL',
  first_slot_sell_sol: 'First-slot sell SOL',
  ix_labels: 'Instruction labels',
  is_cashback_enabled: 'Cashback on',
};

// --- bucketed (binned) fields ----------------------------------------------

/** Bucket width (SOL) the backend groups the continuous SOL-amount fields by.
 *  Mirrors `trading_core` `grouping::SOL_BUCKET_WIDTH` — keep the two in sync. */
export const SOL_BUCKET_WIDTH = 0.1;

/** Fields the backend groups into `SOL_BUCKET_WIDTH`-wide **ranges** (group chips
 *  read as `"lo–hi"`, e.g. `"1.0–1.1"`) instead of exact values — they are continuous
 *  SOL amounts. Every other field groups on its exact value. Mirrors the binned arms
 *  of `grouping::render_field` / `creation_stats_repo::group_field_sql`. */
export const BUCKETED_GROUP_FIELDS: ReadonlySet<GroupField> = new Set<GroupField>([
  'initial_buy_sol',
  'max_cost_lamports',
  'spendable_lamports_in',
  'first_slot_buy_sol',
  'first_slot_sell_sol',
]);

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
  /** The resolved param axes the run used (`{ axes: AxisSpec[] }` for generic). */
  axes_spec: Record<string, unknown>;
  min_tokens: number;
  token_count: number;
  group_count: number;
  combo_count: number;
  corpus_hash: string | null;
  created_at: string;
  /** Lifecycle: `running` | `completed` | `cancelled` (user abort, honestly
   *  partial) | `partial` (the run stopped early — a DB write error, or an
   *  engine failure after some groups had already folded — so its committed
   *  groups are real but the sweep is incomplete) | `failed` (it stopped before
   *  any group folded, e.g. bad axes or a corpus that cannot fit this machine).
   *  A failed run is kept rather than deleted so the attempt stays inspectable. */
  status: 'running' | 'completed' | 'cancelled' | 'partial' | 'failed';
  /** Groups persisted so far; equals `group_count` for a `completed` run. */
  groups_done: number;
  /** The exact-set instruction-label corpus filter the run used, or `null`. */
  ix_labels_filter: string[] | null;
  /** Per-field value filters the corpus was pinned to, or `null` when none. */
  field_filters: Record<string, (number | boolean)[]> | null;
  /** The per-run token cap submitted; `null` for legacy runs. */
  token_cap: number | null;
  /** The per-group combo-cap override submitted; `null` ⇒ backend default. */
  max_combos: number | null;
  /** Optional user-given name; `null` = unnamed (UI falls back to timestamp). */
  label: string | null;
  /** Notional (SOL) each simulated round-trip was priced at; `null` on legacy runs. */
  buy_amount_sol: number | null;
  /** Bucket width (SOL) the continuous SOL group fields were binned at — the width
   *  a promoted rule's matcher must use so it matches the same bucket. `null` legacy. */
  bucket_width_sol: number | null;
  /** Corpus-wide volume-ix patterns used when the run swept flow axes. `null` =
   *  non-flow run / legacy. Promote copies these into the fingerprint. */
  volume_ix_patterns: string[][] | null;
}

/** One group's summary row: its fingerprint key, sample size, and winning combo. */
export interface GroupedSweepGroupRecord {
  id: string;
  group_index: number;
  /** `{ "cu_price": "1000", "max_cost_lamports": "1.0–1.1" }`; `{}` = the ALL group. */
  group_key: Record<string, string>;
  token_count: number;
  /** The best combo's `n_fired` — sample size behind the headline pick. */
  fired_count: number;
  best_combo_id: number;
  /** Robust realized score of the winning combo; `null` when < 2 closed trades. */
  best_score: number | null;
  best_expectancy_sol: number;
  best_win_rate: number;
  best_total_pnl_sol: number;
  /** Winning combo's unrealized PnL over its still-open positions — excluded
   *  from `best_total_pnl_sol` by design, surfaced so a profitable-looking
   *  realized total can't hide a pile of open losers. */
  best_open_pnl_sol: number;
  /** Winning combo's still-open / closed split of `fired_count`. Shows how much
   *  of the headline sample is unrealized. */
  best_n_open: number;
  best_n_closed: number;
  /** Winning combo's profit factor; `null` = no losing trades (UI shows ∞). */
  best_profit_factor: number | null;
  best_mean_pnl_pct: number;
  best_median_pnl_pct: number;
  best_p90_pnl_pct: number;
  best_std_pnl_pct: number;
  best_avg_holding_secs: number;
  best_median_holding_secs: number;
  /** The winning combo's params — a `RuleParams` blob for the generic engine. */
  best_params: Record<string, unknown>;
}

/** Drill-in combo rows reuse the flat sweep-result shape (same metric set). */
export type GroupedSweepResultRecord = SweepResultRecord;

/** Per-token outcome for a single re-simulated combo (the token-results drill-in). */
export interface ComboTokenResult {
  mint_address: string;
  symbol: string;
  fired: boolean;
  pnl_sol: number;
  pnl_pct: number;
  holding_secs: number;
  /** `"TakeProfit"` | `"StopLoss"` | `"Metrics"` | `"Dead"` | `"Open"` | `"NoEntry"`. */
  exit: string;
  entry_time: string | null;
  entry_price: number | null;
  /** Real base58 tx signature of the entry fill. Null when not fired / unresolved. */
  entry_tx: string | null;
  exit_time: string | null;
  exit_price: number | null;
  /** Real base58 tx signature of the exit fill. Null when open / not fired. */
  exit_tx: string | null;
  created_at: string | null;
  ath_price: number | null;
  creator_wallet: string;
  ath_timestamp: string | null;
  current_price: number | null;
  market_cap: number | null;
  volume_sol_total: number;
  trade_count: number;
  is_migrated: boolean;
  is_dead: boolean;
}

/** `GET …/token-results` response: the drill-in's per-token rows plus an **exact**
 *  (no-sketch) metrics summary over exactly those rows. */
export interface ComboTokenResultsResponse {
  rows: ComboTokenResult[];
  metrics: SweepResultRecord;
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
  /** Exact-set instruction-label corpus filter (mutually exclusive with grouping
   *  by `ix_labels`). Omitted ⇒ no filter. */
  ix_labels_filter?: string[];
  /** Per-field value filters (key = GroupField tag; value = allowed numbers/bools). */
  field_filters?: Record<string, (number | boolean)[]>;
  min_tokens?: number;
  /** `grid` | `random:N` | `lhs:N` | `refine:N[:K]`. */
  method?: string;
  /** Strategy-specific axes — for `"generic"` this is `AxesRequest { axes: [...] }`.
   *  Resolved by `strategy_id` on the backend. */
  axes?: unknown;
  token_cap?: number;
  /** Per-group combo cap override. Omitted ⇒ backend default. */
  max_combos?: number;
  /** Notional (SOL) to price every simulated round-trip at. Omitted ⇒ 1.0 SOL. */
  buy_amount_sol?: number;
  /** Bucket width (SOL) for the continuous SOL group fields. Omitted ⇒ 0.1. */
  bucket_width_sol?: number;
  /** Host RAM (MB) the run leaves free for OS + desktop; every sizing ceiling is
   *  `host free − this`. A preference, not a limit — a run that does not fit
   *  degrades (fewer threads / smaller batches) rather than being refused.
   *  Omitted ⇒ backend default (1024). */
  ram_reserve_mb?: number;
  /** Opt into the AVX-512 vectorized per-`(combo × token)` exit scan (lab-only
   *  speedup). Honored only when the host has AVX-512 — otherwise the backend forces
   *  the scalar scan and toasts a notice. Like `ram_reserve_mb`, it's a property of
   *  *how the box computed*, not the analysis, so it isn't persisted on the run row.
   *  Omitted ⇒ scalar. */
  use_avx512?: boolean;
  /** Corpus-wide volume-ix patterns when axes reference `m_flow_*`. Required by
   *  the backend for those runs; omitted otherwise. */
  volume_ix_patterns?: string[][];
}
