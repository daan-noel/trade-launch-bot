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
//  - Retry loop on sell (up to 5 attempts, fresh nonce each time);
//    `sell_token_once` exposes a single attempt for callers that own
//    their own retry loop.
//  - Background pool refill immediately after a buy consumes a
//    template, so the next buy starts warm.
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
mod consolidate;
#[cfg(feature = "claim")]
pub mod claim;
mod init;
mod jito_tip;
mod nonce;
mod pool;
#[cfg(feature = "probe")]
pub mod probe;
mod query;
mod reserves;
mod sell;
pub mod sim;
mod swap_retry;
mod tx;

use blockhash::BlockhashCache;
use dashmap::DashMap;
use jito_tip::JitoTipCache;
use reserves::ReserveCache;

pub use buy::BuySignedHook;
pub use nonce::NonceAuthCheck;
pub use sim::{AccountDelta, SimOutcome};
pub use swap_retry::{
    classify_swap_revert, SwapDirection, SwapRetryDecision, SwapRoute, AMM_BUY_SLIPPAGE_BELOW_MIN_BASE,
    AMM_EXCEEDED_SLIPPAGE, ANCHOR_CONSTRAINT_SEEDS, BONDING_CURVE_COMPLETE,
    CURVE_BUY_SLIPPAGE_BELOW_MIN_TOKENS_OUT, CURVE_MISSING_USER_VOLUME_ACCUMULATOR,
    CURVE_TOO_LITTLE_SOL_RECEIVED, CURVE_TOO_MUCH_SOL_REQUIRED,
};
pub use tx::SigStatus;

// `TraderConfig` lives in `crate::config` now; re-export it here so the existing
// `crate::trader::TraderConfig` paths (incl. unit tests) keep resolving.
pub(crate) use crate::config::TraderConfig;
use crate::error::{Result, TradeError};
use crate::protocol::{
    self, LAMPORTS_PER_SOL, TOKEN_2022_ACCOUNT_SPACE, TOKEN_ACCOUNT_RENT_PLACEHOLDER,
    TOKEN_ACCOUNT_SPACE,
};
use rand::seq::SliceRandom;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig, hash::Hash, instruction::Instruction, pubkey::Pubkey,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::{Mutex, Notify};

