// ============================================================
// trader — Pump.fun trader, split by concern.
//
// Design goals:
//  - Zero-copy nonce acquisition: multi-slot hash cache with
//    in_use flags, no synchronous RPC on the hot path.
//  - Pre-built seed pool (buy templates) with async replenishment
//    so account creation instructions are always ready.
//  - Pre-built Jito tip and compute-budget instructions, built
//    once at init and reused every transaction.
//  - Retry loop on sell (up to 5 attempts, fresh nonce each time).
//  - Background prebuild of the *next* buy template immediately
//    after a buy succeeds or fails, on top of the pool refill —
//    double safety net.
//  - base64 encoding (correct for the JSON-RPC "base64" param).
//  - Confirmation polling extracted into a reusable helper with
//    configurable retries; sell retries re-use it too.
//
// Module map (by priority):
//   mod.rs   — types, config, struct, `new`, accessors  (this file)
//   init.rs  — one-time `initialize` + its helpers
//   buy.rs   — `buy_token`               (hot path)
//   sell.rs  — `sell_token`, `execute_sell` (hot path)
//   tx.rs    — build / send / confirm transaction helpers
//   nonce.rs — durable-nonce acquisition & background refresh
//   pool.rs  — buy-template seed pool
//   query.rs — read-only RPC queries (balances, holdings, creator)
// ============================================================

mod amm;
mod blockhash;
mod buy;
mod init;
mod nonce;
mod pool;
mod query;
mod reserves;
mod sell;
mod tx;

use blockhash::BlockhashCache;
use reserves::ReserveCache;

use crate::constants::{
    EVENT_AUTHORITY, FEE_PROGRAM_ID, PUMP_FUN_PROGRAM_ID, PUMP_PROGRAM_UPGRADE_FEE_RECIPIENT,
    PUMP_SWAP_PROGRAM_ID, WSOL_MINT,
};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::system_program;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    hash::Hash,
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use tokio::sync::Mutex;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Pre-built account-creation instruction + the resulting token account address.
#[derive(Debug, Clone)]
struct BuyTemplate {
    create_with_seed_ix: Instruction,
    user_token_account: Pubkey,
}

/// Per-nonce-account slot held in the hash cache.
#[derive(Debug)]
struct NonceSlot {
    /// Most recently fetched blockhash for this nonce account.
    cached_hash: Option<Hash>,
    /// True while a transaction is in-flight using this slot.
    in_use: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct TokenPDAs {
    pub token_program: Pubkey,
    pub bonding_curve: Pubkey,
    pub bonding_curve_v2: Pubkey,
    pub associated_bonding_curve: Pubkey,
    pub creator_vault: Pubkey,
    pub cashback_enabled: bool,
}

#[derive(Debug, Clone)]
pub struct GlobalAccount {
    pub global_pda: Pubkey,
    pub fee_recipient: Pubkey,
    pub global_volume_accumulator: Pubkey,
    pub user_volume_accumulator: Pubkey,
    pub fee_config: Pubkey,
}

/// Cached PumpSwap pool facts for a migrated token, read once from the on-chain
/// `Pool` account (see offsets in `constants`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct AmmPoolInfo {
    pub pool: Pubkey,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub base_token_program: Pubkey,
    pub pool_base_token_account: Pubkey,
    pub pool_quote_token_account: Pubkey,
    pub coin_creator: Pubkey,
    pub is_cashback_coin: bool,
    /// Per-coin "fee-share" marker account the deployed pump_amm requires in
    /// non-cashback swaps (between `fee_program` and the buyback pair). It's an
    /// uninitialized PDA the program derives but no published IDL documents and
    /// we can't reproduce offline — read from a recent on-chain swap and cached.
    /// `None` for cashback pools (they use a derivable cashback block instead).
    pub fee_share_marker: Option<Pubkey>,
}

/// Cached PumpSwap `GlobalConfig` facts: fee rates (bps) and a chosen protocol
/// fee recipient. Fetched once and reused for slippage math + account building.
#[derive(Debug, Clone, Copy)]
pub(crate) struct AmmGlobalConfig {
    pub lp_fee_bps: u64,
    pub protocol_fee_bps: u64,
    pub coin_creator_fee_bps: u64,
    pub protocol_fee_recipient: Pubkey,
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct TraderConfig {
    pub rpc_url: String,
    pub helius_sender_url: String,
    pub keypair: Keypair,
    pub nonce_accounts: Vec<String>,
}

// ---------------------------------------------------------------------------
// Trader
// ---------------------------------------------------------------------------

pub struct PumpFunTrader {
    config: Arc<TraderConfig>,
    http: reqwest::Client,
    rpc: Arc<RpcClient>,

    // Set once in initialize()
    global_account: Option<GlobalAccount>,
    compute_budget_ixs: Vec<Instruction>,
    jito_tip_ix: Arc<Mutex<Option<Instruction>>>,

    // Nonce management
    nonce_pubkeys: Vec<Pubkey>,
    nonce_cursor: AtomicUsize,
    nonce_slots: Arc<Mutex<HashMap<Pubkey, NonceSlot>>>,

