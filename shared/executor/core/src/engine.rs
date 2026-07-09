// ============================================================
// Engine — the venue-AGNOSTIC send engine.
//
// This is the half of the former `PumpFunTrader` god-object that knows
// nothing about pump.fun: signer + rpc + http, the durable-nonce hash
// cache, the Jito tip cache + chosen tip account, the recent-blockhash
// cache, the pre-built compute-budget ix pairs, the wallet SOL-exposure
// ledger, and the background refresher task handles.
//
// A venue crate (e.g. `executor-pumpfun`) wraps an `Engine` and consumes
// its send/nonce/tip/confirm/sim helpers via `Deref`, adding the
// venue-specific caches + instruction builders on top. See the engine
// method files: send.rs, nonce.rs, jito_tip.rs, sim.rs.
// ============================================================

use crate::blockhash::BlockhashCache;
use crate::config::TraderConfig;
use crate::error::{Context, Result};
use crate::jito_tip::{refresh_tip_floor, JitoTipCache};
use dashmap::DashMap;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig, compute_budget::ComputeBudgetInstruction, hash::Hash,
    instruction::Instruction, pubkey::Pubkey,
};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;
use tracing::{info, warn};

/// Per-nonce-account slot held in the hash cache.
#[derive(Debug)]
pub struct NonceSlot {
    /// Most recently fetched blockhash for this nonce account.
    pub(crate) cached_hash: Option<Hash>,
    /// True while a transaction is in-flight using this slot.
    pub(crate) in_use: bool,
}

/// Venue-agnostic Solana send engine. Owns the signer/rpc/http, the nonce +
/// blockhash + tip caches, the pre-built compute-budget instructions, and the
/// per-process SOL-exposure ledger. Every field is `pub` so a wrapping venue
/// struct in another crate can read the shared handles directly.
pub struct Engine {
    pub config: Arc<TraderConfig>,
    pub http: reqwest::Client,
    pub rpc: Arc<RpcClient>,

    // Pre-built compute-budget instruction pairs (CU limit + price), built once
    // in `initialize()` and cloned per tx. Split by path because the priority fee
    // scales with the requested CU limit, and curve trades are far lighter than
    // AMM swaps — see `ComputeBudgetCfg`.
    pub cu_ixs_curve_buy: Vec<Instruction>,
    pub cu_ixs_curve_sell: Vec<Instruction>,
    pub cu_ixs_curve_create: Vec<Instruction>,
    pub cu_ixs_amm: Vec<Instruction>,

    // Jito tip: a fixed tip account (chosen once per instance) plus a
    // background-refreshed tip-floor cache that sizes the tip amount per trade.
    pub jito_tip_account: Pubkey,
    pub jito_tip_cache: Arc<JitoTipCache>,

    // Nonce management
    pub nonce_pubkeys: Vec<Pubkey>,
    pub nonce_cursor: AtomicUsize,
    // `std::sync::Mutex` (not tokio): every critical section is a synchronous
    // scan/write with no `.await` held, and this is the one lock every buy AND
    // sell contends on — a sync lock avoids the async-mutex scheduler overhead.
    pub nonce_slots: Arc<std::sync::Mutex<HashMap<Pubkey, NonceSlot>>>,
    /// Signalled each time a nonce slot is refreshed back to free.
    pub nonce_available: Arc<Notify>,

    // Diagnostic counters
    pub nonce_wait_events: AtomicUsize,
    pub nonce_wait_iters_total: AtomicUsize,

    // Rent values (fetched once in `initialize`). The token-account sizes are
    // venue-neutral SPL constants supplied by the wrapping venue at construction.
    pub token_account_space: u64,
    pub token_account_rent: u64,
    pub token_2022_account_space: u64,
    pub token_2022_account_rent: u64,

    // Background-refreshed recent blockhash for tx paths that can't use a durable
    // nonce (large multi-account swaps that would overflow the 1232-byte limit).
    pub blockhash_cache: Arc<BlockhashCache>,

    // --- SOL exposure tracking ---
    // Running sum of buy_amounts (lamports) for all real positions this process
    // has committed to but not yet closed, shared via the `Arc<...Trader>` so
    // strategies see the same open-commitment total.
    pub committed_lamports: AtomicU64,
    pub position_commitments: Arc<DashMap<String, u64>>,
    // Cached on-chain SOL balance (lamports, fetch instant), refreshed in the
    // background — never queried inline on the hot buy path.
    pub balance_lamports_cache: Arc<std::sync::Mutex<Option<(u64, std::time::Instant)>>>,