/// Validate a caller-supplied buy size in SOL and convert it to lamports. Pure
/// (free fn so the unit tests can drive it with an explicit ceiling); the method
/// [`PumpFunTrader::buy_lamports_checked`] supplies `config.limits.max_buy_sol`.
///
/// The public buy entry points (`buy_token`, `buy_token_snipe`, `amm_buy`) take
/// an `f64` straight from the caller, so every value that would corrupt the real
/// spend is rejected — the crate's last-line guard, independent of any API check:
/// non-finite (casts to garbage), non-positive (wastes tip+fee), above
/// `max_buy_sol` (a huge `f64` saturates the cast), or rounds to 0 lamports.
fn buy_lamports_checked(sol_amount: f64, max_buy_sol: f64) -> Result<u64> {
    if !sol_amount.is_finite() || sol_amount <= 0.0 {
        return Err(TradeError::InvalidBuyAmount(format!(
            "{sol_amount} SOL: must be finite and > 0"
        )));
    }
    if sol_amount > max_buy_sol {
        return Err(TradeError::InvalidBuyAmount(format!(
            "{sol_amount} SOL exceeds the {max_buy_sol} SOL sanity ceiling"
        )));
    }
    let lamports = (sol_amount * LAMPORTS_PER_SOL as f64) as u64;
    if lamports == 0 {
        return Err(TradeError::InvalidBuyAmount(format!(
            "{sol_amount} SOL rounds to 0 lamports"
        )));
    }
    Ok(lamports)
}

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
    config: Arc<TraderConfig>,
    http: reqwest::Client,
    rpc: Arc<RpcClient>,

    // Set once in initialize()
    global_account: Option<GlobalAccount>,
    // Pre-built compute-budget instruction pairs (CU limit + price), built once
    // at init and cloned per tx. Split by path because the priority fee scales
    // with the requested CU limit, and curve trades are far lighter than AMM
    // swaps — see constants::COMPUTE_UNIT_LIMIT_*.
    cu_ixs_curve_buy: Vec<Instruction>,
    cu_ixs_curve_sell: Vec<Instruction>,
    cu_ixs_amm: Vec<Instruction>,
    // Jito tip: a fixed tip account (chosen once per instance) plus a
    // background-refreshed tip-floor cache that sizes the tip amount per trade.
    // See jito_tip.rs.
    jito_tip_account: Pubkey,
    jito_tip_cache: Arc<JitoTipCache>,

    // Nonce management
    nonce_pubkeys: Vec<Pubkey>,
    nonce_cursor: AtomicUsize,
    // `std::sync::Mutex` (not tokio): every `acquire_nonce` / `schedule_nonce_
    // refresh` critical section is a synchronous scan/write with no `.await`
    // held, and this is the one lock every buy AND sell contends on — a sync lock
    // avoids the async-mutex scheduler overhead for a section that never yields.
    // The `notified()` wait stays outside the lock (see `acquire_nonce`). Matches
    // `JitoTipCache`/`BlockhashCache`, which are already `std::sync::Mutex`.
    nonce_slots: Arc<std::sync::Mutex<HashMap<Pubkey, NonceSlot>>>,
    /// Signalled each time a nonce slot is refreshed back to free, so a waiting
    /// `acquire_nonce` wakes immediately instead of polling on a fixed sleep.
    nonce_available: Arc<Notify>,

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

    // PumpSwap AMM (migrated tokens)
    // Program-constant PumpSwap PDAs, derived once in `new()` (they never depend
    // on the token, only on the program / fee-program / this wallet) instead of
    // re-running `find_program_address` on every AMM swap.
    amm_global_config_pda: Pubkey,
    amm_event_authority: Pubkey,
    amm_fee_config: Pubkey,
    amm_global_volume_accumulator: Pubkey,
    amm_user_volume_accumulator: Pubkey,
    // Sharded concurrent maps (`DashMap`): each critical section is a single
    // get/insert with no `.await` held, and sharding means the WS-ingest writer
    // and the trade readers no longer serialize on one global mutex.
    amm_pool_cache: Arc<DashMap<String, AmmPoolInfo>>,
    // GlobalConfig + the instant it was fetched. Governance-mutable fee bps mean
    // a stale cache would silently loosen slippage protection forever; the fetch
    // instant lets `amm_config` refresh past a max-age (every other cache is
    // freshness-bounded — this one used to be cached for the process lifetime).
    amm_global_config: Arc<std::sync::Mutex<Option<(AmmGlobalConfig, std::time::Instant)>>>,

    // Per-token caches.
    user_token_accounts: Arc<DashMap<String, Pubkey>>,
    token_pdas: Arc<DashMap<String, TokenPDAs>>,
    // Per-mint bonding-curve routing facts (creator / token program / cashback /
    // migration). Served without RPC once a mint is observed migrated — see
    // `read_curve_routing` for the monotonic-migration invariant this relies on.
    curve_routing_cache: Arc<DashMap<String, query::CurveRouting>>,

    // WS-fed live reserve snapshots (mint → latest post-trade reserves), read on
    // the slippage / AMM-reserve hot path with an on-chain fallback.
    reserve_cache: Arc<ReserveCache>,

    // Background-refreshed recent blockhash for the AMM buy path (which can't use
    // a durable nonce — see `build_recent_tx`).
    blockhash_cache: Arc<BlockhashCache>,

    // --- SOL exposure tracking ---
    // Running sum of buy_amounts (in lamports) for all real positions this process
    // has committed to but not yet closed. Both strategies share this single counter
    // via the Arc<PumpFunTrader> so neither sees only half the open commitments.
    // Per-position lamports keyed by position_id (as String) for clean release.
    committed_lamports: AtomicU64,
    position_commitments: Arc<DashMap<String, u64>>,
    // Cached on-chain SOL balance of the wallet (lamports, fetch instant). Updated
    // by a background task every ~30s — never queried inline on the hot buy path.
    // `None` until the first refresh completes; `can_commit_buy` fails open (allows)
    // when the cache is empty so a stale cache doesn't block all trades.
    balance_lamports_cache: Arc<std::sync::Mutex<Option<(u64, std::time::Instant)>>>,
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

        let wallet = config.signer.pubkey();
        // Clone the Jito tuning out before `config` is moved into the struct below
        // (the tip cache owns its own copy to size tips without touching config).
        let jito_cfg = config.jito.clone();

        // PumpSwap PDAs that never change for this wallet — derived once here so
        // `amm_swap_accounts` is a few field reads instead of ~5 `find_program_address`.
        let amm_global_config_pda =
            Pubkey::find_program_address(&[b"global_config"], &protocol::PUMP_SWAP).0;
        let amm_event_authority =
            Pubkey::find_program_address(&[b"__event_authority"], &protocol::PUMP_SWAP).0;
        let amm_fee_config =
            Pubkey::find_program_address(&[b"fee_config", protocol::PUMP_SWAP.as_ref()], &protocol::FEE_PROGRAM).0;
        let amm_global_volume_accumulator =
            Pubkey::find_program_address(&[b"global_volume_accumulator"], &protocol::PUMP_SWAP).0;
        let amm_user_volume_accumulator = Pubkey::find_program_address(
            &[b"user_volume_accumulator", wallet.as_ref()],
            &protocol::PUMP_SWAP,
        )
        .0;

        // Jito tip account — one chosen at random per trader instance from the
        // `const [Pubkey; 10]`; the tip *amount* is sized per trade (jito_tip.rs).
        let jito_tip_account = protocol::JITO_TIP_ACCOUNTS
            .choose(&mut rand::thread_rng())
            .copied()
            .expect("JITO_TIP_ACCOUNTS is a non-empty const array");

        // Configured HTTP client (vs `Client::new()`): `tcp_nodelay` removes
        // Nagle delay on the small JSON-RPC sends, and a generous
        // `pool_idle_timeout` keeps the keep-alive connections to the senders/RPC
        // warm between trades so a quiet period doesn't drop the pooled TLS
        // session and re-pay a handshake on the next send. `initialize()` also
        // fires a warmup request to seed the pool.
        let http = reqwest::Client::builder()
            .tcp_nodelay(true)
            .pool_idle_timeout(std::time::Duration::from_secs(90))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            config,
            http,
            rpc,
            global_account: None,
            cu_ixs_curve_buy: Vec::new(),
            cu_ixs_curve_sell: Vec::new(),
            cu_ixs_amm: Vec::new(),
            jito_tip_account,
            jito_tip_cache: Arc::new(JitoTipCache::new(jito_cfg)),
            nonce_pubkeys: Vec::new(),
            nonce_cursor: AtomicUsize::new(0),
            nonce_slots: Arc::new(std::sync::Mutex::new(HashMap::new())),
            nonce_available: Arc::new(Notify::new()),
            nonce_wait_events: AtomicUsize::new(0),
            nonce_wait_iters_total: AtomicUsize::new(0),
            buy_pool_legacy: Arc::new(Mutex::new(Vec::new())),
            buy_pool_2022: Arc::new(Mutex::new(Vec::new())),
            buy_pool_misses_legacy: AtomicUsize::new(0),
            buy_pool_misses_2022: AtomicUsize::new(0),
            seed_counter: AtomicUsize::new(0),
            token_account_space: TOKEN_ACCOUNT_SPACE,
            token_account_rent: TOKEN_ACCOUNT_RENT_PLACEHOLDER,
            token_2022_account_space: TOKEN_2022_ACCOUNT_SPACE,
            token_2022_account_rent: TOKEN_ACCOUNT_RENT_PLACEHOLDER,
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
            blockhash_cache: Arc::new(BlockhashCache::default()),
            committed_lamports: AtomicU64::new(0),
            position_commitments: Arc::new(DashMap::new()),
            balance_lamports_cache: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    /// Wallet public key for trade correlation.
    pub fn wallet_pubkey(&self) -> String {
        self.config.signer.pubkey().to_string()
    }

    /// Validate a caller-supplied buy size against `config.limits.max_buy_sol` and
    /// convert to lamports. The crate's last-line spend guard — see the free
    /// [`buy_lamports_checked`] for the rejected cases.
    pub(super) fn buy_lamports_checked(&self, sol_amount: f64) -> Result<u64> {
        buy_lamports_checked(sol_amount, self.config.limits.max_buy_sol)
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

    // --- SOL exposure API ---

    /// Reserve `buy_lamports` of SOL for `position_id`. Called before a real buy is
    /// sent; the reservation persists until `release_sol_for_position` is called (on
    /// close or on buy failure). Both strategies share the same atomic counter via the
    /// shared `Arc<PumpFunTrader>` so neither sees only half the open commitments.
    pub fn commit_sol_for_position(&self, position_id: String, buy_lamports: u64) {
        self.committed_lamports.fetch_add(buy_lamports, Ordering::Relaxed);
        self.position_commitments.insert(position_id, buy_lamports);
    }

    /// Release the reservation for `position_id`. No-op if the position was never
    /// committed (e.g. paper positions, or a double-release on an already-released id).
    pub fn release_sol_for_position(&self, position_id: &str) {
        if let Some((_, lamports)) = self.position_commitments.remove(position_id) {
            // Saturating sub: a race between two release calls can't underflow.
            let _ = self.committed_lamports.fetch_update(
                Ordering::Relaxed,
                Ordering::Relaxed,
                |cur| Some(cur.saturating_sub(lamports)),
            );
        }
    }

    /// Total SOL committed (lamports) to open real positions across both strategies.
    pub fn committed_lamports(&self) -> u64 {
        self.committed_lamports.load(Ordering::Relaxed)
    }

    /// Whether there is enough free SOL to commit `buy_lamports` more. Checks:
    ///   `cached_balance - RESERVE_FLOOR (0.02 SOL) - committed >= buy_lamports`
    /// Fails **open** (returns `true`) when the cached balance is unknown so a stale
    /// cache doesn't block all trades on startup. The on-chain transaction is the
    /// ultimate safety net; this is a soft pre-send gate.
    pub fn can_commit_buy(&self, buy_lamports: u64) -> bool {
        // 0.02 SOL reserve — enough for fees/tips/rent on the next sell.
        const RESERVE_FLOOR: u64 = 20_000_000;
        let balance = match *self.balance_lamports_cache.lock().unwrap_or_else(|p| p.into_inner()) {
            Some((lamports, _)) => lamports,
            None => return true, // cache empty → fail open
        };
        let committed = self.committed_lamports.load(Ordering::Relaxed);
        balance
            .saturating_sub(RESERVE_FLOOR)
            .saturating_sub(committed)
            >= buy_lamports
    }

    /// Update the cached on-chain SOL balance. Called by the background refresh task
    /// every ~30 s — never on the hot buy path.
    pub fn update_balance_lamports_cache(&self, lamports: u64) {
        *self.balance_lamports_cache.lock().unwrap_or_else(|p| p.into_inner()) =
            Some((lamports, std::time::Instant::now()));
    }
}

