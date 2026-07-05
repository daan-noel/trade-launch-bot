import type { ChartSwingLeg } from 'components/token-price-chart';

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
  creator_address?: string;
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
  create_tx_address?: string;
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
  creator_address: string;
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
  create_tx_address: string;
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

export interface RuleRecord {
  id: string;
  rule_name: string;
  p_token_initial_buy_sol: number | null;
  p_token_cu_limit: number | null;
  p_token_cu_price: number | null;
  p_token_max_cost_lamports: number | null;
  p_token_spendable_lamports_in: number | null;
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
  tolerance_pct: number;
  is_active: boolean;
  /** Derived lifecycle for the UI: 'Active' | 'Draining' | 'Idle' | 'Finished'.
   *  `is_active` gates entries only, so an inactive rule with open positions is
   *  'Draining' (its exits keep running until they close). */
  lifecycle: string;
  /** Positions this rule currently holds open (drives the 'Draining (N)' badge
   *  and gates the Stop & close action). */
  open_positions: number;
  /** Not-yet-filled positions this rule currently has armed or buy-in-flight
   *  (`Arming` | `BuySubmitted`) — distinct from `open_positions` (`Holding` only). */
  pending_positions: number;
  /** Realized-performance stats from the runtime cache (all-time for real rules,
   *  current-run for paper). `total_positions` counts entered positions;
   *  `win_count`/`loss_count` cover only closed positions; `win_rate` (0-100) and
   *  `avg_pnl_pct` are 0 until something closes; `total_pnl_sol` is realized SOL. */
  total_positions: number;
  win_count: number;
  loss_count: number;
  win_rate: number;
  avg_pnl_pct: number;
  total_pnl_sol: number;
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
  mint: string;
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
  rule_id: string;
  /** Why the position exited (TakeProfit/StopLoss/TrailingStop/Stall/TimeStop/
   * LiquidityExit); null while still open. */
  exit_reason: string | null;
  /** Owning run's monotonic sequence — populated ONLY by the run-history ("old
   * runs") view, where it drives the run column + per-run banding. null/absent on
   * the current-run/live paths (single run). */
  run_seq?: number | null;
  created_at: string;
  updated_at: string;
  // Token enrichment fields (populated by the batch endpoint) come from
  // `TokenEnrichmentFields`; only the record-specific extras stay here.
  /** Swing1-only: legs harvested from the live exit memo at close. `null`/absent
   *  for tpsl1/tpsl2 positions and for still-open swing1 positions. */
  swing_legs?: ChartSwingLeg[] | null;
}

/** Rule context snapshot bundled with every `tpsl_positions_changed` SSE event
 *  so notification consumers have full context without a round-trip. */
export interface RuleNotifSnapshot {
  rule_name: string;
  trade_mode: string;
  p_token_initial_buy_sol: number | null;
  tolerance_pct: number;
  p_token_cu_limit: number | null;
  p_token_cu_price: number | null;
  p_token_max_cost_lamports: number | null;
  p_token_spendable_lamports_in: number | null;
  p_token_ix_labels: string[];
  p_exit_take_profit: number;
  p_exit_stop_loss: number;
}

/** Payload of the `tpsl_positions_changed` SSE event. The backend ships the
 *  changed row + the rule's live cap counters so the client patches one row + the
 *  badge in place instead of refetching. `position` is present even on removal (so
 *  the client knows which row to drop); `removed` distinguishes the two. */
export interface TpslPositionDelta {
  ruleId: string;
  ruleSnapshot: RuleNotifSnapshot | null;
  position: RulePositionRecord | null;
  removed: boolean;
  openPositions: number;
  pendingPositions: number;
  totalPositions: number;
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
  total_pnl_sol: number;
  total_entry_sol: number;
  total_holding_sol: number;
  total_gains_sol: number;
  total_losses_sol: number;
  avg_hold_secs: number;
  best_pct: number | null;
  worst_pct: number | null;
}

export interface MatchedTokenRecord extends TokenEnrichmentFields {
  mint: string;
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
  mint: string;
  symbol: string;
  /** Trigger-trade (scalp signal) snapshot that armed the position, distinct
   * from the worst-case `entry_*` fill. The gap is the modeled adverse
   * slippage. null only for legacy paper rows that never recorded a target. */
  target_price: number | null;
  /** Trigger trade's TOKEN count; SOL derived as `target_price × target_token_amount`. */
  target_token_amount: number | null;
  target_time: string | null;
  target_tx: string | null;
  entry_price: number;
  /** All-time-high price from `tokens_info` (row-owned enrichment); null if the
   *  token has no info row. */
  ath_price: number | null;
  /** Tokens bought at entry; SOL derived as `entry_price × entry_token_amount`. */
  entry_token_amount: number;
  entry_tx: string;
  entry_time: string;
  exit_price: number | null;
  exit_tx: string | null;
  exit_time: string | null;
  holding_secs: number | null;
  pnl_percent: number | null;
  pnl_sol: number | null;
  exit_reason: string;
  /** swing1-only: the exact leg ledger the sim resolved this row's entry/exit
   *  against, carried by the single-rule backtest so the inspect chart draws them
   *  with no separate `swing1-detect` round-trip. Absent for tpsl1/tpsl2 and for
   *  live-position rows (whose legs, if any, come from the exit memo). */
  swing_legs?: ChartSwingLeg[] | null;
  // Token enrichment fields come from `TokenEnrichmentFields` (the backend bakes
  // them in once per backtest run via `lab::strategies::token_enrich`); only
  // `created_at`, which the base leaves to row-owners, stays here.
  created_at?: string;
}

/** Whole-run aggregate for the Simulated summary card (server-side over the
 *  finished backtest's rows). Mirrors the lab `sim_result_summary` handler. */
