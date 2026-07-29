// Records + request shapes for the GROUPED param-sweep endpoints
// (`/api/strategies/sweeps[...]`). One generic engine (`strategy_id="generic"`)
// drives these now; the swept `params` per combo is a `RuleParams` blob (rendered
// by `genericSweepColumns`), and the axes are the registry-driven `AxisSpec[]`
// (`genericAxes.ts`) — the old per-strategy static axis grids + fingerprint-blob
// serializers were removed with the legacy sweep pages (redesign FE5.4).

import type { CostModelId, FillModelId } from 'lib/strategy/types';

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
  /** Always `"db"` today (vestigial single-source tag) — carries no signal, not
   *  worth surfacing in the UI. Kept typed only so the wire shape is complete. */
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
  /** Hash of the corpus slice this run scanned — two runs with the same hash swept
   *  the exact same tape (same selection + filters + lake export). `null` on a
   *  trade-less / legacy corpus. Was already sent by the backend but unread here. */
  corpus_hash: string | null;
  /** Block time of the newest trade in the corpus this run scanned — how fresh its
   *  data was. `null` on legacy runs / a trade-less corpus. The sweep reads the
   *  sealed lake only while Simulate splices the fresh PG tail, so a stale export
   *  makes the two disagree without either being wrong: the sweep freezes `Open (est)`
   *  rows at old prices that a simulate watches die. Surfaced as "data through". */
  corpus_last_trade_at: string | null;
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
  /** Saved fingerprint the corpus was scoped to (engine match — exact + bucket
   *  axes), or `null` for a manual group-by / filter run. Mutually exclusive with
   *  `ix_labels_filter` / `field_filters`, which are `null` on a scoped run. */
  fingerprint_id: string | null;
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
  /** Which trade in the fill window priced each leg. `null` on legacy runs ⇒
   *  `worst_case`, what the sweep hardcoded before the model became selectable.
   *  Part of the run's IDENTITY — two runs under different fill models are not
   *  comparable, so this is shown next to the run's PnL. */
  fill_model: FillModelId | null;
  /** Which cost model priced the round-trips. `null` on legacy runs ⇒
   *  `pumpfun_default`, which charges slippage on top of the fill price. */
  cost_model: CostModelId | null;
  /** The candidate scale-out ladder(s) searched in Pass 2 — `ExitStage[][]`
   *  (backend keeps the grid-shaped wire contract for forward compat), but the
   *  FE authors exactly ONE user ladder via `ScaleOutBuilder` and sends it as
   *  the sole entry: comparing many arbitrary ladders against the same small
   *  per-combo sample is a multiple-comparisons trap, not a real search. `null`
   *  = no Pass 2. Each group's top-K combos are independently re-scored against
   *  the ladder(s) here plus their own baseline exit and keep whichever wins —
   *  a combo it doesn't help keeps its own exit, so this field is the search
   *  space, not what any one combo ended up with (see that combo's own
   *  `params.scale_out`). */
  scale_out: unknown[][] | null;
  /** How many best combos/group Pass 2 re-scored. `null` when no overlay. */
  scale_out_top_k: number | null;
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
  /** Checklist score of the winning combo; `null` when never fired. */
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
  /** Scope the corpus to a saved fingerprint using the ENGINE matcher (exact +
   *  bucket axes — the same gate live arms on). When set the backend ignores
   *  `ix_labels_filter` / `field_filters`; `group_by` still partitions within the
   *  matched slice. Omitted ⇒ manual group-by / filters. */
  fingerprint_id?: string;
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
  /** Corpus-wide volume-ix patterns when axes reference `m_flow_split` /
   *  `m_flow_split_window` (not aggregate `m_flow_lifetime` / `m_flow_window`).
   *  Required by the backend for those runs; omitted otherwise. */
  volume_ix_patterns?: string[][];
  /** Which trade in the fill window prices each simulated leg. Omitted ⇒
   *  `worst_case` (what the sweep hardcoded before this was selectable), so stored
   *  and replayed runs keep their meaning. Unlike `use_avx512` this changes the
   *  RESULT, not just how it was computed — it is persisted on the run row. */
  fill_model?: FillModelId;
  /** Which execution-cost model prices the round-trips. Omitted ⇒
   *  `pumpfun_default`. Pair an explicit `fill_model` with `pumpfun_fee_only`:
   *  the fill price already prices slippage. */
  cost_model?: CostModelId;
  /** Pass-2 ladder for the run — `ExitStage[][]` on the wire (backend grid
   *  contract), but the FE sends exactly one user-authored ladder as its sole
   *  entry. Omitted / empty ⇒ no Pass 2. Each top-K combo per group is
   *  re-scored against it plus its own baseline and keeps whichever wins
   *  (per combo — never forced onto a combo it doesn't help). */
  scale_out?: unknown[][];
  /** Top-K combos per group for Pass 2. Default 3. */
  scale_out_top_k?: number;
}
