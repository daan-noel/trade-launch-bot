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
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;
use tracing::{info, warn};

/// SOL kept back from every buy for the fees, tips and rent of the next sell.
pub const RESERVE_FLOOR_LAMPORTS: u64 = 20_000_000;

/// A slotless balance reading (an RPC poll at `confirmed`) covers a landed buy
/// only when it was taken this long after the buy was seen at `processed`.
const POLL_COVER_LAG: Duration = Duration::from_secs(5);

/// A cached wallet balance and where it came from: `slot` is the slot of a
/// pushed account update, `None` for an RPC poll.
#[derive(Debug, Clone, Copy)]
pub struct BalanceReading {
    pub lamports: u64,
    pub at: Instant,
    pub slot: Option<u64>,
}

/// One buy's SOL earmark. While the buy is in flight its SOL is still in the
/// wallet balance, so the earmark holds it back from the next buy. Once the buy
/// lands (`settled`) the balance itself drops by it; the earmark stays only until
/// a balance reading that already includes the buy, so the SOL is never counted
/// as both free and spent, nor spent twice.
#[derive(Debug, Clone, Copy)]
pub struct SolReservation {
    pub lamports: u64,
    pub settled: Option<Settled>,
}

/// When a reserved buy was seen landing: its slot when known, and the instant.
#[derive(Debug, Clone, Copy)]
pub struct Settled {
    pub slot: Option<u64>,
    pub at: Instant,
}

impl SolReservation {
    /// Whether this earmark still holds SOL back, given the cached balance.
    fn holds(&self, balance: Option<&BalanceReading>) -> bool {
        let Some(settled) = self.settled else {
            return true;
        };
        let Some(reading) = balance else {
            return true;
        };
        match (settled.slot, reading.slot) {
            (Some(landed), Some(read)) => read < landed,
            _ => reading.at < settled.at + POLL_COVER_LAG,
        }
    }
}

/// Per-nonce-account slot held in the hash cache.
#[derive(Debug)]
pub struct NonceSlot {
    /// Most recently fetched blockhash for this nonce account.
    pub(crate) cached_hash: Option<Hash>,
    /// True while a transaction is in-flight using this slot.
    pub(crate) in_use: bool,
    /// Bumped on every `acquire_nonce`. Lets the post-send refresh poll detect
    /// that the slot was already re-armed (by the push feed) and re-acquired —
    /// finalizing then would free a slot another tx is using.
    pub(crate) use_epoch: u64,
    /// Highest slot seen on the push path (`on_nonce_account_update`) — gates
    /// out replayed/out-of-order account updates on reconnect.
    pub(crate) last_push_slot: u64,
}

/// Venue-agnostic Solana send engine. Owns the signer/rpc/http, the nonce +
/// blockhash + tip caches, the pre-built compute-budget instructions, and the
/// per-process SOL-exposure ledger. Every field is `pub` so a wrapping venue
/// struct in another crate can read the shared handles directly.
pub struct Engine {
    pub config: Arc<TraderConfig>,
    pub http: reqwest::Client,
    pub rpc: Arc<RpcClient>,

    // Pre-built create compute-budget pair (CU limit + price), built once in
    // `initialize()`. The single-tx buy/sell/AMM pairs are built per tx by the
    // venue (`FeeShapeCfg` may vary them), from `ComputeBudgetCfg`.
    pub cu_ixs_curve_create: Vec<Instruction>,

    // Jito tip: a fixed tip account (chosen once per instance) plus a
    // background-refreshed tip-floor cache that sizes the tip amount per trade.
    // `jito_tip_account` is used ONLY on the Jito block-engine bundle path
    // (`bundle_buy`/`create`), which requires a Jito tip account. The single-tx
    // buy/sell tip (`jito_tip_ix`) submits through the Helius `/fast` sender, which
    // requires one of Helius's OWN tip wallets and rejects a Jito account
    // (`-32602`), so it uses `sender_tip_account` instead — the two must stay
    // distinct (see `protocol::HELIUS_SENDER_TIP_ACCOUNTS`).
    pub jito_tip_account: Pubkey,
    pub sender_tip_account: Pubkey,
    /// Every valid Sender tip wallet; `sender_tip` picks from it per tx when
    /// fee shaping is on (`sender_tip_account` is one member of it).
    pub sender_tip_pool: &'static [Pubkey],
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

