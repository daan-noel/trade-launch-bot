import type { RunSummary } from 'lib/strategy/runSummary';
import type { CostModelId, FillModelId } from 'lib/strategy/types';

export type { RunMetrics, RunSummary } from 'lib/strategy/runSummary';

export type PriceUnit = 'SOL' | 'USD';

export interface PriceUnitState {
  unit: PriceUnit;
  usdRate: number | null;
}

/** The shared token-enrichment fields the backend `TokenEnrichment` struct
 *  (`trading_core::storage::token_enrichment`) flattens onto every token-result row.
 *  All optional here because most result tables receive them only after a batch
 *  enrich resolves (the fully-populated `TokenRecord`/`TokenDetailRecord` redeclare
 *  them required). Single source: interfaces `extends` this instead of re-listing the
 *  block, so a backend field add/rename is one edit, not five. */
export interface TokenEnrichmentFields {
  symbol?: string;
  name?: string;
  creator_wallet?: string;
  initial_buy_sol?: number | null;
  initial_supply_token?: number | null;
  token_amount?: number | null;
  max_cost_lamports?: number | null;
  spendable_lamports_in?: number | null;
  min_tokens_out?: number | null;
  cu_limit?: number | null;
  cu_price?: number | null;
  is_mayhem_mode?: boolean;
  is_cashback_enabled?: boolean;
  creation_tx_signature?: string;
  ix_labels_count?: number;
  instruction_labels?: unknown;
  trade_count?: number;
  current_price?: number | null;
  volume_sol_total?: number;
  first_slot_buy_sol?: number | null;
  first_slot_sell_sol?: number | null;
  market_cap?: number | null;
  ath_price?: number | null;
  ath_timestamp?: string | null;
  is_migrated?: boolean;
  is_dead?: boolean;
  last_trade_at?: string | null;
  last_synced_at?: string | null;
}

export interface TokenRecord {
  mint_address: string;
  name: string;
  symbol: string;
  creator_wallet: string;
  trade_count: number;
  current_price: number | null;
  volume_sol_total: number;
  /** Buy/sell SOL summed over trades in the token's creation slot (human SOL).
   *  `null` for tokens predating the metric or with no creation-slot activity. */
  first_slot_buy_sol: number | null;
  first_slot_sell_sol: number | null;
  ath_price: number | null;
  ath_timestamp: string | null;
  market_cap: number | null;
  initial_buy_sol: number | null;
  initial_supply_token: number | null;
  token_amount: number | null;
  max_cost_lamports: number | null;
  spendable_lamports_in: number | null;
  min_tokens_out: number | null;
  cu_limit: number | null;
  cu_price: number | null;
  ix_labels_count: number;
  instruction_labels: unknown;
  is_migrated: boolean;
  /** Dead-token verdict: liquidity gone + price back at launch + only dust trading
   *  (backend `TokenState::is_dead`). A near-stable flag once true. */
  is_dead: boolean;
  is_mayhem_mode: boolean;
  is_cashback_enabled: boolean;
  created_at: string;
  /** `created_at` pre-parsed to epoch-ms once at ingest (see apiSlice transform),
   *  so age cells never re-parse the ISO string per render/tick. */
  created_at_ms?: number;
  creation_tx_signature: string;
  last_trade_at: string | null;
  /** Gap-aware lifetime in seconds (creation → last non-stray trade); `Some` only
   *  when the token is dead, per backend `TokenState::lifetime_secs`. */
  lifetime_secs: number | null;
  last_synced_at: string | null;
}

export interface FetchTokensResult {
  total: number;
  items: TokenRecord[];
}

/** A rule's last-simulation rollup — computed server-side in-memory (no DB row)
 *  whenever a backtest finishes; see hunter's `state::sim_summary`. `win_rate`
 *  is a fraction (0..1), matching `SimulatedSummary.win_rate` (× 100 to render).
 *  `is_stale` flags a rollup from before the rule's params were last edited. */
export interface RuleLastSimulation {
  n_fired: number;
  closed: number;
  win_rate: number;
  avg_pnl_pct: number;
  total_pnl_sol: number;
  computed_at: string;
  is_stale: boolean;
}

