//! Tier 2 — operational tuning ([`TraderConfig`] + grouped sub-structs).
//!
//! Everything here is A/B-tuned per deployment: CU limits, Jito tip bounds,
//! retry/confirm counts, slippage, cache TTLs, the per-trade SOL ceiling. Each
//! sub-struct `impl Default` carries today's production values, so a consumer
//! constructs `TraderConfig { rpc_url, helius_sender_urls, signer, nonce_accounts,
//! ..Default::default() }` and overrides only the knobs it cares about — never
//! forking source to tune. Protocol invariants live in [`crate::protocol`];
//! per-call params (buy amount, slippage, tip level) stay function arguments.

use solana_sdk::{pubkey::Pubkey, signature::Signer};
use std::sync::Arc;

/// Compute-budget sizing — the priority fee is `price_micro_lamports × <limit>`
/// on every included tx, charged on the *requested* limit, so each path is sized
/// to its measured consumption (curve trades are far lighter than AMM swaps).
#[derive(Clone, Debug)]
pub struct ComputeBudgetCfg {
    /// Curve buy CU limit (may include create-with-seed + initialize + buy).
    pub curve_buy_cu: u32,
    /// Token `create` / `create_v2` CU limit (metadata / mayhem CPIs are heavy).
    pub curve_create_cu: u32,
    /// `create_v2` + dev-buy in one tx (create + ATA + buy).
    pub curve_create_buy_cu: u32,
    /// Curve sell CU limit (sell + tip; no account creation).
    pub curve_sell_cu: u32,
    /// PumpSwap AMM swap CU limit (heaviest path — many accounts/CPIs).
    pub amm_cu: u32,
    /// Compute-unit price in micro-lamports (the priority-fee rate per CU).
    pub price_micro_lamports: u64,
}

impl Default for ComputeBudgetCfg {
    fn default() -> Self {
        Self {
            curve_buy_cu: 150_000,
            curve_create_cu: 250_000,
            curve_create_buy_cu: 350_000,
            curve_sell_cu: 100_000,
            amm_cu: 180_000,
            price_micro_lamports: 200_000,
        }
    }
}

/// Dynamic Jito tip sizing — the tip is sized per-trade from Jito's live
/// tip-floor feed, clamped to `[min_sol, max_sol]`, and escalates per sell retry.
#[derive(Clone, Debug)]
pub struct JitoTipCfg {
    /// Floor for the tip, in SOL — also the fallback when the feed is cold/stale.
    pub min_sol: f64,
    /// Ceiling for the tip, in SOL — the hard per-trade cost guardrail.
    pub max_sol: f64,
    /// Landed-tip percentile to target for the first attempt (25|50|75|95|99).
    pub percentile: u8,
    /// Per-retry tail multiplier once the ladder climbs past p99 (or from the
    /// floor when the feed is cold).
    pub escalation_tail_mult: f64,
    /// Jito tip-floor REST feed (landed-tip percentiles; values in SOL).
    pub floor_url: String,
    /// How often to re-fetch the tip-floor feed, in milliseconds.
    pub floor_refresh_ms: u64,
    /// Max age of a cached tip-floor read before falling back to the floor (ms).
    pub floor_max_age_ms: u64,
}

impl Default for JitoTipCfg {
    fn default() -> Self {
        Self {
            min_sol: 0.0002,
            max_sol: 0.005,
            percentile: 75,
            escalation_tail_mult: 1.5,
            floor_url: "https://bundles.jito.wtf/api/v1/bundles/tip_floor".to_string(),
            floor_refresh_ms: 3_000,
            floor_max_age_ms: 30_000,
        }
    }
}

/// Sell retry + confirmation polling counts.
#[derive(Clone, Debug)]
pub struct RetryCfg {
    /// How many times `sell_token` retries before giving up.
    pub max_sell_attempts: usize,
    /// How many times we poll for transaction confirmation.
    pub confirm_max_retries: usize,
    /// Fallback/steady-state gap between confirmation polls, in ms (used once the
    /// ramp below is exhausted).
    pub confirm_poll_ms: u64,
    /// Ramped gaps between the first confirmation polls, in ms; polls beyond this
    /// list fall back to `confirm_poll_ms`.
    pub confirm_poll_schedule_ms: Vec<u64>,
}

impl Default for RetryCfg {
    fn default() -> Self {
        Self {
            max_sell_attempts: 5,
            confirm_max_retries: 5,
            confirm_poll_ms: 1_000,
            confirm_poll_schedule_ms: vec![250, 400, 700, 1_000],
        }
    }
}

/// Durable-nonce acquisition + background-refresh tuning.
#[derive(Clone, Debug)]
pub struct NonceCfg {
    /// Maximum spin-wait iterations when all nonce slots are in use.
    pub max_wait_iters: usize,
    /// Sleep between nonce spin-wait iterations, in milliseconds.
    pub wait_sleep_ms: u64,
    /// After a send, how many times the refresh re-reads the nonce account
    /// waiting for its blockhash to advance before re-arming with the last read.
    pub refresh_max_attempts: usize,
    /// Delay between nonce-refresh re-reads, in milliseconds.
    pub refresh_retry_ms: u64,
}

impl Default for NonceCfg {
    fn default() -> Self {
        Self {
            max_wait_iters: 200,
            wait_sleep_ms: 20,
            refresh_max_attempts: 8,
            refresh_retry_ms: 150,
        }
    }
}

