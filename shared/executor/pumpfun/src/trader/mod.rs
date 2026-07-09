// ============================================================
// trader — the pump.fun VENUE over the shared `executor-core` engine.
//
// `PumpFunTrader` wraps an `executor_core::Engine` (sign / fan-out send /
// feed-confirm / durable-nonce / Jito tip / retry-classify / sim) and adds the
// pump.fun specifics on top: the curve/AMM caches, the pump ix builders, and
// curve+AMM pricing. It `Deref`s to the engine, so every venue method can call
// the engine's `build_nonce_tx` / `acquire_nonce` / `send_transaction` /
// `confirm_transaction` / `jito_tip_ix` / `simulate_ixs` unchanged.
//
// The engine-side handles (`config` / `rpc` / `http`) are duplicated as cheap
// shared `Arc`/`Client` clones on the venue too, so the ~60 `self.config` /
// `self.rpc` reads across the venue files resolve without an `engine.` hop —
// they point at the SAME allocation the engine holds, never a divergent copy.
//
// Module map (venue):
//   mod.rs   — venue types, struct, `new`, `Deref`/`Venue`, live-reserve feed
//   init.rs  — venue `initialize` (engine.initialize + global account + seed pools)
//   buy.rs   — `buy_token`               (hot path)
//   sell.rs  — `sell_token`, `execute_sell` (hot path)
//   amm.rs   — PumpSwap AMM buy/sell
//   create.rs / bundle_buy.rs — launch + bundle buys
//   pool.rs  — buy-template seed pool
//   query.rs / reserves.rs — read-only RPC queries + live reserves
// The send engine (tx/nonce/blockhash/jito_tip/swap_retry/sim primitive) lives
// in `executor-core`.
// ============================================================

mod amm;
mod bundle_buy;
mod buy;
mod consolidate;
mod create;
#[cfg(feature = "claim")]
pub mod claim;
mod init;
mod pool;
#[cfg(feature = "probe")]
pub mod probe;
mod query;
mod reserves;
mod sell;
pub mod sim;

use dashmap::DashMap;
use reserves::ReserveCache;

pub use bundle_buy::{BundleBuyVariant, BundleLegParams};
pub use buy::BuySignedHook;

use crate::protocol;
use executor_core::config::TraderConfig;
use executor_core::venue::{Venue, VenueId};
use executor_core::Engine;
use rand::seq::SliceRandom;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{instruction::Instruction, pubkey::Pubkey};
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use tokio::sync::Mutex;

// ---------------------------------------------------------------------------
// Venue types
// ---------------------------------------------------------------------------

/// Pre-built account-creation instruction + the resulting token account address.
#[derive(Debug, Clone)]
pub(crate) struct BuyTemplate {
    pub(crate) create_with_seed_ix: Instruction,
    pub(crate) user_token_account: Pubkey,
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
    /// `whitelisted_quote_mints[0]` from the on-chain `Global` account — the
    /// non-WSOL quote enabled for cashback (the "stable" mint). `None` if the
    /// account is older/shorter than that field or the slot is unset. Used only
    /// off the hot path by the stable cashback claim (the `claim` feature).
    #[cfg_attr(not(feature = "claim"), allow(dead_code))]
    pub stable_quote_mint: Option<Pubkey>,
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
// Trader
// ---------------------------------------------------------------------------

pub struct PumpFunTrader {
    /// The venue-agnostic send engine. Every send/nonce/tip/confirm/sim call goes
    /// through this (reachable directly on `self` via `Deref`).
    engine: Engine,

    // Shared handles duplicated from the engine (same allocation, never a
    // divergent copy) so venue-file `self.config` / `self.rpc` / `self.http`
    // reads resolve without an `engine.` hop.
    pub(crate) config: Arc<TraderConfig>,
    pub(crate) http: reqwest::Client,
    pub(crate) rpc: Arc<RpcClient>,

    // Set once in initialize()
    pub(crate) global_account: Option<GlobalAccount>,

    // PumpSwap AMM (migrated tokens). Program-constant PumpSwap PDAs, derived once
    // in `new()` (they never depend on the token, only on the program / this
    // wallet) instead of re-running `find_program_address` on every AMM swap.
    pub(crate) amm_global_config_pda: Pubkey,
    pub(crate) amm_event_authority: Pubkey,
    pub(crate) amm_fee_config: Pubkey,
    pub(crate) amm_global_volume_accumulator: Pubkey,
    pub(crate) amm_user_volume_accumulator: Pubkey,
    // Sharded concurrent maps (`DashMap`): each critical section is a single
    // get/insert with no `.await` held, and sharding means the WS-ingest writer
    // and the trade readers no longer serialize on one global mutex.
    pub(crate) amm_pool_cache: Arc<DashMap<String, AmmPoolInfo>>,
    // GlobalConfig + the instant it was fetched. Governance-mutable fee bps mean
    // a stale cache would silently loosen slippage protection forever; the fetch
    // instant lets `amm_config` refresh past a max-age.
    pub(crate) amm_global_config:
        Arc<std::sync::Mutex<Option<(AmmGlobalConfig, std::time::Instant)>>>,