export interface RuleRecord {
  id: string;
  rule_name: string;
  p_token_initial_buy_sol: number | null;
  p_token_first_slot_buy_sol: number | null;
  p_token_first_slot_sell_sol: number | null;
  p_token_cu_limit: number | null;
  p_token_cu_price: number | null;
  p_token_max_sol_cost: number | null;
  p_token_spendable_sol_in: number | null;
  p_max_concurrent_tokens: number | null;
  p_max_total_tokens: number | null;
  p_token_ix_labels: unknown;
  trade_mode: string;
  buy_amount_sol: number;
  p_exit_take_profit: number;
  p_exit_stop_loss: number;
  p_exit_trailing_stop_pct: number | null;
  p_exit_time_stop_secs: number | null;
  p_exit_stall_secs: number | null;
  p_exit_liquidity_drop_pct: number | null;
  // Scalp-continuation gates (tpsl2 only; null on tpsl1). 0/null = disabled.
  p_entry_min_age_secs: number | null;
  p_entry_max_age_secs: number | null;
  p_entry_min_alive_sol: number | null;
  p_entry_min_net_buy_sol: number | null;
  p_entry_pullback_pct: number | null;
  p_entry_higher_low_secs: number | null;
  p_entry_min_liquidity_sol: number | null;
  // ── swing1-only params (null on tpsl1/tpsl2). Merged flat into the rule JSON by
  //    the backend `rule_to_json`; 0/null = that knob is off. ──
  p_swing_high_to_low_sol?: number | null;
  p_swing_high_to_low_pct?: number | null;
  p_swing_low_to_high_sol?: number | null;
  p_swing_low_to_high_pct?: number | null;
  p_swing_min_leg_trades?: number | null;
  p_dust_frac?: number | null;
  p_kill_depth_min_pct?: number | null;
  p_kill_max_duration_ms?: number | null;
  p_kill_min_net_flow_per_sec?: number | null;
  p_vol_depth_max_pct?: number | null;
  p_vol_min_duration_ms?: number | null;
  p_vol_min_up_duration_ms?: number | null;
  p_min_kills_before_volume?: number | null;
  p_exit_next_kill_depth_min_pct?: number | null;
  p_exit_next_kill_max_duration_ms?: number | null;
  bucket_width_sol: number;
  is_active: boolean;
  /** Derived lifecycle for the UI: 'Active' | 'Draining' | 'Idle' | 'Finished'.
   *  `is_active` gates entries only, so an inactive rule with open positions is
   *  'Draining' (its exits keep running until they close). */
  lifecycle: string;
  /** Positions this rule currently holds open (drives the 'Draining (N)' badge
   *  and gates the Stop & close action). */
  open_positions: number;
  /** Not-yet-filled positions this rule currently has buy-in-flight
   *  (`BuySubmitted`) — distinct from `open_positions` (`Holding` only). */
  pending_positions: number;
  /** Realized-performance stats from the runtime cache (all-time for real rules,
   *  current-run for paper). `total_positions` counts entered positions;
   *  `win_count`/`loss_count` cover only closed positions; `win_rate` (0-100) and
   *  `avg_pnl_pct` are 0 until something closes. `avg_pnl_pct` is the capital-weighted
   *  return (Σ realized PnL ÷ Σ SOL deployed × 100), so its sign always matches
   *  `total_pnl_sol` — the realized SOL PnL. */
  total_positions: number;
  win_count: number;
  loss_count: number;
  win_rate: number;
  avg_pnl_pct: number;
  total_pnl_sol: number;
  /** Rollup of this rule's last finished backtest, `null` if never simulated
   *  (or the process restarted since — the cache is in-RAM, not persisted). */
  last_simulation: RuleLastSimulation | null;
  created_at: string;
  updated_at: string;
}

/** Response shape for the bulk lifecycle endpoints (`rules/pause-all`,
 *  `rules/stop-all`): the rules that transitioned successfully, plus any that
 *  failed (best-effort batch — one rule's failure doesn't block the rest). */
export interface BulkRuleResult {
  updated: RuleRecord[];
  failed: { rule_id: string; error: string }[];
}

export interface RulePositionRecord extends TokenEnrichmentFields {
  id: string;
  mint_address: string;
  wallet: string;
  /** Target (trigger-trade) snapshot — the scalp-entry signal trade that armed
   * this position, distinct from the actual entry fill. null until armed. The
   * gap vs. the entry_* fields is derived client-side, not stored. */
  target_price: number | null;
  /** Trigger trade's TOKEN count; SOL derived as `target_price × target_token_amount`. */
  target_token_amount: number | null;
  target_time: string | null;
  target_tx: string | null;
  /** Entry-fill price. null while the position is armed (target set) but the
   *  entry trade hasn't filled yet — the DB columns are nullable, so every
   *  render/computation must null-guard. */
  entry_price: number | null;
  /** Tokens bought at entry; SOL derived as `entry_price × entry_token_amount`.
   *  null until the entry fills (see `entry_price`). */
  entry_token_amount: number | null;
  entry_time: string | null;
  entry_tx: string;
  exit_price: number | null;
  /** Tokens sold at exit; SOL derived as `exit_price × exit_token_amount`. */
  exit_token_amount: number | null;
  exit_time: string | null;
  exit_tx: string | null;
  pnl_percent: number | null;
  /** Realized SOL PnL from the backend (null until the position closes). */
  pnl_sol: number | null;
  status: string;
  strategy: string;
  /** Owning rule id; null if the rule was deleted (`ON DELETE SET NULL`). */
  rule_id: string | null;
  /** Why the position exited (TakeProfit/StopLoss/TrailingStop/Stall/TimeStop/
   * LiquidityExit); null while still open. */
  exit_reason: string | null;
  /** Owning run's monotonic sequence — populated ONLY by the run-history ("old
   * runs") view, where it drives the run column + per-run banding. null/absent on
   * the current-run/live paths (single run). */
  run_seq?: number | null;
  /** `bot` | `manual` — who opened the position. */
  origin?: string;
  /** Manual-position TP/SL config (`{tp_pct, sl_pct}`); null on bot rows. */
  manual_exit?: { tp_pct?: number | null; sl_pct?: number | null } | null;
  /** Backend-derived B3 flag: stale unresolved BuySubmitted — needs Verify. */
  needs_review?: boolean;
  /** ExitStuck redrive state (mig 0012): parked ⇒ auto-retry stopped. */
  exit_parked?: boolean;
  exit_redrive_count?: number;
  /** Why the most recent buy attempt did not fill — send error or Anchor code
   *  (mig 0017). This is what explains an `EntryFailed` row, which carries no
   *  `exit_reason` (nothing ever exited). Not cleared on a later success, so on a
   *  `Holding` row it is the history of what it took to get in. */
  last_entry_error?: string | null;
  created_at: string;
  updated_at: string;
  // Token enrichment fields (populated by the batch endpoint) come from
  // `TokenEnrichmentFields`; only the record-specific extras stay here.
}