    // Background refresher tasks spawned in `initialize()` (Jito tip-floor +
    // recent-blockhash). Held so `Drop` on the wrapping trader can abort them.
    pub background_tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl Drop for Engine {
    /// Abort the `initialize()`-spawned refresher loops (Jito tip-floor +
    /// recent-blockhash) so a dropped trader leaves no orphaned RPC/Jito
    /// pollers behind. Harmless for the long-lived live-trading engine (dropped
    /// only at shutdown); load-bearing for a short-lived per-launch trader.
    fn drop(&mut self) {
        for task in &self.background_tasks {
            task.abort();
        }
    }
}

impl Engine {
    /// Construct the engine from a [`TraderConfig`] and the venue's SPL
    /// token-account sizes (used to size the rent read + the buy-template pool).
    /// Nothing touches the network here — call [`Engine::initialize`] once before
    /// trading to prime the caches and spawn the refreshers.
    pub fn new(
        config: Arc<TraderConfig>,
        jito_tip_account: Pubkey,
        token_account_space: u64,
        token_account_rent: u64,
        token_2022_account_space: u64,
        token_2022_account_rent: u64,
    ) -> Self {
        let rpc = Arc::new(RpcClient::new_with_commitment(
            config.rpc_url.clone(),
            CommitmentConfig::confirmed(),
        ));

        let jito_cfg = config.jito.clone();

        // Configured HTTP client (vs `Client::new()`): `tcp_nodelay` removes Nagle
        // delay on the small JSON-RPC sends, and a generous `pool_idle_timeout`
        // keeps keep-alive connections warm between trades. `initialize()` fires a
        // warmup request to seed the pool.
        let http = reqwest::Client::builder()
            .tcp_nodelay(true)
            .pool_idle_timeout(std::time::Duration::from_secs(90))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            config,
            http,
            rpc,
            cu_ixs_curve_buy: Vec::new(),
            cu_ixs_curve_sell: Vec::new(),
            cu_ixs_curve_create: Vec::new(),
            cu_ixs_amm: Vec::new(),
            jito_tip_account,
            jito_tip_cache: Arc::new(JitoTipCache::new(jito_cfg)),
            nonce_pubkeys: Vec::new(),
            nonce_cursor: AtomicUsize::new(0),
            nonce_slots: Arc::new(std::sync::Mutex::new(HashMap::new())),
            nonce_available: Arc::new(Notify::new()),
            nonce_wait_events: AtomicUsize::new(0),
            nonce_wait_iters_total: AtomicUsize::new(0),
            token_account_space,
            token_account_rent,
            token_2022_account_space,
            token_2022_account_rent,
            blockhash_cache: Arc::new(BlockhashCache::default()),
            committed_lamports: AtomicU64::new(0),
            position_commitments: Arc::new(DashMap::new()),
            balance_lamports_cache: Arc::new(std::sync::Mutex::new(None)),
            background_tasks: Vec::new(),
        }
    }

