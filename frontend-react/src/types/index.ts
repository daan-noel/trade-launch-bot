export type PriceUnit = 'SOL' | 'USD';

export interface PriceUnitState {
  unit: PriceUnit;
  usdRate: number | null;
}

export interface TokenRecord {
  mint_address: string;
  name: string;
  symbol: string;
  creator_address: string;
  trade_count: number;
  current_price: number | null;
  volume_sol_total: number;
  ath_price: number | null;
  ath_timestamp: string | null;
  market_cap: number | null;
  initial_buy_sol: number | null;
  initial_supply_token: number | null;
  token_amount: number | null;
  max_sol_cost: number | null;
  spendable_sol_in: number | null;
  min_tokens_out: number | null;
  cu_limit: number | null;
  cu_price: number | null;
  ix_labels_count: number;
  instruction_labels: unknown;
  is_migrated: boolean;
  is_mayhem_mode: boolean;
  is_cashback_enabled: boolean;
  created_at: string;
  /** `created_at` pre-parsed to epoch-ms once at ingest (see apiSlice transform),
   *  so age cells never re-parse the ISO string per render/tick. */
  created_at_ms?: number;
  create_tx_address: string;
  last_trade_at: string | null;
  /** Gap-aware lifetime in seconds (creation → last non-stray trade); null if no trades. */
  active_lifetime_secs: number | null;
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
  p_token_max_sol_cost: number | null;
  p_token_spendable_sol_in: number | null;
  p_max_concurrent_tokens: number | null;
  p_max_total_tokens: number | null;
  p_token_ix_labels: unknown;
  trade_mode: string;
  buy_amount: number;
  p_exit_take_profit: number;
  p_exit_stop_loss: number;
  p_exit_trailing_stop_pct: number | null;
  p_exit_time_stop_secs: number | null;
  p_exit_stall_secs: number | null;
  p_exit_liquidity_drop_pct: number | null;
  // Scalp-continuation gates (tpsl2 only; null on tpsl1). 0/null = disabled.
  p_entry_min_age_secs: number | null;
  p_entry_min_alive_sol: number | null;
  p_entry_min_organic_sol: number | null;
  p_entry_pullback_pct: number | null;
  p_entry_higher_low_secs: number | null;
  p_entry_max_cohort_held: number | null;
  p_entry_min_liquidity_sol: number | null;
  p_entry_min_organic_liq: number | null;
  p_exit_cohort_ratio: number | null;
  tolerance_pct: number;
  is_active: boolean;
  /** Derived lifecycle for the UI: 'Active' | 'Draining' | 'Idle' | 'Finished'.
   *  `is_active` gates entries only, so an inactive rule with open positions is
   *  'Draining' (its exits keep running until they close). */
  lifecycle: string;
  /** Positions this rule currently holds open (drives the 'Draining (N)' badge
   *  and gates the Stop & close action). */
  open_positions: number;
  created_at: string;
  updated_at: string;
}

export interface RulePositionRecord {
  id: string;
  mint: string;
  wallet: string;
  /** Target (trigger-trade) snapshot — the scalp-entry signal trade that armed
   * this position, distinct from the actual entry fill. null until armed. The
   * gap vs. the entry_* fields is derived client-side, not stored. */
  target_price: number | null;
  target_amount: number | null;
  target_time: string | null;
  target_tx: string | null;
  entry_price: number;
  entry_amount: number;
  entry_time: string | null;
  entry_tx: string;
  exit_price: number | null;
  exit_amount: number | null;
  exit_time: string | null;
  exit_tx: string | null;
  pnl_percent: number | null;
  status: string;
  strategy: string;
  rule_id: string;
  /** Why the position exited (TakeProfit/StopLoss/TrailingStop/Stall/TimeStop/
   * LiquidityExit/CohortExit); null while still open. */
  exit_reason: string | null;
  created_at: string;
  updated_at: string;
}

export interface MatchedTokenRecord {
  mint: string;
  symbol: string;
  name: string;
  created_at: string;
  initial_buy_sol: number | null;
  cu_limit: number | null;
  cu_price: number | null;
}

export interface SimulatedTokenResult {
  mint: string;
  symbol: string;
  entry_price: number;
  ath_price: number;
  entry_amount: number;
  entry_tx: string;
  entry_time: string;
  exit_price: number | null;
  exit_tx: string | null;
  exit_time: string | null;
  holding_secs: number | null;
  pnl_percent: number | null;
  pnl_sol: number | null;
  exit_reason: string;
  total_trades: number;
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

export interface WalletHolding {
  mint: string;
  amount: number;
  ui_amount: number;
  decimals: number;
  token_account: string;
  token_program_id: string;
  symbol: string | null;
  price_usd: number | null;
  value_usd: number | null;
  liquidity: number | null;
  price_change_24h: number | null;
  token_created_at: string | null;
  is_migrated: boolean;
  is_cashback_enabled: boolean;
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
  max_sol_cost: number | null;
  spendable_sol_in: number | null;
  min_tokens_out: number | null;
  cu_limit: number | null;
  cu_price: number | null;
  instruction_labels: unknown;
  is_mayhem_mode: boolean;
  is_cashback_enabled: boolean;
  create_tx_address: string;
  created_at: string;
  trade_count: number | null;
  volume_sol_total: number | null;
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
  sol_amount: number;
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
  sol_amount: number;
  token_amount: number;
  price_per_token: number;
  tx_signature: string;
  leg_index: number;
  slot: number;
  block_time: string;
  received_at?: string;
  virtual_sol_reserves?: number | null;
  virtual_token_reserves?: number | null;
  real_sol_reserves?: number | null;
  real_token_reserves?: number | null;
  /** Trading venue: 'curve' (bonding curve) or 'amm' (post-migration PumpSwap). */
  venue?: 'curve' | 'amm';
}

export interface AnalysisRecord {
  analyzer_name: string;
  score: number;
  indicators: string[];
  computed_at: string;
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

export interface CreatorRecord {
  wallet_address: string;
  tokens_created: number;
  total_volume_sol: number;
  suspiciousness_score: number;
  wash_trade_score: number;
  last_analyzed_at: string | null;
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