    /// `signature -> (nonce_account, nonce_hash)` for durable-nonce txs we sent.
    ///
    /// A durable-nonce tx does **not** expire: if it never lands, it stays
    /// executable until that nonce account advances. `schedule_nonce_refresh`
    /// even re-arms the slot with the *same* hash when the send didn't land, so
    /// the tx can still execute later. That makes "resend the sell" unsafe in
    /// general — two sells could both land and the second would eat a sibling
    /// position's bag out of a shared token account.
    ///
    /// This index is what makes recovery decidable: with it,
    /// [`Engine::nonce_tx_state`] can prove a tx is permanently dead (nonce moved
    /// on) and [`Engine::burn_nonce_tx`] can *make* it dead. FIFO-bounded because
    /// it is only consulted on the cold recovery path.
    pub nonce_tx_index: Arc<DashMap<String, (Pubkey, Hash)>>,
    pub(crate) nonce_tx_order: Arc<std::sync::Mutex<VecDeque<String>>>,
    /// The signature of a sell signed for a key (the mint) whose send has not
    /// returned success. A send can fail or time out AFTER the tx reached the
    /// network, and a durable-nonce tx never expires, so the caller must learn
    /// this signature before it sends another sell (see [`Engine::take_signed_sell`]).
    signed_sells: Arc<DashMap<String, String>>,

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
    // One earmark per buy whose SOL the cached wallet balance does not show as
    // spent yet. A buy that landed is already out of the balance, so its earmark
    // lasts only until a balance reading taken after it (see `SolReservation`).
    pub position_commitments: Arc<DashMap<String, SolReservation>>,
    // Serializes check-and-reserve, so two buys never both pass on the same SOL.
    reserve_lock: Arc<std::sync::Mutex<()>>,
    // Newest slot of a pushed balance; an older push is dropped.
    balance_push_slot: AtomicU64,
    // Cached on-chain SOL balance, refreshed in the background (push or poll) —
    // never queried inline on the hot buy path.
    pub balance_lamports_cache: Arc<std::sync::Mutex<Option<BalanceReading>>>,