/** One page of a rule's positions plus the run/rule-wide `total` (from the
 *  `X-Total-Count` header) so the pager can size itself without fetching the
 *  whole population. */
export interface RulePositionsPage {
  items: RulePositionRecord[];
  total: number;
}

/** Run/rule-wide position aggregates for the Positions Summary panel — computed
 *  server-side over the *entire* population (never a page), mirroring the backend
 *  `PositionsSummary`. `tokens` = entered positions; `open` = holding-index rows;
 *  a win is a clean `End` exit with positive realized SOL. SOL fields are human SOL. */
export interface PositionsSummary {
  tokens: number;
  open: number;
  win: number;
  loss: number;
  closed: number;
  win_rate: number;
  avg_pnl_pct: number;
  /** Realized only — closed positions. Never includes an open position's mark. */
  total_pnl_sol: number;
  /** Unrealized mark-to-current-price PnL of the still-open positions, priced
   *  through the same cost model the sim and the sweep use. Reported beside
   *  `total_pnl_sol`, never folded into it, so a rule holding its losers open
   *  can't read as profitable. */
  open_pnl_sol: number;
  total_entry_sol: number;
  total_holding_sol: number;
  total_gains_sol: number;
  total_losses_sol: number;
  avg_hold_secs: number;
  best_pct: number | null;
  worst_pct: number | null;
  /** Entered tokens that graduated to AMM (`is_migrated`) — counted among
   *  `tokens`, independent of exit reason. */
  migrated: number;
  /** Closed-position counts by `exit_reason`. Deliberately not exhaustive of
   *  `closed` — a close can carry no reason at all — so the summary
   *  reconciles the remainder into a visible `Other` slice. Optional for older
   *  live binaries that predate the exits aggregate. */
  exits?: ExitReasonCounts;
}

/** Mirrors the Rust `ExitReasonCounts`. */
export interface ExitReasonCounts {
  take_profit: number;
  stop_loss: number;
  /** Total metric-condition exits (`metrics_win + metrics_loss`). */
  metrics: number;
  /** Metric exits with positive realized SOL. */
  metrics_win?: number;
  /** Metric exits that are not wins (loss or break-even). */
  metrics_loss?: number;
  dead: number;
  manual: number;
  trailing: number;
  stall: number;
  time: number;
  liquidity: number;
  next_kill: number;
}

export interface MatchedTokenRecord extends TokenEnrichmentFields {
  mint_address: string;
  // These are always present on a matched row, so they narrow the optional
  // `TokenEnrichmentFields` members to required (the rest come from the base).
  symbol: string;
  name: string;
  created_at: string;
  initial_buy_sol: number | null;
  cu_limit: number | null;
  cu_price: number | null;
}

export interface MatchedTokensResponse {
  tokens: MatchedTokenRecord[];
  /** True count before cap was applied. */
  total: number;
  /** True when total > 5000 and only the first 5000 are in `tokens`. */
  capped: boolean;
}