    /// Engine-side one-time initialization: connection warmup, rent-exemption
    /// reads, compute-budget ix building, the Jito tip-floor prime + background
    /// refresher, nonce parse + hash prefetch, and the recent-blockhash prime +
    /// background refresher. The wrapping venue calls this from its own
    /// `initialize`, then performs its venue steps (global-account fetch, seed
    /// pools). Pushes the spawned refresher handles into `background_tasks`.
    pub async fn initialize(&mut self) -> Result<()> {
        // 0. Connection warmup — seed the keep-alive pool so the FIRST trade
        // doesn't pay a TLS handshake on the latency-critical send.
        self.warmup_connections().await;

        // 1. Rent exemption amounts (one RPC call each).
        self.token_account_rent = self
            .rpc
            .get_minimum_balance_for_rent_exemption(self.token_account_space as usize)
            .await
            .context("Failed to get rent for token account")?;
        self.token_2022_account_rent = self
            .rpc
            .get_minimum_balance_for_rent_exemption(self.token_2022_account_space as usize)
            .await
            .context("Failed to get rent for token-2022 account")?;

        // 2. Compute budget instructions — built once, cloned per tx. The CU limit
        // is sized per path so the priority fee = price × limit isn't inflated by
        // sizing every tx for the heaviest path. Shared price ix, per-path limit.
        let compute = &self.config.compute;
        let price_ix =
            ComputeBudgetInstruction::set_compute_unit_price(compute.price_micro_lamports);
        let cu_ixs_curve_buy = vec![
            ComputeBudgetInstruction::set_compute_unit_limit(compute.curve_buy_cu),
            price_ix.clone(),
        ];
        let cu_ixs_curve_sell = vec![
            ComputeBudgetInstruction::set_compute_unit_limit(compute.curve_sell_cu),
            price_ix.clone(),
        ];
        let cu_ixs_curve_create = vec![
            ComputeBudgetInstruction::set_compute_unit_limit(compute.curve_create_cu),
            price_ix.clone(),
        ];
        let cu_ixs_amm = vec![
            ComputeBudgetInstruction::set_compute_unit_limit(compute.amm_cu),
            price_ix,
        ];
        info!(
            "⚡ Priority fee: {} µlamports/cu | CU limit (buy/sell/create/amm): {}/{}/{}/{}",
            compute.price_micro_lamports,
            compute.curve_buy_cu,
            compute.curve_sell_cu,
            compute.curve_create_cu,
            compute.amm_cu,
        );
        self.cu_ixs_curve_buy = cu_ixs_curve_buy;
        self.cu_ixs_curve_sell = cu_ixs_curve_sell;
        self.cu_ixs_curve_create = cu_ixs_curve_create;
        self.cu_ixs_amm = cu_ixs_amm;

        // 3. Jito tip — prime the tip-floor cache once, then refresh in background.
        if let Err(e) = refresh_tip_floor(&self.http, &self.jito_tip_cache).await {
            warn!("Initial Jito tip-floor fetch failed (using floor): {e}");
        }
        {
            let http = self.http.clone();
            let cache = self.jito_tip_cache.clone();
            let refresh_ms = self.config.jito.floor_refresh_ms;
            let handle = tokio::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_millis(refresh_ms)).await;
                    if let Err(e) = refresh_tip_floor(&http, &cache).await {
                        warn!("Jito tip-floor refresh failed: {e}");
                    }
                }
            });
            self.background_tasks.push(handle);
        }
        info!(
            "💸 Jito tip: dynamic — p{} of live tip-floor, clamped {}–{} SOL → {}",
            self.config.jito.percentile,
            self.config.jito.min_sol,
            self.config.jito.max_sol,
            self.jito_tip_account
        );

        // 4. Parse & deduplicate nonce accounts.
        self.collect_nonce_pubkeys()?;

        // 5. Pre-fetch all nonce hashes. Fetch each hash first (async), then take
        // the sync lock only for the inserts — a `std::sync::Mutex` guard must not
        // be held across `.await`.
        info!("🔧 Pre-fetching nonce hashes...");
        {
            let mut fetched = Vec::with_capacity(self.nonce_pubkeys.len());
            for &pubkey in &self.nonce_pubkeys {
                let hash = self.fetch_nonce_hash_async(&pubkey).await?;
                fetched.push((pubkey, hash));
            }
            let mut slots = self.nonce_slots.lock().unwrap_or_else(|p| p.into_inner());
            for (pubkey, hash) in fetched {
                slots.insert(
                    pubkey,
                    NonceSlot {
                        cached_hash: Some(hash),
                        in_use: false,
                    },
                );
            }
        }
        info!(
            "✅ Nonce hashes cached for {} account(s)",
            self.nonce_pubkeys.len()
        );

        // 6. Recent-blockhash refresher. Prime once, then refresh in background.
        match self.rpc.get_latest_blockhash().await {
            Ok(hash) => self.blockhash_cache.store(hash),
            Err(e) => warn!("Initial blockhash fetch failed: {e}"),
        }
        {
            let rpc = self.rpc.clone();
            let cache = self.blockhash_cache.clone();
            let refresh_ms = self.config.cache.blockhash_refresh_ms;
            let handle = tokio::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_millis(refresh_ms)).await;
                    match rpc.get_latest_blockhash().await {
                        Ok(hash) => cache.store(hash),
                        Err(e) => warn!("Blockhash refresh failed: {e}"),
                    }
                }
            });
            self.background_tasks.push(handle);
        }
        info!("✅ Blockhash refresher started");
        Ok(())
    }

    /// Best-effort TLS/keep-alive warmup: POST a cheap `getHealth` JSON-RPC to
    /// each Sender endpoint and the RPC URL concurrently, seeding the shared
    /// reqwest connection pool. All errors are swallowed — a down/slow endpoint
    /// at startup must never block or fail `initialize`.
    async fn warmup_connections(&self) {
        let mut targets: Vec<&str> = Vec::with_capacity(self.config.helius_sender_urls.len() + 1);
        for url in &self.config.helius_sender_urls {
            if !targets.contains(&url.as_str()) {
                targets.push(url);
            }
        }
        if !targets.contains(&self.config.rpc_url.as_str()) {
            targets.push(&self.config.rpc_url);
        }

        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getHealth",
        });

        let mut set = tokio::task::JoinSet::new();
        for url in targets {
            let http = self.http.clone();
            let url = url.to_string();
            let body = body.clone();
            set.spawn(async move {
                let _ = http.post(&url).json(&body).send().await;
            });
        }
        while set.join_next().await.is_some() {}
        info!("🔥 Connection pool warmed");
    }

    /// Parse & deduplicate the configured nonce accounts into `nonce_pubkeys`.
    fn collect_nonce_pubkeys(&mut self) -> Result<()> {
        use crate::error::bail;
        if self.config.nonce_accounts.is_empty() {
            bail!("At least one nonce account is required");
        }
        if self.config.limits.buy_seed_pool_size == 0 {
            bail!("buy_seed_pool_size must be >= 1");
        }
        let mut seen = HashSet::new();
        self.nonce_pubkeys.clear();
        for &pk in &self.config.nonce_accounts {
            if seen.insert(pk) {
                self.nonce_pubkeys.push(pk);
            }
        }
        info!(
            "✅ {} unique nonce account(s) configured",
            self.nonce_pubkeys.len()
        );
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Small accessors + the wallet SOL-exposure ledger (venue-neutral)
    // -----------------------------------------------------------------------

    /// Wallet public key for trade correlation.
    pub fn wallet_pubkey(&self) -> String {
        self.config.signer.pubkey().to_string()
    }

    /// Expose the RPC URL for callers that need to make their own RPC requests.
    pub fn rpc_url(&self) -> &str {
        &self.config.rpc_url
    }

    /// Reserve `buy_lamports` of SOL for `position_id`. Called before a real buy
    /// is sent; the reservation persists until `release_sol_for_position`.
    pub fn commit_sol_for_position(&self, position_id: String, buy_lamports: u64) {
        self.committed_lamports.fetch_add(buy_lamports, Ordering::Relaxed);
        self.position_commitments.insert(position_id, buy_lamports);
    }

    /// Release the reservation for `position_id`. No-op if never committed.
    pub fn release_sol_for_position(&self, position_id: &str) {
        if let Some((_, lamports)) = self.position_commitments.remove(position_id) {
            let _ = self.committed_lamports.fetch_update(
                Ordering::Relaxed,
                Ordering::Relaxed,
                |cur| Some(cur.saturating_sub(lamports)),
            );
        }
    }

    /// Total SOL committed (lamports) to open real positions.
    pub fn committed_lamports(&self) -> u64 {
        self.committed_lamports.load(Ordering::Relaxed)
    }

    /// Whether there is enough free SOL to commit `buy_lamports` more. Fails
    /// **open** (returns `true`) when the cached balance is unknown so a stale
    /// cache doesn't block all trades on startup.
    pub fn can_commit_buy(&self, buy_lamports: u64) -> bool {
        const RESERVE_FLOOR: u64 = 20_000_000; // 0.02 SOL — fees/tips/rent for the next sell.
        let balance = match *self
            .balance_lamports_cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
        {
            Some((lamports, _)) => lamports,
            None => return true,
        };
        let committed = self.committed_lamports.load(Ordering::Relaxed);
        balance
            .saturating_sub(RESERVE_FLOOR)
            .saturating_sub(committed)
            >= buy_lamports
    }

    /// Update the cached on-chain SOL balance. Called by the background refresh
    /// task every ~30 s — never on the hot buy path.
    pub fn update_balance_lamports_cache(&self, lamports: u64) {
        *self
            .balance_lamports_cache
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some((lamports, std::time::Instant::now()));
    }

    /// Validate a caller-supplied buy size against `config.limits.max_buy_sol` and
    /// convert to lamports. The crate's last-line spend guard — see the free
    /// [`buy_lamports_checked`] for the rejected cases.
    pub fn buy_lamports_checked(&self, sol_amount: f64) -> Result<u64> {
        buy_lamports_checked(sol_amount, self.config.limits.max_buy_sol)
    }
}