/// Freshness bounds for the WS-fed reserve cache and the recent-blockhash cache.
#[derive(Clone, Debug)]
pub struct CacheCfg {
    /// Max age of a WS-fed reserve snapshot still trusted on the trade path (ms).
    pub reserve_max_age_ms: u64,
    /// Background refresh interval for the recent-blockhash cache (ms).
    pub blockhash_refresh_ms: u64,
    /// Max age of a cached recent blockhash still used on the AMM buy path (ms).
    pub blockhash_max_age_ms: u64,
}

impl Default for CacheCfg {
    fn default() -> Self {
        Self {
            reserve_max_age_ms: 3_000,
            blockhash_refresh_ms: 2_000,
            blockhash_max_age_ms: 10_000,
        }
    }
}

/// Slippage defaults applied when a caller doesn't pass a per-call tolerance.
#[derive(Clone, Debug)]
pub struct SlippageCfg {
    /// Default AMM slippage tolerance, in basis points (500 = 5%).
    pub amm_default_bps: u64,
    /// Conservative fee allowance (bps) subtracted from the bonding-curve quote
    /// before applying slippage, when computing a curve buy/sell `min_out`.
    pub curve_fee_buffer_bps: u128,
}

impl Default for SlippageCfg {
    fn default() -> Self {
        Self {
            amm_default_bps: 500,
            curve_fee_buffer_bps: 200,
        }
    }
}

/// Hard backstops independent of any API-layer policy.
#[derive(Clone, Debug)]
pub struct LimitsCfg {
    /// Hard per-trade sanity ceiling on a buy, in SOL — a fat-finger/hostile-input
    /// backstop, deliberately generous (the business limit lives in the API).
    pub max_buy_sol: f64,
    /// How many buy templates to keep pre-built per token-program pool.
    pub buy_seed_pool_size: usize,
}

impl Default for LimitsCfg {
    fn default() -> Self {
        Self {
            max_buy_sol: 5.0,
            buy_seed_pool_size: 16,
        }
    }
}

/// Configuration for a [`crate::PumpFunTrader`]. The four required fields have no
/// `Default`; the seven tuning sub-structs each carry today's production values,
/// so a consumer overrides only what it needs:
///
/// ```ignore
/// let config = TraderConfig {
///     rpc_url, helius_sender_urls, signer: Arc::new(my_signer), nonce_accounts,
///     compute: ComputeBudgetCfg { amm_cu: 220_000, ..Default::default() },
///     ..Default::default()
/// };
/// ```
pub struct TraderConfig {
    // --- required, no Default ---
    pub rpc_url: String,
    /// One or more Helius Sender endpoints. A transaction is fanned out to all of
    /// them concurrently (same signature → on-chain dedup, Jito tip paid once);
    /// a single entry is the plain single-endpoint path. Must be non-empty.
    pub helius_sender_urls: Vec<String>,
    /// The trade signer. An `Arc<dyn Signer>` (not a raw `Keypair`) so a consumer
    /// can plug in an HSM / remote signer instead of handing over a private key.
    pub signer: Arc<dyn Signer + Send + Sync>,
    /// Durable-nonce accounts (parsed pubkeys; deduplicated at init).
    pub nonce_accounts: Vec<Pubkey>,
    /// Optional on-chain Address Lookup Table for the launch (create + dev-buy)
    /// path. When set, the venue fetches it once at init and compiles the create
    /// tx as a v0 (versioned) message so the ~27 accounts of a create_v2 + dev-buy
    /// reference the ~15 immutable ones by 1-byte index instead of 32-byte keys —
    /// dropping the tx from ~1267 B (over the 1232 B limit) to ~840 B. `None`
    /// keeps the legacy single-message path unchanged (what hunter uses).
    pub launch_alt_address: Option<Pubkey>,
    // --- tuning, all Default ---
    pub compute: ComputeBudgetCfg,
    pub jito: JitoTipCfg,
    pub retry: RetryCfg,
    pub nonce: NonceCfg,
    pub cache: CacheCfg,
    pub slippage: SlippageCfg,
    pub limits: LimitsCfg,
}

impl TraderConfig {
    /// Build a config from the four required fields with every tuning sub-struct
    /// at its [`Default`]. Override individual sub-structs afterward (or use a
    /// struct literal with `..Default::default()` on the sub-structs) to tune.
    pub fn new(
        rpc_url: String,
        helius_sender_urls: Vec<String>,
        signer: Arc<dyn Signer + Send + Sync>,
        nonce_accounts: Vec<Pubkey>,
    ) -> Self {
        Self {
            rpc_url,
            helius_sender_urls,
            signer,
            nonce_accounts,
            launch_alt_address: None,
            compute: ComputeBudgetCfg::default(),
            jito: JitoTipCfg::default(),
            retry: RetryCfg::default(),
            nonce: NonceCfg::default(),
            cache: CacheCfg::default(),
            slippage: SlippageCfg::default(),
            limits: LimitsCfg::default(),
        }
    }
}

impl std::fmt::Debug for TraderConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The signer is deliberately omitted (it may wrap a private key / HSM).
        f.debug_struct("TraderConfig")
            .field("rpc_url", &self.rpc_url)
            .field("helius_sender_urls", &self.helius_sender_urls)
            .field("wallet", &self.signer.pubkey())
            .field("nonce_accounts", &self.nonce_accounts)
            .field("launch_alt_address", &self.launch_alt_address)
            .field("compute", &self.compute)
            .field("jito", &self.jito)
            .field("retry", &self.retry)
            .field("nonce", &self.nonce)
            .field("cache", &self.cache)
            .field("slippage", &self.slippage)
            .field("limits", &self.limits)
            .finish()
    }
}