export interface SimulatedTokenResult extends TokenEnrichmentFields {
  mint_address: string;
  symbol: string;
  /** Whether the strategy took a position. `false` ⇒ matched fingerprint but
   *  never entered (`exit_reason: "NoEntry"`) — same contract as sweep
   *  `ComboTokenResult.fired`. Absent on legacy resident payloads. */
  fired?: boolean;
  /** Trigger-trade (scalp signal) snapshot that armed the position, distinct
   * from the worst-case `entry_*` fill. The gap is the modeled adverse
   * slippage. null only for legacy paper rows that never recorded a target. */
  target_price: number | null;
  /** Trigger trade's TOKEN count; SOL derived as `target_price × target_token_amount`. */
  target_token_amount: number | null;
  target_time: string | null;
  target_tx: string | null;
  /** Null when not fired (`NoEntry`). */
  entry_price: number | null;
  /** All-time-high price from `tokens_info` (row-owned enrichment); null if the
   *  token has no info row. */
  ath_price: number | null;
  /** SOL notional the rule deployed (rendered as entry size); null when not fired. */
  entry_token_amount: number | null;
  entry_tx: string | null;
  entry_time: string | null;
  exit_price: number | null;
  exit_tx: string | null;
  exit_time: string | null;
  holding_secs: number | null;
  pnl_percent: number | null;
  pnl_sol: number | null;
  /** Includes `"NoEntry"` for matched-but-never-entered candidates. */
  exit_reason: string;
  // Token enrichment fields come from `TokenEnrichmentFields` (the backend bakes
  // them in once per backtest run via `lab::strategies::token_enrich`); only
  // `created_at`, which the base leaves to row-owners, stays here.
  created_at?: string;
}

/** Filtered-population aggregate for the Simulated summary card (server-side over
 *  rows matching the table's search/filters). Mirrors the lab `sim_result_summary`
 *  handler, which serializes the core kernel's `RunSummary` verbatim.
 *
 *  This **is** the shared run-summary shape — the same two-band `realized`/`mtm`
 *  payload the grouped sweep aggregates to and the live/paper positions card maps
 *  onto — so one renderer (`lib/strategy/runSummary`) draws all three. It replaced
 *  a narrow five-field shape whose `total_pnl_sol` silently folded in unrealized
 *  open marks (parity plan B4/F1). */
export interface SimulatedSummary extends RunSummary {
  /** ISO time the run's result was generated — rendered as relative time ("20m
   *  ago") in the Simulate table's Run column. Null/absent for legacy payloads. */
  computed_at?: string | null;
  /** Fired tokens that graduated to AMM (`is_migrated`), over the filtered
   *  cohort. Absent on legacy payloads → the Migrated tile is then hidden. */
  n_migrated?: number;
  /** Distinct tokens whose creation axes matched this rule's fingerprint — the
   *  candidate pool `realized.n_fired` positions are drawn from (entered one or
   *  more times, or matched-but-never-entered `NoEntry`). Counted **once per
   *  token**, unlike `realized.n_fired`/`n_closed`/`n_open`, which count
   *  positions — a re-entry rule (`RuleParams.reentry`) can enter the same token
   *  more than once, so `n_fired` can exceed `n_matched`. On the Simulate
   *  table's per-rule hydrate this is the run's full unfiltered count
   *  (`SimMeta::n_matched`); on the drill-in summary card it's scoped to the
   *  table's current search/filters, like the other counts there. Absent on
   *  legacy payloads computed before this field existed (re-run the simulation
   *  to backfill). */
  n_matched?: number;
  /** Distinct tokens the rule actually **entered** (≥1 fired episode) — narrower
   *  than `n_matched` (the whole candidate pool, entered or not) and different
   *  from `realized.n_fired` (every entry; a re-entry rule's repeat visits to
   *  one mint each add to it). `realized.n_fired - n_tokens_entered` is the
   *  run's re-entry volume: `0` for a one-shot rule, positive whenever a mint
   *  fired more than once. Same filtered/unfiltered scoping as `n_matched`.
   *  Absent on legacy payloads computed before this field existed (re-run the
   *  simulation to backfill). */
  n_tokens_entered?: number;
  /** Which fill model priced this run's round-trips — rendered as the Simulate
   *  table's Fill column. Absent/null on legacy payloads → falls back to the
   *  default (worst-case) label. */
  fill_model?: FillModelId | null;
  /** Which execution-cost model priced this run's round-trips — rendered as the
   *  Simulate table's Cost column. Absent/null on legacy payloads → falls back to
   *  the default (`pumpfun_default`) label. */
  cost_model?: CostModelId | null;
}

/** Hold + wall-clock bins for the Temporal summary band — mirrors lab
 *  `sim_query::time_summary` / FE `buildTemporalSummary`. */
export interface TemporalSummaryPayload {
  hold: import('lib/strategy/temporalSummary').HoldBinStats[];
  /** Hold-duration scale actually used (auto or override). */
  holdScheme?: import('lib/strategy/temporalSummary').HoldScheme;
  /** Auto pick for this cohort (present even when `holdScheme` was overridden). */
  holdSchemeAuto?: import('lib/strategy/temporalSummary').HoldScheme;
  wall: import('lib/strategy/temporalSummary').WallCellStats[];
  wallGrain: import('lib/strategy/temporalSummary').WallGrain;
  /** Auto pick for this cohort (present even when `wallGrain` was overridden). */
  wallGrainAuto?: import('lib/strategy/temporalSummary').WallGrain;
  /** max−min wall timestamps in ms. */
  wallSpanMs?: number;
  wallField: 'entry_time' | 'created_at';
  nFired: number;
}