export interface SimulatedSummary {
  total_tokens: number;
  closed_tokens: number;
  /** Fraction of closed tokens with pnl_sol > 0 (0..1). */
  win_rate: number;
  avg_pnl_percent: number;
  total_pnl_sol: number;
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
 *  identifies which phase is reporting: `"coarse"` | `"sweep"` | `"saving"`. */
export interface SweepProgressEvent {
  strategy_id: string;
  phase: string;
  processed: number;
  total: number;
}

/** Payload of the `sweep_finished` SSE event: the single-flight grouped sweep
 *  for `strategy_id` ended (`cancelled` = user abort vs normal finish/error). */
export interface SweepFinishedEvent {
  strategy_id: string;
  cancelled: boolean;
}

/** Payload of the `simulation_finished` SSE event: the backtest for `rule_id`
 *  ended (`cancelled` = user abort vs normal finish/error). */
export interface SimulationFinishedEvent {
  rule_id: string;
  cancelled: boolean;
}

/** Payload of the `swing_detection_finished` SSE event: the "Swing Detection All"
 *  run for `run_id` ended (`cancelled` = user abort vs normal finish/error). */
export interface SwingDetectionFinishedEvent {
  run_id: string;
  cancelled: boolean;
}

/** Response of `GET /api/jobs/status` — a snapshot of every running background
 *  job, used to recover the progress UI after a page load/refresh (SSE only
 *  delivers future frames). `sweep` is present iff the single-flight sweep runs. */
export interface JobsStatus {
  sweep: { processed: number; total: number } | null;
  simulations: { rule_id: string; processed: number; total: number }[];
  swings: { run_id: string; processed: number; total: number }[];
}

/** The live (real) strategy managing a held mint — the Holdings bot badge and the
 *  manual-vs-bot double-sell guard. Mirrors the backend `ManagedMint`. */
export interface ManagedBy {
  mint: string;
  rule_id: string | null;
  rule_name: string | null;
  /** `Arming` | `BuySubmitted` | `Holding` | `ExitPending`. */
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
export interface WalletHolding extends TokenEnrichmentFields {
  mint: string;
  amount: number;
  ui_amount: number;
  decimals: number;
  token_account: string;
  token_program_id: string;
  price_usd: number | null;
  value_usd: number | null;
  liquidity: number | null;
  price_change_24h: number | null;
  token_created_at: string | null;
  is_migrated: boolean;
  is_cashback_enabled: boolean;
  /** Mark-to-market SOL value of the bag; `null`/absent when no live SOL mark. */
  value_sol?: number | null;
  /** Remaining bag's cost basis in SOL; `null`/absent when no recorded buys. */
  cost_basis_sol?: number | null;
  unrealized_pnl_sol?: number | null;
  unrealized_pnl_pct?: number | null;
  /** The live strategy managing this bag, or `null` when unmanaged (orphan/manual). */
  managed_by?: ManagedBy | null;
}

/** Wallet-wide roll-up from `GET /api/portfolio/summary` — the Home KPI row.
 *  Mirrors the backend `PortfolioSummary`. */
export interface PortfolioSummary {
  total_value_sol: number;
  total_value_usd: number;
  total_cost_basis_sol: number;
  total_unrealized_pnl_sol: number;
  /** Held bags (token accounts with a balance). */
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
  strategy_id: string;
  rule_id: string | null;
  mint: string;
  mode: string;
  status: string;
  entry_price?: number | null;
  entry_sol?: number | null;
  entry_time?: string | null;
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
  creator_address: string;
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
  create_tx_address: string;
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
  mint: string;
  wallet: string;
  trade_type: string;
  amount_sol: number;
  token_amount: number;
  price_per_token: number;
  tx_signature: string;
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
}

export interface SwingParams {
  high_to_low_threshold_sol: number;
  high_to_low_threshold_pct: number;
  low_to_high_threshold_sol: number;
  low_to_high_threshold_pct: number;
  min_leg_trades: number;
  min_leg_duration_ms: number;
  min_leg_volume: number;
  min_leg_net_flow: number;
  max_leg_trades: number;
  max_leg_duration_ms: number;
  max_leg_volume: number;
  max_leg_net_flow: number;
  // Per-leg-type delta % and net-flow-per-second bounds (0 = no bound).
  swing_high_min_delta_pct: number;
  swing_high_max_delta_pct: number;
  swing_high_min_net_flow_per_sec: number;
  swing_high_max_net_flow_per_sec: number;
  swing_low_min_delta_pct: number;
  swing_low_max_delta_pct: number;
  swing_low_min_net_flow_per_sec: number;
  swing_low_max_net_flow_per_sec: number;
  /** "Big tx" threshold (SOL); 0 = disabled. A single tx >= this confirms a
   *  reversal on its own and anchors a leg's terminal pivot to the last such tx. */
  big_tx_sol: number;
}

export type SwingLegType = 'swing_high' | 'swing_low';

export interface SwingLegRecord {
  type: SwingLegType;
  start_at: number;
  end_at: number;
  duration_ms: number;
  start_price: number;
  end_price: number;
  /** Terminal pivot for charting: last big same-side tx (or price extreme fallback).
   *  Optional for backward compatibility with cached/old responses. */
  pivot_end_at?: number;
  pivot_end_price?: number;
  inflow: number;
  outflow: number;
  net_flow: number;
  trade_count: number;
}

export interface SwingDetectionResult {
  mint: string;
  params: SwingParams;
  count: number;
  swings: SwingLegRecord[];
}

/** One token's swing ledger inside a batch (multi-token) detection response. */
export interface SwingBatchEntry {
  mint: string;
  count: number;
  swings: SwingLegRecord[];
}

export interface SwingBatchResponse {
  params: SwingParams;
  results: SwingBatchEntry[];
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
