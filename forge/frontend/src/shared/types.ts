// Wire types — mirror the `platform-core` models the `live` HTTP surface returns
// (crates/live/src/http.rs). Amounts are quote/base base units unless a field
// name says otherwise; `*_lamports` is exact native SOL.

// Only the SOL-in encodings (`buy_exact_sol_in`, `buy_exact_quote_in_v2`) are valid
// for a bundler leg and offered in the authoring dropdown (see `legForm.BUY_VARIANTS`).
// The tokens-out `buy`/`buy_v2` + the legacy non-v2 `buy_exact_quote_in` remain in the
// union only so a template authored before the alignment still types when read back.
export type BuyVariant =
  | 'buy'
  | 'buy_exact_sol_in'
  | 'buy_v2'
  | 'buy_exact_quote_in'
  | 'buy_exact_quote_in_v2';

// The ix-layout decoration steps (mirrors `executor_core::DecoStep`). `core` is the
// opaque on-chain instruction block; the rest are wrappers around it. Order matters.
export type DecoStep = 'cu_limit' | 'cu_price' | 'create_ata' | 'core' | 'tip';

export interface LegStructureRecipe {
  variant: BuyVariant;
  slippage_bps_min?: number;
  slippage_bps_max?: number;
  cu_limit_min?: number;
  cu_limit_max?: number;
  cu_price_min?: number;
  cu_price_max?: number;
  tip_quote_min?: number;
  tip_quote_max?: number;
  // Hand-picked ix layout (decoration step order) for legs using this recipe.
  // Omitted ⇒ the executor's canonical buy shape.
  layout?: DecoStep[];
}

export interface PumpfunTemplateParams {
  dev_buy_quote?: number;
  // Dev-buy curve encoding (any of the four buy variants). Omitted ⇒ backend default
  // `buy_exact_sol_in`. A tokens-out encoding (`buy`/`buy_v2`) needs `slippage_bps`.
  dev_buy_variant?: BuyVariant;
  slippage_bps?: number;
  is_mayhem_mode?: boolean;
  cashback_enabled?: boolean;
  bundle_leg_count?: number;
  bundle_quote_per_leg?: number;
  bundle_tip_quote?: number;
  leg_structures?: LegStructureRecipe[];
  // Hand-picked create-tx ix layout (orders CU/tip around the fused create Core).
  // Omitted ⇒ canonical_create.
  create_layout?: DecoStep[];
}

export interface LaunchTemplate {
  id: string;
  template_name: string;
  launchpad_id: number;
  quote_asset_id: number;
  variant: string;
  metadata_template_id: string | null;
  params: PumpfunTemplateParams;
  created_at: string;
  updated_at: string;
}

export interface NewLaunchTemplateInput {
  template_name: string;
  launchpad_id: number;
  variant: string;
  quote_asset_id: number;
  metadata_template_id: string | null;
  params: PumpfunTemplateParams;
}

export interface Launchpad {
  id: number;
  key: string;
  display_name: string;
  default_quote_asset_id: number;
}

export interface QuoteAsset {
  id: number;
  mint: string;
  symbol: string;
  decimals: number;
  is_native: boolean;
  usd_rate?: number | null;
}

export type WalletStatus =
  | 'generated'
  | 'funding'
  | 'funded'
  | 'reserved'
  | 'used'
  | 'retired';

export type WalletRole = 'dev' | 'bundler' | 'treasury' | 'trading';

export type PositionStatus = 'open' | 'closed';

// Per-wallet holding of a launched token — `GET /api/tokens/:mint/positions`
// (TokenPosition in platform-core). Amounts are exact base-unit integers:
// `balance_base` token base units, `cost_quote`/`realized_quote` quote base units
// (lamports for a SOL-quoted token). PnL/value is derived on the client from the
// token's price + decimals.
export interface TokenPosition {
  id: string;
  mint_address: string;
  // Internal storage key (managed_wallets.id UUID) — never rendered; join/render
  // by `wallet_address`, the canonical cross-subsystem wallet identity.
  managed_wallet_id: string;
  wallet_address: string;
  role: WalletRole;
  token_account: string | null;
  balance_base: number;
  cost_quote: number;
  realized_quote: number;
  balance_checked_at: string | null;
  status: PositionStatus;
  created_at: string;
  updated_at: string;
}

// ---- Post-launch management (token-management-plan.md Phase 2) ----

export interface WalletSelection {
  role?: string | null;
  wallet_ids?: string[];
}

// One planned/executed per-wallet leg of a management action.
export interface PlanLeg {
  // Internal storage key (managed_wallets.id UUID); render `wallet_address`.
  managed_wallet_id: string;
  wallet_address: string;
  role: WalletRole;
  token_account: string | null;
  side: string; // 'sell' | 'buy' | 'consolidate'
  amount_base: number;
  spend_quote: number;
  est_quote: number;
  status?: string; // 'confirmed' | 'failed' (execution outcome)
  signature?: string;
  error?: string;
}

// A previewable action plan — `POST /api/tokens/:mint/manage/preview`.
export interface ActionPlan {
  mint_address: string;
  kind: string;
  sizing: string;
  legs: PlanLeg[];
  total_amount_base: number;
  total_est_quote: number;
}