/// Validate a caller-supplied buy size in SOL and convert it to lamports. Pure
/// (free fn so the unit tests can drive it with an explicit ceiling); the method
/// [`Engine::buy_lamports_checked`] supplies `config.limits.max_buy_sol`.
///
/// Every value that would corrupt the real spend is rejected — the engine's
/// last-line guard, independent of any API check: non-finite (casts to garbage),
/// non-positive (wastes tip+fee), above `max_buy_sol` (a huge `f64` saturates the
/// cast), or rounds to 0 lamports.
pub fn buy_lamports_checked(sol_amount: f64, max_buy_sol: f64) -> Result<u64> {
    use crate::error::TradeError;
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
    let lamports = (sol_amount * crate::LAMPORTS_PER_SOL as f64) as u64;
    if lamports == 0 {
        return Err(TradeError::InvalidBuyAmount(format!(
            "{sol_amount} SOL rounds to 0 lamports"
        )));
    }
    Ok(lamports)
}

#[cfg(test)]
mod tests {
    use super::buy_lamports_checked;
    use crate::LAMPORTS_PER_SOL;

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
        assert!(buy_lamports_checked(1e30, MAX_BUY_SOL).is_err());
    }

    #[test]
    fn rejects_sub_dust_rounding_to_zero() {
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
