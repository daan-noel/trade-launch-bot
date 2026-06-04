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
  age: number;
  created_at: string;
  create_tx_address: string;
  last_trade_at: string | null;
}

export interface FetchTokensResult {
  total: number;
  items: TokenRecord[];
}

export interface RuleRecord {
  id: string;
  rule_name: string;
  p_initial_buy_sol: number | null;
  p_cu_limit: number | null;
  p_cu_price: number | null;
  p_max_sol_cost: number | null;
  p_spendable_sol_in: number | null;
  p_max_holding_tokens: number | null;
  p_total_max_trade_tokens: number | null;
  p_ix_labels: unknown;
  trade_mode: string;
  buy_amount: number;
  take_profit: number;
  stop_loss: number;
  tolerance_pct: number;
  is_active: boolean;
  created_at: string;
  updated_at: string;
}

export interface RulePositionRecord {
  id: string;
  mint: string;
  wallet: string;
  entry_price: number;
  exit_price: number | null;
  entry_tx: string;
  exit_tx: string | null;
  status: string;
  strategy: string;
  rule_id: string;
  entry_amount: number;
  exit_amount: number | null;
  pnl_percent: number | null;
  entry_time: string | null;
  exit_time: string | null;
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
  virtual_sol_reserves?: number | null;
  virtual_token_reserves?: number | null;
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
}

export type SwingLegType = 'swing_high' | 'swing_low';

export interface SwingLegRecord {
  type: SwingLegType;
  start_at: number;
  end_at: number;
  duration_ms: number;
  start_price: number;
  end_price: number;
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

export interface CreatorRecord {
  wallet_address: string;
  tokens_created: number;
  total_volume_sol: number;
  suspiciousness_score: number;
  wash_trade_score: number;
  last_analyzed_at: string | null;
}