/** Metadata for one paper-test run (a single activate→finish cycle). */
export interface PaperRunResponse {
  run_seq: number;
  /** "Running" | "Finished" | "Stopped". */
  status: string;
  max_total_tokens: number | null;
  started_at: string;
  finished_at: string | null;
}

/** Result of `GET /strategies/tpsl/rules/{id}/paper-result`. `run` is null when
 *  the rule has never been run in paper mode; `tokens` are the latest run's
 *  recorded positions, shaped like a simulation result for the shared card/table. */
export interface PaperResultResponse {
  rule_name: string;
  run: PaperRunResponse | null;
  tokens: SimulatedTokenResult[];
}

/** Payload of the `paper_test_finished` SSE event (cap reached + all positions exited). */
export interface PaperTestFinishedEvent {
  rule_id: string;
  rule_name: string;
  run_seq: number;
  tokens_traded: number;
  timestamp: string;
}

/** Payload of the `simulation_progress` SSE event: `processed` of `total`
 *  candidate tokens resolved for the in-flight backtest of `rule_id`. */
export interface SimulationProgressEvent {
  rule_id: string;
  processed: number;
  total: number;
}

/** Payload of the `sweep_progress` SSE event: `processed` of `total` tokens
 *  folded across all surviving groups of the in-flight grouped sweep. `phase`
 *  identifies which phase is reporting: `"corpus"` (lake load; `total: 0` ⇒
 *  indeterminate) | `"coarse"` (refine runs only) | `"sweep"`. */
export interface SweepProgressEvent {
  strategy_id: string;
  phase: string;
  processed: number;
  total: number;
}

/** Payload of the `sweep_group_done` SSE event: one grouped-sweep group is
 *  fully folded AND committed — its rows are readable from the run's `groups`
 *  endpoint right now, mid-run. One frame per persisted group plus an announce
 *  frame (`group_index: null`, `groups_done: 0`) when the surviving counts are
 *  first known. Drives the live groups-table refresh + "persisted N/M". */
export interface SweepGroupDoneEvent {
  strategy_id: string;
  run_id: string;
  group_index: number | null;
  groups_done: number;
  group_count: number;
}

/** Payload of the `sweep_notice` SSE event: a NON-terminal advisory for a running
 *  grouped sweep — today, that the engine degraded its sizing to fit free RAM
 *  (fewer threads / smaller fold buffers). The run continues and its results are
 *  unaffected; it is only slower. Surfaced as an info toast so a degraded run
 *  reads as "slow, and here's why" rather than as a stall. */
export interface SweepNoticeEvent {
  strategy_id: string;
  message: string;
}

/** Payload of the `sweep_finished` SSE event: the single-flight grouped sweep
 *  for `strategy_id` ended (`cancelled` = user abort vs normal finish/error).
 *  `error` is set on a failure or a short write. The run row is kept in every
 *  case (status `partial` when some groups committed, `failed` when none did),
 *  so this toast explains a run the user can still open and inspect. */
export interface SweepFinishedEvent {
  strategy_id: string;
  cancelled: boolean;
  error?: string | null;
}

/** Payload of the `simulation_finished` SSE event: the backtest for `rule_id`
 *  ended (`cancelled` = user abort vs normal finish/error). */
export interface SimulationFinishedEvent {
  rule_id: string;
  cancelled: boolean;
}

/** Response of `GET /api/jobs/status` — a snapshot of every running background
 *  job, used to recover the progress UI after a page load/refresh (SSE only
 *  delivers future frames). `sweep` is present iff the single-flight sweep runs. */
export interface JobsStatus {
  sweep: { processed: number; total: number } | null;
  simulations: { rule_id: string; processed: number; total: number }[];
  /** Present iff the single-flight flow-discovery job is running. */
  discovery: { processed: number; total: number } | null;
}

/** Payload of the `flow_discovery_progress` SSE event. */
export interface FlowDiscoveryProgressEvent {
  run_id: string;
  phase: string;
  processed: number;
  total: number;
}

/** Payload of the `flow_discovery_finished` SSE event. */
export interface FlowDiscoveryFinishedEvent {
  run_id: string;
  cancelled: boolean;
  error?: string | null;
}

/** Payload of the `flow_discovery_notice` SSE event. */
export interface FlowDiscoveryNoticeEvent {
  run_id: string;
  message: string;
}

/** One wallet's gross SOL contribution to a `FlowDiscoveryStructure`. */
export interface FlowDiscoveryWalletGross {
  wallet_hash: string;
  gross_sol: number;
}

/** One ranked ix-structure from a flow-discovery group. */
export interface FlowDiscoveryStructure {
  ix_labels: string[];
  volume_share: number;
  wash_symmetry: number;
  cross_token_recurrence: number;
  group_lift: number;
  slot_burst: number;
  wallet_reuse: number;
  wallet_overlap: number;
  n_trades: number;
  gross_sol: number;
  buy_sol: number;
  sell_sol: number;
  wallets: FlowDiscoveryWalletGross[];
}

/** One token's aggregate contribution to a `FlowDiscoveryGroup` — a cheap
 *  roster (no trade payload) driving the per-token preview picker. */