// The operator's action request body.
export interface ManageRequest {
  kind: string; // 'sell'
  sizing: string; // 'pct_of_holdings'
  size: number; // percent for pct_of_holdings
  selection?: WalletSelection;
  // Buy/consolidate only: true (default) = this token's dev/snipe wallets; false =
  // pool-wide by role. Omitted for sell (already mint-scoped via positions).
  token_scoped?: boolean;
}

// Audit row — `POST .../manage/execute` result + `GET .../manage/actions`.
export interface ManageAction {
  id: string;
  mint_address: string;
  kind: string;
  sizing: string;
  selection: unknown;
  plan: PlanLeg[];
  status: string; // planned | executing | completed | partial | failed
  legs_total: number;
  legs_confirmed: number;
  error: string | null;
  created_at: string;
  completed_at: string | null;
}

// ---- Sell ladders (token-management-plan.md Phase 4) ----
export interface LadderRung {
  metric: string; // 'market_cap_usd' | 'price_usd'
  threshold: number;
  pct: number; // % of holdings to sell when crossed
  fired?: boolean;
}

export interface SellLadder {
  id: string;
  mint_address: string;
  selection: WalletSelection;
  rungs: LadderRung[];
  status: string; // armed | done | cancelled
  created_at: string;
  updated_at: string;
}

// ---- Volume-making bots (token-management-plan.md Phase 5) ----
export interface VolumeConfig {
  buy_sol_min: number;
  buy_sol_max: number;
  interval_secs_min: number;
  interval_secs_max: number;
  sell_back_pct: number; // 0 = accumulate (buy-only)
  budget_sol: number; // hard cumulative buy-spend cap
  max_cycles?: number | null;
}

export interface VolumeBot {
  id: string;
  mint_address: string;
  status: string; // running | paused | stopped
  selection: WalletSelection;
  config: VolumeConfig;
  cycles_done: number;
  spent_quote: number; // lamports spent on buys
  volume_quote: number; // lamports traded (buys + sell-backs)
  next_run_at: string;
  last_error: string | null;
  created_at: string;
  updated_at: string;
}

export interface ManagedWalletPool {
  id: string;
  address: string;
  label: string | null;
  role: string;
  derivation_index: number | null;
  status: WalletStatus;
  funding_source: string | null;
  reserved_by_launch_id: string | null;
  reserved_at: string | null;
  balance_lamports: number | null;
  balance_checked_at: string | null;
  created_at: string;
}

export interface WalletFundOutcome {
  managed_wallet_id: string;
  address: string;
  role: string;
  amount_lamports: number;
  result:
    | 'sent'
    | 'dry_run'
    | 'failed'
    | 'skipped_cap'
    | 'skipped_bad_address'
    | 'skipped_funded';
  signature: string | null;
  error: string | null;
}

export interface FundReport {
  spent_lamports: number;
  outcomes: WalletFundOutcome[];
}

// Operator wallet-to-wallet SOL move — `POST /api/wallet_pool/transfer`.
export interface TransferArgs {
  from_id: string;
  to_id: string;
  // Whole SOL; ignored when `max` is set, required otherwise.
  amount_sol?: number;
  // Sweep the source to ~0.
  max?: boolean;
}

export interface TransferReport {
  from_id: string;
  to_id: string;
  from_address: string;
  to_address: string;
  lamports_sent: number;
  signature: string | null;
}

// Per-wallet result of an operator sweep/consolidate pass. `retired` = the wallet
// is retired after the pass; `note` carries a skip reason or error.
export interface WalletSweepOutcome {
  wallet_id: string;
  address: string;
  role: string;
  status: WalletStatus;
  sol_swept_lamports: number;
  rent_reclaimed_lamports: number;
  accounts_closed: number;
  accounts_nonempty_skipped: number;
  retired: boolean;
  signature: string | null;
  note: string | null;
}

// `POST /api/wallet_pool/sweep` + `/consolidate` — aggregate roll-up + per-wallet rows.
export interface SweepReport {
  treasury_id: string;
  treasury_address: string;
  total_sol_swept_lamports: number;
  total_rent_reclaimed_lamports: number;
  wallets_retired: number;
  accounts_closed: number;
  outcomes: WalletSweepOutcome[];
}

export interface MetadataTemplate {
  id: string;
  template_name: string;
  name: string;
  symbol: string;
  description: string | null;
  twitter: string | null;
  telegram: string | null;
  website: string | null;
  image_uri: string | null;
  uri: string;
  created_at: string;
}

export interface NewMetadataTemplate {
  template_name: string;
  name: string;
  symbol: string;
  description?: string;
  twitter?: string;
  telegram?: string;
  website?: string;
  image_base64: string;
  image_filename: string;
  image_content_type: string;
}

export interface UpdateMetadataTemplate {
  template_name: string;
  name: string;
  symbol: string;
  description?: string;
  twitter?: string;
  telegram?: string;
  website?: string;
  // Omit all three image fields to keep the existing pinned image.
  image_base64?: string;
  image_filename?: string;
  image_content_type?: string;
}