    // Per-token caches.
    pub(crate) user_token_accounts: Arc<DashMap<String, Pubkey>>,
    pub(crate) token_pdas: Arc<DashMap<String, TokenPDAs>>,
    // Per-mint bonding-curve routing facts (creator / token program / cashback /
    // migration). Served without RPC once a mint is observed migrated.
    pub(crate) curve_routing_cache: Arc<DashMap<String, query::CurveRouting>>,

    // WS-fed live reserve snapshots (mint → latest post-trade reserves), read on
    // the slippage / AMM-reserve hot path with an on-chain fallback.
    pub(crate) reserve_cache: Arc<ReserveCache>,

    // Per token-program buy template pools
    pub(crate) buy_pool_legacy: Arc<Mutex<Vec<BuyTemplate>>>,
    pub(crate) buy_pool_2022: Arc<Mutex<Vec<BuyTemplate>>>,
    pub(crate) buy_pool_misses_legacy: AtomicUsize,
    pub(crate) buy_pool_misses_2022: AtomicUsize,
    pub(crate) seed_counter: AtomicUsize,
}

impl std::ops::Deref for PumpFunTrader {
    type Target = Engine;
    /// Expose the engine's send/nonce/tip/confirm/sim methods directly on the
    /// trader, so every venue call site (`self.acquire_nonce()`,
    /// `self.send_transaction(&tx)`, `self.jito_tip_ix(0)`, `self.simulate_ixs(…)`)
    /// resolves unchanged after the split.
    fn deref(&self) -> &Engine {
        &self.engine
    }
}

impl Venue for PumpFunTrader {
    fn venue_id(&self) -> VenueId {
        VenueId::PumpFun
    }
    fn engine(&self) -> &Engine {
        &self.engine
    }
}

impl PumpFunTrader {
    // -----------------------------------------------------------------------
    // Construction
    // -----------------------------------------------------------------------

    pub fn new(config: Arc<TraderConfig>) -> Self {
        let wallet = config.signer.pubkey();

        // PumpSwap PDAs that never change for this wallet — derived once here so
        // `amm_swap_accounts` is a few field reads instead of ~5 `find_program_address`.
        let amm_global_config_pda =
            Pubkey::find_program_address(&[b"global_config"], &protocol::PUMP_SWAP).0;
        let amm_event_authority =
            Pubkey::find_program_address(&[b"__event_authority"], &protocol::PUMP_SWAP).0;
        let amm_fee_config = Pubkey::find_program_address(
            &[b"fee_config", protocol::PUMP_SWAP.as_ref()],
            &protocol::FEE_PROGRAM,
        )
        .0;
        let amm_global_volume_accumulator =
            Pubkey::find_program_address(&[b"global_volume_accumulator"], &protocol::PUMP_SWAP).0;
        let amm_user_volume_accumulator = Pubkey::find_program_address(
            &[b"user_volume_accumulator", wallet.as_ref()],
            &protocol::PUMP_SWAP,
        )
        .0;

        // Jito tip account — one chosen at random per trader instance from the
        // `const [Pubkey; 10]`; the tip *amount* is sized per trade (engine).
        let jito_tip_account = protocol::JITO_TIP_ACCOUNTS
            .choose(&mut rand::thread_rng())
            .copied()
            .expect("JITO_TIP_ACCOUNTS is a non-empty const array");

        // Build the venue-agnostic engine with the pump SPL token-account sizes
        // (used to size the rent read + the buy-template pool). The rent values
        // start at the placeholder and are re-read for real in `initialize`.
        let engine = Engine::new(
            Arc::clone(&config),
            jito_tip_account,
            protocol::TOKEN_ACCOUNT_SPACE,
            protocol::TOKEN_ACCOUNT_RENT_PLACEHOLDER,
            protocol::TOKEN_2022_ACCOUNT_SPACE,
            protocol::TOKEN_ACCOUNT_RENT_PLACEHOLDER,
        );

        // Duplicate the immutable shared handles onto the venue (same allocation).
        let rpc = Arc::clone(&engine.rpc);
        let http = engine.http.clone();

        Self {
            engine,
            config,
            http,
            rpc,
            global_account: None,
            amm_global_config_pda,
            amm_event_authority,
            amm_fee_config,
            amm_global_volume_accumulator,
            amm_user_volume_accumulator,
            amm_pool_cache: Arc::new(DashMap::new()),
            amm_global_config: Arc::new(std::sync::Mutex::new(None)),
            user_token_accounts: Arc::new(DashMap::new()),
            token_pdas: Arc::new(DashMap::new()),
            curve_routing_cache: Arc::new(DashMap::new()),
            reserve_cache: Arc::new(ReserveCache::default()),
            buy_pool_legacy: Arc::new(Mutex::new(Vec::new())),
            buy_pool_2022: Arc::new(Mutex::new(Vec::new())),
            buy_pool_misses_legacy: AtomicUsize::new(0),
            buy_pool_misses_2022: AtomicUsize::new(0),
            seed_counter: AtomicUsize::new(0),
        }
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
}