export interface FlowDiscoveryTokenGross {
  mint_address: string;
  gross_sol: number;
  n_trades: number;
}

/** One fingerprint group in a flow-discovery result. */
export interface FlowDiscoveryGroup {
  group_key: Record<string, string>;
  n_tokens: number;
  n_trades_scored: number;
  ambiguity: boolean;
  structures: FlowDiscoveryStructure[];
  /** Ranked (desc gross_sol) member-token roster, capped server-side. */
  tokens: FlowDiscoveryTokenGross[];
}

/** `GET /api/strategies/flow-discovery/{run_id}` response. */
export interface FlowDiscoveryResult {
  run_id: string;
  groups: FlowDiscoveryGroup[];
}

/** The live (real) strategy managing a held mint — the Holdings bot badge and the
 *  manual-vs-bot double-sell guard. Mirrors the backend `ManagedMint`. */
export interface ManagedBy {
  mint_address: string;
  rule_id: string | null;
  rule_name: string | null;
  /** Open-partition status (`BuySubmitted` | `Holding` | `ExitPending` |
   *  `ExitStuck` | `ExitUnconfirmed`). */
  status: string;
  /** `real` | `paper` (Holdings only ever surfaces `real`). */
  mode: string;
}

/** One enriched wallet holding from `GET /api/portfolio/holdings`. Extends the
 *  shared {@link TokenEnrichmentFields} (the backend flattens the same enrichment
 *  SSOT onto each row) and adds the live wallet fields, SOL valuation, cost basis,
 *  unrealized PnL, and the bot-managed tag. `is_migrated`/`is_cashback_enabled`/
 *  `symbol` carry the live-authoritative values. PnL fields are `undefined` on the
 *  lean single-mint confirmation response until the next full refresh. */
/** Wallet classification — mirrors Rust `AssetKind` (`cash` | `wrapped_sol` | `meme`). */
export type WalletAssetKind = 'cash' | 'wrapped_sol' | 'meme';

export interface WalletHolding extends TokenEnrichmentFields {
  mint_address: string;
  amount: number;
  ui_amount: number;
  decimals: number;
  token_account: string;
  token_program_id: string;
  /** Server classification; cash = dry powder (USDC), not a trading position. */
  asset_kind?: WalletAssetKind;
  price_usd: number | null;
  value_usd: number | null;
  liquidity: number | null;
  price_change_24h: number | null;
  token_created_at: string | null;
  is_migrated: boolean;
  is_cashback_enabled: boolean;
  /** Mark-to-market SOL value of the bag; `null`/absent when no live SOL mark. */
  value_sol?: number | null;
  /** Remaining bag's cost basis in SOL; `null`/absent when no recorded buys / cash. */
  cost_basis_sol?: number | null;
  unrealized_pnl_sol?: number | null;
  unrealized_pnl_pct?: number | null;
  /** The live strategy managing this bag, or `null` when unmanaged (orphan/manual). */
  managed_by?: ManagedBy | null;
}

/** Wallet-wide roll-up from `GET /api/portfolio/summary` — the Home KPI row.
 *  Mirrors the backend `PortfolioSummary`. Value totals include cash; cost basis /
 *  unrealized PnL / `position_count` are meme positions only. */
export interface PortfolioSummary {
  total_value_sol: number;
  total_value_usd: number;
  cash_value_usd: number;
  cash_value_sol: number;
  positions_value_sol: number;
  positions_value_usd: number;
  total_cost_basis_sol: number;
  total_unrealized_pnl_sol: number;
  /** Held meme bags (excludes cash). */
  position_count: number;
  /** Realized SOL PnL from real positions that cleanly exited since 00:00 UTC. */
  realized_pnl_today_sol: number;
  /** Active real-mode rules. */
  active_rules: number;
  /** Open real strategy positions across all rules. */
  open_position_count: number;
}

/** One open strategy position from `GET /api/portfolio/positions` — the fields the
 *  Home per-strategy strip and the Live-Trading roll-up read (a subset of the
 *  backend `StrategyPosition`). */
export interface OpenStrategyPosition {
  /** `strategy_positions.id` — required for the per-row Sell ALL close path. */
  id: string;
  strategy_id: string;
  rule_id: string | null;
  mint_address: string;
  mode: string;
  status: string;
  entry_price?: number | null;
  entry_sol?: number | null;
  entry_time?: string | null;
  /** `bot` | `manual` — who opened the position. */
  origin?: string;
  /** Backend-derived B3 flag: stale unresolved BuySubmitted, needs Verify. */
  needs_review?: boolean;
  /** ExitStuck redrive state (mig 0012). */
  exit_parked?: boolean;
  exit_redrive_count?: number;
}