#[cfg(test)]
mod tests {
    use super::{buy_lamports_checked, LAMPORTS_PER_SOL};

    /// The default `config.limits.max_buy_sol` the live trader uses.
    const MAX_BUY_SOL: f64 = 5.0;

    #[test]
    fn rejects_non_finite_and_non_positive() {
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -1.0] {
            assert!(
                buy_lamports_checked(bad, MAX_BUY_SOL).is_err(),
                "{bad} should be rejected"
            );
        }
    }

    #[test]
    fn rejects_above_ceiling() {
        assert!(buy_lamports_checked(MAX_BUY_SOL + 1.0, MAX_BUY_SOL).is_err());
        // A huge value that would otherwise saturate the lamports cast.
        assert!(buy_lamports_checked(1e30, MAX_BUY_SOL).is_err());
    }

    #[test]
    fn rejects_sub_dust_rounding_to_zero() {
        // Positive but far below one lamport → casts to 0 lamports.
        assert!(buy_lamports_checked(1e-12, MAX_BUY_SOL).is_err());
    }

    #[test]
    fn accepts_valid_amounts() {
        assert_eq!(buy_lamports_checked(1.0, MAX_BUY_SOL).unwrap(), LAMPORTS_PER_SOL);
        assert_eq!(buy_lamports_checked(0.5, MAX_BUY_SOL).unwrap(), LAMPORTS_PER_SOL / 2);
        assert_eq!(
            buy_lamports_checked(MAX_BUY_SOL, MAX_BUY_SOL).unwrap(),
            MAX_BUY_SOL as u64 * LAMPORTS_PER_SOL
        );
    }
}