    // Background tasks spawned in `initialize()` (Jito tip-floor, recent-blockhash,
    // Sender keep-warm). Held so `Drop` on the wrapping trader can abort them.
    pub background_tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl Drop for Engine {
    /// Abort the `initialize()`-spawned loops (Jito tip-floor, recent-blockhash,
    /// Sender keep-warm) so a dropped trader leaves no orphaned RPC/Jito/Sender
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
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: Arc<TraderConfig>,
        jito_tip_account: Pubkey,
        sender_tip_account: Pubkey,
        sender_tip_pool: &'static [Pubkey],
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
        // delay on the small JSON-RPC sends, and the pool keeps keep-alive
        // connections between trades. `initialize()` fires a warmup request to
        // seed the pool.
        //
        // A cold pool costs the hot-path POST DNS + a TCP (+TLS) handshake, so the
        // client-side idle timeout must outlast the gap between trades — a
        // *selective* rule fires only a few times an hour. The client never closes
        // first; the SERVER does: a Helius Sender drops an idle socket after
        // `SENDER_IDLE_CLOSE_MS`. The regional Sender URLs are `http://`, which
        // reqwest speaks as HTTP/1.1, so the HTTP/2 PING settings below are inert
        // there and apply only to an endpoint that negotiates HTTP/2. What holds a
        // Sender socket open is the keep-warm ping loop (`SenderCfg`).
        let http = reqwest::Client::builder()
            .tcp_nodelay(true)
            .pool_idle_timeout(std::time::Duration::from_secs(600))
            .http2_keep_alive_interval(std::time::Duration::from_secs(20))
            .http2_keep_alive_timeout(std::time::Duration::from_secs(10))
            .http2_keep_alive_while_idle(true)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            config,
            http,
            rpc,
            cu_ixs_curve_create: Vec::new(),
            jito_tip_account,
            sender_tip_account,
            sender_tip_pool,
            jito_tip_cache: Arc::new(JitoTipCache::new(jito_cfg)),
            nonce_pubkeys: Vec::new(),
            nonce_cursor: AtomicUsize::new(0),
            nonce_slots: Arc::new(std::sync::Mutex::new(HashMap::new())),
            nonce_available: Arc::new(Notify::new()),
            nonce_tx_index: Arc::new(DashMap::new()),
            nonce_tx_order: Arc::new(std::sync::Mutex::new(VecDeque::new())),
            signed_sells: Arc::new(DashMap::new()),
            nonce_wait_events: AtomicUsize::new(0),
            nonce_wait_iters_total: AtomicUsize::new(0),
            token_account_space,
            token_account_rent,
            token_2022_account_space,
            token_2022_account_rent,
            blockhash_cache: Arc::new(BlockhashCache::default()),
            position_commitments: Arc::new(DashMap::new()),
            reserve_lock: Arc::new(std::sync::Mutex::new(())),
            balance_push_slot: AtomicU64::new(0),
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
        // ...and hold those sockets open between sparse trades.
        if let Some(handle) = self.spawn_sender_keep_warm() {
            self.background_tasks.push(handle);
            info!(
                every_ms = self.config.sender.keep_warm_ms,
                sockets = self.config.sender.keep_warm_sockets,
                "🔥 Sender keep-warm running"
            );
        }

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

        // 2. Create compute-budget pair — built once, cloned per create tx. The CU
        // limit is sized per path so the priority fee = price × limit isn't inflated
        // by sizing every tx for the heaviest path.
        let compute = &self.config.compute;
        let price_ix =
            ComputeBudgetInstruction::set_compute_unit_price(compute.price_micro_lamports);
        let cu_ixs_curve_create = vec![
            ComputeBudgetInstruction::set_compute_unit_limit(compute.curve_create_cu),
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
        if self.config.fee_shape.enabled {
            info!(
                "🎲 Fee shape on: buy/sell/amm CU limit drawn up to x{} (price rounded down                  to {} µlamports, fee <= the configured pair), random Sender tip account                  and decoration order per tx",
                self.config.fee_shape.limit_span_mult,
                self.config.fee_shape.price_step_micro_lamports,
            );
        }
        self.cu_ixs_curve_create = cu_ixs_curve_create;

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

        // 5. Pre-fetch all nonce hashes — ONLY in durable-nonce mode. A
        // recent-blockhash consumer (`durable_nonce=false`, e.g. forge's
        // per-wallet ephemeral signers) never acquires a nonce slot on the trade
        // path (`build_trade_tx` → `build_recent_tx`), so reading each nonce
        // account here is pure waste — one `getAccountInfo` per nonce account on
        // EVERY trader init (and forge rebuilds a trader per manage leg). Skip it
        // and leave the slots empty; the hot path never touches them. Hunter
        // (`durable_nonce=true`) is unchanged.
        if self.config.durable_nonce {
            // Fetch each hash first (async), then take the sync lock only for the
            // inserts — a `std::sync::Mutex` guard must not be held across `.await`.
            info!("🔧 Pre-fetching nonce hashes...");
            {
                // One `getMultipleAccounts` (chunked) for the whole pool instead of a
                // sequential `getAccount` per nonce account.
                let fetched = self.prefetch_nonce_hashes(&self.nonce_pubkeys).await?;
                let mut slots = self.nonce_slots.lock().unwrap_or_else(|p| p.into_inner());
                for (pubkey, hash) in fetched {
                    slots.insert(
                        pubkey,
                        NonceSlot {
                            cached_hash: Some(hash),
                            in_use: false,
                            use_epoch: 0,
                            last_push_slot: 0,
                        },
                    );
                }
            }
            info!(
                "✅ Nonce hashes cached for {} account(s)",
                self.nonce_pubkeys.len()
            );
        } else {
            info!("⏭️  Recent-blockhash mode — skipping nonce prefetch (no nonce reads)");
        }

        // 6. Recent-blockhash refresher. Prime once, then watchdog in background:
        // each tick fetches ONLY if the cache wasn't refreshed within the tick —
        // a host that bridges a `blocks_meta` push feed into the cache (see
        // `set_cached_blockhash`) keeps it sub-second fresh and the loop makes
        // zero RPC calls; a host with no feed gets the old poll behavior.
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
                    // HALF the tick, not the whole one. A live push feed stores
                    // every ~400 ms, so half an interval separates "the feed
                    // covered this tick" from "the newest value is our own, and
                    // it is now a tick old". Testing against the full interval
                    // makes the age of our own last write borderline-fresh, so
                    // the fetch is skipped and the cache drifts to TWO intervals
                    // — past `blockhash_max_age_ms`, which puts a live
                    // `getLatestBlockhash` back on the AMM buy hot path.
                    if cache.is_fresher_than(Duration::from_millis(refresh_ms / 2)) {
                        continue; // push feed already covered this tick
                    }
                    info!(
                        refresh_ms,
                        "blockhash watchdog: cache stale — getLatestBlockhash RPC"
                    );
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

    /// Feed a `(blockhash, slot)` from a push source (e.g. a LaserStream
    /// `blocks_meta` subscription) into the recent-blockhash cache. Slot-gated —
    /// out-of-order/replayed metas are ignored. While the feed is live the
    /// background refresher makes zero `getLatestBlockhash` calls (it only acts
    /// as a stall watchdog); see `initialize`.
    pub fn set_cached_blockhash(&self, hash: Hash, slot: u64) {
        self.blockhash_cache.store_pushed(hash, slot);
    }

    /// Earmark `buy_lamports` for `position_id` without a balance check — for an
    /// earmark carried over from another trader (a wallet rebuild) or re-made
    /// at boot. Idempotent: an existing earmark is kept as it is.
    pub fn commit_sol_for_position(&self, position_id: String, buy_lamports: u64) {
        self.position_commitments
            .entry(position_id)
            .or_insert(SolReservation { lamports: buy_lamports, settled: None });
    }

    /// Carry an earmark over exactly, settle state included (a wallet rebuild).
    pub fn restore_reservation(&self, position_id: String, reservation: SolReservation) {
        self.position_commitments.entry(position_id).or_insert(reservation);
    }

    /// Check-and-reserve in one step: earmark `buy_lamports` for `position_id`
    /// only if the free SOL covers it, so concurrent buys on one wallet each see
    /// the others. `true` when the earmark is held (made now, or already held by
    /// an earlier attempt of the same position — a retry is never re-checked).
    /// Fails **open** when no balance has been read yet, so a cold cache does not
    /// block every buy at startup.
    pub fn try_reserve_buy(&self, position_id: &str, buy_lamports: u64) -> bool {
        let _serial = self.reserve_lock.lock().unwrap_or_else(|p| p.into_inner());
        if self.position_commitments.contains_key(position_id) {
            return true;
        }
        if let Some(free) = self.free_lamports() {
            if free < buy_lamports {
                return false;
            }
        }
        self.position_commitments.insert(
            position_id.to_string(),
            SolReservation { lamports: buy_lamports, settled: None },
        );
        true
    }

    /// The reserved buy landed (`slot` of its fill when known): from here the
    /// earmark lasts only until a balance reading that includes the buy.
    pub fn settle_reservation(&self, position_id: &str, slot: Option<u64>) {
        if let Some(mut r) = self.position_commitments.get_mut(position_id) {
            if r.settled.is_none() {
                r.settled = Some(Settled { slot, at: Instant::now() });
            }
        }
    }

    /// Record a sell signed for `key` before it is sent.
    pub fn note_signed_sell(&self, key: &str, signature: &str) {
        self.signed_sells.insert(key.to_string(), signature.to_string());
    }

    /// The send of `signature` returned success: the caller has it in hand.
    pub fn clear_signed_sell(&self, key: &str, signature: &str) {
        self.signed_sells.remove_if(key, |_, s| s == signature);
    }

    /// The signature of the last sell signed for `key` whose send did not return
    /// success — it may still be on the network. Taken once.
    pub fn take_signed_sell(&self, key: &str) -> Option<String> {
        self.signed_sells.remove(key).map(|(_, s)| s)
    }

    /// The SOL earmarked for `position_id`, if any.
    pub fn reserved_lamports_for(&self, position_id: &str) -> Option<u64> {
        self.position_commitments.get(position_id).map(|r| r.lamports)
    }

    /// Release the reservation for `position_id`. No-op if never committed.
    pub fn release_sol_for_position(&self, position_id: &str) {
        self.position_commitments.remove(position_id);
    }

    /// SOL (lamports) earmarked for buys the cached balance does not reflect yet.
    /// Drops the earmarks a balance reading has already covered.
    pub fn committed_lamports(&self) -> u64 {
        let balance = *self.balance_lamports_cache.lock().unwrap_or_else(|p| p.into_inner());
        let mut held = 0u64;
        self.position_commitments.retain(|_, r| {
            let keep = r.holds(balance.as_ref());
            if keep {
                held = held.saturating_add(r.lamports);
            }
            keep
        });
        held
    }

    /// Spendable SOL for the next buy: balance less the reserve floor and the
    /// earmarks. `None` while no balance has been read.
    fn free_lamports(&self) -> Option<u64> {
        let balance = (*self.balance_lamports_cache.lock().unwrap_or_else(|p| p.into_inner()))?;
        Some(
            balance
                .lamports
                .saturating_sub(RESERVE_FLOOR_LAMPORTS)
                .saturating_sub(self.committed_lamports()),
        )
    }

    /// Whether there is enough free SOL to commit `buy_lamports` more, without
    /// reserving it. Fails **open** while no balance has been read.
    pub fn can_commit_buy(&self, buy_lamports: u64) -> bool {
        self.free_lamports().is_none_or(|free| free >= buy_lamports)
    }

    /// Cache an RPC-polled balance (no slot).
    pub fn update_balance_lamports_cache(&self, lamports: u64) {
        *self.balance_lamports_cache.lock().unwrap_or_else(|p| p.into_inner()) =
            Some(BalanceReading { lamports, at: Instant::now(), slot: None });
    }

    /// Cache a pushed balance of account state at `slot`. A push older than the
    /// newest one seen is a replay or an out-of-order frame and is dropped.
    pub fn update_balance_lamports_at_slot(&self, lamports: u64, slot: u64) {
        if self.balance_push_slot.fetch_max(slot, Ordering::Relaxed) > slot {
            return;
        }
        *self.balance_lamports_cache.lock().unwrap_or_else(|p| p.into_inner()) =
            Some(BalanceReading { lamports, at: Instant::now(), slot: Some(slot) });
    }

    /// Read the cached native SOL balance (lamports), if any. `None` until the
    /// first LaserStream account push or safety poll lands — never RPC.
    pub fn cached_balance_lamports(&self) -> Option<u64> {
        self.balance_lamports_cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .map(|r| r.lamports)
    }

    /// `true` when the push-fed (or last polled) wallet balance is newer than
    /// `max_age` — used by the host's `getBalance` watchdog to skip RPC when the
    /// LaserStream account push already covered the tick (mirrors
    /// [`BlockhashCache::is_fresher_than`]).
    pub fn balance_cache_is_fresher_than(&self, max_age: std::time::Duration) -> bool {
        match *self
            .balance_lamports_cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
        {
            Some(r) => r.at.elapsed() < max_age,
            None => false,
        }
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
    let lamports = crate::sol_to_lamports(sol_amount);
    if lamports == 0 {
        return Err(TradeError::InvalidBuyAmount(format!(
            "{sol_amount} SOL rounds to 0 lamports"
        )));
    }
    Ok(lamports)
}

#[cfg(test)]
mod tests {
    use super::{buy_lamports_checked, BalanceReading, Settled, SolReservation, POLL_COVER_LAG};
    use crate::LAMPORTS_PER_SOL;
    use std::time::{Duration, Instant};

    fn reading(at: Instant, slot: Option<u64>) -> BalanceReading {
        BalanceReading { lamports: 1, at, slot }
    }

    /// An in-flight buy holds its SOL; a landed one holds it only until a balance
    /// reading that already shows it spent — never counted free, never twice.
    #[test]
    fn an_earmark_lasts_until_the_balance_shows_the_buy() {
        let t0 = Instant::now();
        let in_flight = SolReservation { lamports: 5, settled: None };
        assert!(in_flight.holds(Some(&reading(t0, Some(100)))));
        assert!(in_flight.holds(None));

        let landed = SolReservation { lamports: 5, settled: Some(Settled { slot: Some(100), at: t0 }) };
        assert!(landed.holds(Some(&reading(t0, Some(99)))), "a push from before the buy");
        assert!(!landed.holds(Some(&reading(t0, Some(100)))), "a push at the buy's slot shows it spent");
        assert!(landed.holds(Some(&reading(t0 + Duration::from_secs(1), None))), "a fresh poll may trail it");
        assert!(!landed.holds(Some(&reading(t0 + POLL_COVER_LAG, None))));
        assert!(landed.holds(None), "no balance yet: keep holding");
    }

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

    /// `1.001 * 1e9` is `1_000_999_999.99…` in `f64`: a truncating cast spends one
    /// lamport less than asked. Every 0.001 step up to the ceiling is exact.
    #[test]
    fn rounds_to_the_nearest_lamport() {
        assert_eq!(buy_lamports_checked(1.001, MAX_BUY_SOL).unwrap(), 1_001_000_000);
        for milli in 1..=5_000u64 {
            let sol = milli as f64 / 1_000.0;
            assert_eq!(
                buy_lamports_checked(sol, MAX_BUY_SOL).unwrap(),
                milli * 1_000_000,
                "{sol} SOL"
            );
        }
    }
}