/** Closed row from `GET /api/portfolio/recent-closes` (Floor Recent hydrate). */
export interface RecentClosedPosition {
  id: string;
  rule_id: string | null;
  mint_address: string;
  mode: string;
  status: string;
  exit_reason?: string | null;
  entry_price?: number | null;
  exit_price?: number | null;
  entry_time?: string | null;
  /** Human SOL — present on StrategyPosition JSON; used for PnL. */
  entry_sol?: number | null;
  exit_sol?: number | null;
  exit_time?: string | null;
  updated_at?: string;
}

/** `GET /api/portfolio/performance` — Portfolio page rollup. */
export interface PortfolioRulePnl {
  rule_id: string;
  rule_name: string | null;
  closed: number;
  win: number;
  loss: number;
  win_rate: number;
  realized_pnl_sol: number;
  total_entry_sol: number;
}

export interface PortfolioPerformance {
  range: 'today' | '7d' | 'all' | string;
  mode: string;
  since: string | null;
  realized_pnl_sol: number;
  closed: number;
  win: number;
  loss: number;
  win_rate: number;
  by_rule: PortfolioRulePnl[];
}

/// Live, fast-changing market data for one mint (Jupiter). Fetched separately
/// from the slow wallet balance read so the wallet table can refresh values on
/// a poll without re-scanning the chain; merged onto {@link WalletHolding}.
export interface WalletPrice {
  price_usd: number | null;
  liquidity: number | null;
  price_change_24h: number | null;
  token_created_at: string | null;
}

/** Accrued pump.fun cashback for one venue pot ("curve" / "amm"). Lamports are
 *  raw integers (JS-safe at these magnitudes); render as SOL. */
export interface CashbackPot {
  label: string;
  exists: boolean;
  claimable_lamports: number;
  stable_claimable: number;
}

export interface CashbackStatus {
  pots: CashbackPot[];
  total_claimable_lamports: number;
}

/** One pot's outcome from POST /api/cashback/claim. */
export interface CashbackClaimOutcome {
  label: string;
  /** SOL pots: claimed lamports. Stable pot (`is_stable`): raw stable-mint units. */
  claimable_lamports: number;
  /** True for the curve stable pot — claim lands as an SPL token balance, not SOL. */
  is_stable: boolean;
  signature: string | null;
  error: string | null;
}

export interface CashbackClaimResult {
  claimed_lamports: number;
  /** Raw stable-mint units swept from the curve stable pot (separate token). */
  claimed_stable: number;
  pots: CashbackClaimOutcome[];
}

export interface TokenDetailRecord {
  mint_address: string;
  name: string;
  symbol: string;
  creator_wallet: string;
  bonding_curve_address: string | null;
  initial_supply_token: number | null;
  initial_buy_sol: number | null;
  token_amount: number | null;
  max_cost_lamports: number | null;
  spendable_lamports_in: number | null;
  min_tokens_out: number | null;
  cu_limit: number | null;
  cu_price: number | null;
  instruction_labels: unknown;
  is_mayhem_mode: boolean;
  is_cashback_enabled: boolean;
  creation_tx_signature: string;
  created_at: string;
  // Coalesced to 0 by the backend (matches the list endpoint), so never null —
  // unlike the other tokens_info-derived fields below.
  trade_count: number;
  volume_sol_total: number;
  first_slot_buy_sol: number | null;
  first_slot_sell_sol: number | null;
  market_cap: number | null;
  current_price: number | null;
  ath_price: number | null;
  ath_timestamp: string | null;
  is_migrated: boolean;
  unique_wallets: number | null;
  last_trade_at: string | null;
  last_synced_at: string | null;
}

/** One row of the Trader Analysis token table — a full {@link TokenRecord} (so it
 *  renders through the same columns as the All Tokens table) plus the wallet's
 *  interaction stats on that mint. Returned by `GET /api/wallets/:wallet/tokens`,
 *  most-recent-trade first. */
/**
 * One Trader Analysis table row: the full token record plus the wallet's
 * interaction stats AND a reconstructed avg-cost PnL on that mint (backend
 * `wallets.rs::WalletTokenRow` / `kernel::wallet_mint_pnl` — see those doc
 * comments for exactly how each figure is derived and what `wallet_partial_data`
 * means). All scoped to the same look-back window, so a mint the wallet only
 * *exited* can show 0 buys and every PnL figure flagged `wallet_partial_data`.
 */