export interface LaunchResult {
  launch_id: string;
  mint_address: string;
  create_signature: string;
  bundle?: {
    bundle_id: string;
    jito_bundle_id: string;
    leg_signatures: string[];
  };
}

export interface LaunchOverrides {
  metadata_template_id?: string;
  bundler_count?: number;
}

// Pre-launch cost preview — `GET /api/launches/requirement`. Mirrors the SSOT the
// launch gate enforces, so the console can show the required amount + shortfall
// before the user clicks Launch (never discovering it via a failed request).
export interface LaunchRequirement {
  dev_required_lamports: number;
  per_leg_lamports: number;
  leg_count: number;
  dev_balance_lamports: number | null;
}

export interface Launch {
  id: string;
  mint_address: string;
  status: string;
  create_signature: string | null;
  bundle_id: string | null;
}

export interface Bundle {
  id: string;
  status: string;
  jito_bundle_id: string | null;
  leg_signatures: string[];
  submitted_at: string | null;
  confirmed_at: string | null;
}

// Enriched launch row — `GET /api/launches` (LaunchListRow in platform-core).
export interface LaunchListRow {
  id: string;
  template_id: string | null;
  mint_address: string;
  variant: string;
  status: string;
  create_signature: string | null;
  dev_buy_quote: number | null;
  bundle_id: string | null;
  bundle_status: string | null;
  created_at: string;
  name: string | null;
  symbol: string | null;
  trade_count: number | null;
  is_migrated: boolean | null;
  is_dead: boolean | null;
  price_usd: number | null;
  market_cap_usd: number | null;
  // Token decimals — divides holding_base to a human token amount.
  token_decimals: number | null;
  // Quote-asset decimals — divides holding_value_quote to human SOL.
  quote_decimals: number | null;
  // Tokens still held across open positions (token base units, SUM(balance_base)).
  // As-of the last reconcile (list load doesn't RPC-probe) — see backend list_page.
  holding_base: number | null;
  // Cost basis of those open positions, quote base units (SUM(cost_quote)).
  holding_cost_quote: number | null;
  // SOL value of the holding, quote base units (holding_base * current_price_quote).
  holding_value_quote: number | null;
}

export interface LaunchesPage {
  launches: LaunchListRow[];
  total: number;
}

// `token_overview` view row — GET /api/tokens/:mint/overview.
export interface TokenOverview {
  mint_address: string;
  launchpad_id: number;
  quote_asset_id: number;
  creator_wallet: string;
  is_own_launch: boolean;
  name: string;
  symbol: string;
  decimals: number;
  initial_supply_base: number | null;
  created_at: string;
  current_price_quote: number | null;
  volume_quote: number | null;
  trade_count: number | null;
  is_dead: boolean | null;
  is_migrated: boolean | null;
  quote_symbol: string;
  quote_decimals: number;
  quote_usd_rate: number | null;
  age_secs: number | null;
  price_quote_display: number | null;
  price_usd: number | null;
  market_cap_quote: number | null;
  market_cap_usd: number | null;
}

// `trades_priced` view row — GET /api/tokens/:mint/trades. `tx_signature` is a
// byte array over the wire (see formatSig).
export interface TradePriced {
  mint_address: string;
  // Interned `wallet_dict` ref (soft int key) — the lean feed table keeps it as a
  // ref; resolve to an address only where needed.
  wallet_ref: number;
  // Canonical on-chain wallet identity (resolved from `wallet_dict` in the view).
  // Render THIS, never `wallet_ref` — address is the SSOT wallet identity.
  wallet_address: string;
  launchpad_id: number;
  market_kind: string;
  quote_asset_id: number;
  trade_type: string;
  amount_quote: number;
  amount_base: number;
  reserve_quote: number | null;
  reserve_base: number | null;
  slot: number;
  tx_index: number;
  leg_index: number;
  block_time: string;
  tx_signature: number[] | string;
  quote_symbol: string;
  quote_decimals: number;
  quote_usd_rate: number | null;
  exec_price_quote: number | null;
  spot_price_quote: number | null;
  amount_quote_display: number | null;
  amount_usd: number | null;
}

export interface LaunchStatus {
  launch: Launch;
  bundle: Bundle | null;
  trade_count: number;
  trades: TradePriced[];
}

export interface IngestStatus {
  configured: boolean;
  live: boolean;
  /** Pipeline health (audit Phase B) — present on the status GET when ingest is
   *  configured; absent on toggle responses / when unconfigured. */
  health?: IngestHealth;
}

/** Live health of the ingest write pipeline (the signals the watchdog trips on). */
export interface IngestHealth {
  /** Millis since the DbWriter last committed a batch. */
  commit_age_ms: number;
  /** Ops queued in the consumer→writer channel (null once the channel is gone). */
  buffer_depth: number | null;
  /** The channel's fixed capacity. */
  buffer_capacity: number;
  /** Durable rows committed since boot. */
  events_total: number;
}

export interface BootstrapPayload {
  templates: LaunchTemplate[];
  dev_wallets: ManagedWalletPool[];
  metadata_templates: MetadataTemplate[];
}