    // Diagnostic counters
    nonce_wait_events: AtomicUsize,
    nonce_wait_iters_total: AtomicUsize,

    // Per token-program buy template pools
    buy_pool_legacy: Arc<Mutex<Vec<BuyTemplate>>>,
    buy_pool_2022: Arc<Mutex<Vec<BuyTemplate>>>,
    buy_pool_misses_legacy: AtomicUsize,
    buy_pool_misses_2022: AtomicUsize,
    seed_counter: AtomicUsize,

    // Rent values (fetched once)
    token_account_space: u64,
    token_account_rent: u64,
    token_2022_account_space: u64,
    token_2022_account_rent: u64,

    // Static program IDs (parsed once)
    pump_program: Pubkey,
    system_program: Pubkey,
    event_authority: Pubkey,
    fee_program: Pubkey,
    upgrade_fee_recipient: Pubkey,

    // PumpSwap AMM (migrated tokens)
    pump_swap_program: Pubkey,
    wsol_mint: Pubkey,
    amm_pool_cache: Arc<Mutex<HashMap<String, AmmPoolInfo>>>,
    amm_global_config: Arc<Mutex<Option<AmmGlobalConfig>>>,

    // Per-token caches
    user_token_accounts: Arc<Mutex<HashMap<String, Pubkey>>>,
    token_pdas: Arc<Mutex<HashMap<String, TokenPDAs>>>,

    // WS-fed live reserve snapshots (mint → latest post-trade reserves), read on
    // the slippage / AMM-reserve hot path with an on-chain fallback.
    reserve_cache: Arc<ReserveCache>,

    // Background-refreshed recent blockhash for the AMM buy path (which can't use
    // a durable nonce — see `build_recent_tx`).
    blockhash_cache: Arc<BlockhashCache>,
}

impl PumpFunTrader {
    // -----------------------------------------------------------------------
    // Construction
    // -----------------------------------------------------------------------

    pub fn new(config: Arc<TraderConfig>) -> Self {
        let rpc = Arc::new(RpcClient::new_with_commitment(
            config.rpc_url.clone(),
            CommitmentConfig::confirmed(),
        ));

        Self {
            config,
            http: reqwest::Client::new(),
            rpc,
            global_account: None,
            compute_budget_ixs: Vec::new(),
            jito_tip_ix: Arc::new(Mutex::new(None)),
            nonce_pubkeys: Vec::new(),
            nonce_cursor: AtomicUsize::new(0),
            nonce_slots: Arc::new(Mutex::new(HashMap::new())),
            nonce_wait_events: AtomicUsize::new(0),
            nonce_wait_iters_total: AtomicUsize::new(0),
            buy_pool_legacy: Arc::new(Mutex::new(Vec::new())),
            buy_pool_2022: Arc::new(Mutex::new(Vec::new())),
            buy_pool_misses_legacy: AtomicUsize::new(0),
            buy_pool_misses_2022: AtomicUsize::new(0),
            seed_counter: AtomicUsize::new(0),
            token_account_space: 165,
            token_account_rent: 2_000_000,
            token_2022_account_space: 182,
            token_2022_account_rent: 2_000_000,
            pump_program: Pubkey::from_str(PUMP_FUN_PROGRAM_ID).unwrap(),
            system_program: system_program::id(),
            event_authority: Pubkey::from_str(EVENT_AUTHORITY).unwrap(),
            fee_program: Pubkey::from_str(FEE_PROGRAM_ID).unwrap(),
            upgrade_fee_recipient: Pubkey::from_str(PUMP_PROGRAM_UPGRADE_FEE_RECIPIENT).unwrap(),
            pump_swap_program: Pubkey::from_str(PUMP_SWAP_PROGRAM_ID).unwrap(),
            wsol_mint: Pubkey::from_str(WSOL_MINT).unwrap(),
            amm_pool_cache: Arc::new(Mutex::new(HashMap::new())),
            amm_global_config: Arc::new(Mutex::new(None)),
            user_token_accounts: Arc::new(Mutex::new(HashMap::new())),
            token_pdas: Arc::new(Mutex::new(HashMap::new())),
            reserve_cache: Arc::new(ReserveCache::default()),
            blockhash_cache: Arc::new(BlockhashCache::default()),
        }
    }

    /// Wallet public key for trade correlation.
    pub fn wallet_pubkey(&self) -> String {
        self.config.keypair.pubkey().to_string()
    }

    /// Feed a post-trade reserve snapshot into the live cache. Called by the WS
    /// ingest for every tracked-token trade. `token_reserves` is raw token base
    /// units; `sol_reserves` is in SOL; `is_amm` tags the venue (curve vs AMM).
    /// The trade path reads these (cache-first, freshness-bounded) instead of an
    /// on-chain reserve read — see `curve_reserves` / `amm_reserves_cached`.
    pub fn update_live_reserves(
        &self,
        mint: &str,
        token_reserves: f64,
        sol_reserves: f64,
        is_amm: bool,
    ) {
        self.reserve_cache
            .update(mint, token_reserves, sol_reserves, is_amm);
    }

    /// Expose the RPC URL for callers that need to make their own RPC requests.
    pub fn rpc_url(&self) -> &str {
        &self.config.rpc_url
    }
}