export interface TraderTokenRow extends TokenRecord {
  /** The wallet's first trade on this mint *within the window* — paired with
   *  `wallet_last_trade_at` as a hold-duration proxy at this per-mint grain. */
  wallet_first_trade_at: string;
  /** `wallet_first_trade_at` pre-parsed to epoch-ms (see labEndpoints transform),
   *  so the analytics panel never re-parses the ISO string per chart render. */
  wallet_first_trade_at_ms?: number;
  /** The wallet's most-recent trade on this mint — the table's default sort. */
  wallet_last_trade_at: string;
  wallet_last_trade_at_ms?: number;
  /** The wallet's buy/sell counts on this mint within the look-back window.
   *  Scoped to the window, so a mint the wallet only *exited* can show 0 buys. */
  wallet_buy_count: number;
  wallet_sell_count: number;
  /** Σ SOL bought/sold in the window (recorded curve-side amount, pre-fee). */
  wallet_buy_sol: number;
  wallet_sell_sol: number;
  /** SOL per raw token unit (same convention as `current_price`); `null` when
   *  that side has no legs in the window. */
  wallet_avg_buy_price: number | null;
  wallet_avg_sell_price: number | null;
  /** `buy_token_amount - sell_token_amount` (raw units). Positive = still
   *  holding a bag; negative only when `wallet_partial_data` is true. */
  wallet_net_token_amount: number;
  /** Realized PnL on the matched (closed) portion, gross of the pump.fun fee. */
  wallet_realized_pnl_sol: number;
  /** Same, net of the measured ~125bps/leg pump.fun protocol fee. */
  wallet_realized_pnl_sol_net_of_fee: number;
  /** `null` when there's no matched cost basis to divide by (no buys). */
  wallet_realized_pnl_pct: number | null;
  /** Mark-to-market PnL on the still-open bag; `null` when there's no open bag
   *  or the current price is unknown. */
  wallet_unrealized_pnl_sol: number | null;
  /** `realized_pnl_sol + (unrealized_pnl_sol ?? 0)` — the one ranking number. */
  wallet_total_pnl_sol: number;
  /** `net_token_amount > 0` — still holding some of this mint. */
  wallet_is_open: boolean;
  /** The wallet sold more than it bought in the window (opening buy predates the
   *  window) — every PnL figure above is a partial-window estimate. */
  wallet_partial_data: boolean;
}

/** Per-token live stats pushed alongside each trade (backend `live_stats`).
 *  Field names mirror {@link TokenRecord} so they patch straight into a row. */
export interface TokenLiveStats {
  current_price: number | null;
  volume_sol_total: number;
  market_cap: number | null;
  trade_count: number;
  ath_price: number | null;
  ath_timestamp: string | null;
  last_trade_at: string | null;
}

export interface LiveTrade {
  mint_address: string;
  wallet: string;
  trade_type: string;
  amount_sol: number;
  token_amount: number;
  price_per_token: number;
  tx_signature: string;
  /** Canonical intra-slot order — must match `TradeRecord` / chart sort. */
  tx_index: number;
  leg_index: number;
  /** Venue-neutral post-trade reserves (optional on older frames). */
  reserve_sol?: number | null;
  reserve_token?: number | null;
  venue?: 'curve' | 'amm' | string;
  slot: number;
  timestamp: string;
  /** Snapshot of the mint's stats after this trade; absent if the token isn't
   *  in the live cache. Used to patch the token grid without re-polling. */
  live?: TokenLiveStats | null;
}

export interface SyncProgressEvent {
  type: 'progress';
  stage: string;
  current: number;
  total: number;
  message: string;
}

export interface SyncCompleteEvent {
  type: 'complete';
  token: TokenDetailRecord;
  trades: TradeRecord[];
}

export interface SyncErrorEvent {
  type: 'error';
  message: string;
}

export type SyncStreamEvent = SyncProgressEvent | SyncCompleteEvent | SyncErrorEvent;

export interface TradeRecord {
  id: string;
  mint_address: string;
  wallet_address: string;
  trade_type: 'buy' | 'sell';
  amount_sol: number;
  token_amount: number;
  price_per_token: number;
  tx_signature: string;
  /** Position of this trade's transaction within its block — the real intra-slot
   *  ordering key. Part of the canonical trade order `slot → tx_index → leg_index`. */
  tx_index: number;
  leg_index: number;
  slot: number;
  block_time: string;
  received_at?: string;
  /** Reserve pair this row prices from (venue-neutral): curve virtual reserves on
   *  curve rows, pool real reserves on amm rows. Spot = reserve_sol / reserve_token. */
  reserve_sol?: number | null;
  reserve_token?: number | null;
  real_sol_reserves?: number | null;
  real_token_reserves?: number | null;
  /** Trading venue: 'curve' (bonding curve) or 'amm' (post-migration PumpSwap). */
  venue?: 'curve' | 'amm';
  /** Ordered ix-structure labels for this trade's tx, when the ingest pipeline
   *  captured them — null/absent on pre-labeling history. Drives flow-split
   *  structural matching (`hunter_engine::metrics::flow_split::ix_hash`). */
  instruction_labels?: string[] | null;
}

export type ProfileType = 'mine' | 'trader' | 'whale' | 'dev';

export interface WalletProfileTag {
  id: string;
  name: string;
  color: string;
  comment: string | null;
  created_at: string;
}

export interface WalletEntry {
  id: string;
  profile_id: string;
  address: string;
  is_tracked: boolean;
  comment: string | null;
  created_at: string;
  last_seen_at: string | null;
}

export interface WalletProfile {
  id: string;
  name: string;
  profile_type: ProfileType;
  created_at: string;
  wallets: WalletEntry[];
  tags: WalletProfileTag[];
}
