use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use tokio::sync::{broadcast, mpsc, watch, Notify};
use tracing::{debug, info, warn};

use serde_json::Value;

use crate::{
    config::constants::{POOL_REFRESH_INTERVAL_SECONDS, POOL_SUBSCRIBE_ACTIVITY_WINDOW_SECONDS},
    models::{
        events::{
            CreatorActivityEvent, InternalEvent, LiquidityEvent, TokenCreatedEvent,
            TokenMigratedEvent, TradeExecutedEvent,
        },
        ingest::{IngestKind, SseEvent, StrategyPing},
        transaction::RawTransaction,
    },
    services::token_sync::derive_pump_swap_pool,
    state::token_cache::{TokenCache, TokenState},
    state::token_metrics::metrics_from_state,
    state::trade_signals::TradeSignals,
    storage::repositories::settings_repo::AppSettings,
    trader::PumpFunTrader,
};

// Cloned ingest internals — self-contained copies, not imported from `ingest/`.
use super::db_writer::{DbWriteOp, RawBlobJob};
use super::decoder::{DecodeOutput, HeliusDecoder, TxRelevance};
use super::proto::geyser::SubscribeUpdateTransaction;

// Larger DB queue so a short volume burst (migration wave, hot-token spike) is
// absorbed here instead of stalling back to the gRPC socket and tripping a
// consumer-lag eviction. Must-persist ops (`Trade`/`Migration`/`Token`/`Wallet`)
// still `send().await` into this buffer; recomputable ops (`Metrics`/`Raw`) are
// `try_send` and shed when it's full (see `enqueue_db_lossy`), so the buffer
// fronts the must-persist writes and effectively never fills under them alone.
const DB_QUEUE_CAP: usize = 16384;
const STRATEGY_QUEUE_CAP: usize = 512;

/// Cumulative count of intentionally-shed hot-path messages under backpressure
/// (Step 1/2 diagnostics): coalesced strategy wakeups and recomputable DB writes
/// dropped because their queue was full. Shared between the pipeline (which
/// increments on each drop) and `run_queue_depth_logger` (which reads + logs).
/// Both are safe to drop — the runner re-derives state from `runtime_cache` on the
/// next ping, and metrics/raw blobs are recomputed/best-effort.
#[derive(Default)]
pub struct ShedCounters {
    pub strategy_pings: AtomicU64,
    pub db_writes: AtomicU64,
}

/// Single hot-path owner: decode → filter → order → cache → queue DB → ping strategy / SSE.
pub struct IngestPipeline {
    decoder: HeliusDecoder,
    token_cache: Arc<TokenCache>,
    db_tx: mpsc::Sender<DbWriteOp>,
    strategy_tx: mpsc::Sender<StrategyPing>,
    sse_tx: broadcast::Sender<SseEvent>,
    pump_program_id: String,
    /// pool → mint index shared with the decoder (to attribute live PumpSwap
    /// swaps, which carry the pool but not the base mint) and with the LaserStream
    /// task (to drive per-pool subscriptions). Holds migrated tokens' pools.
    pool_index: Arc<DashMap<String, String>>,
    /// Pinged whenever a token migrates and a new pool is added to `pool_index`,
    /// so the LaserStream task subscribes to it without dropping the connection.
    pools_changed: Arc<Notify>,
    /// Live settings (persisted in `app_settings`, mutated via the settings API).
    /// `track_mayhem` gates Mayhem-mode ingestion; `track_post_migration` gates
    /// recording migrated tokens' AMM trades. Flipping either off also applies to
    /// already-tracked state (see [`Self::run`]).
    settings_rx: watch::Receiver<AppSettings>,
    /// Trader handle — fed live reserve snapshots from each tracked-token trade
    /// and asked to pre-warm a token's AMM pool caches on its first AMM trade, so
    /// the trade path reads cached reserves / a warm pool instead of RPC.
    trader: Arc<PumpFunTrader>,
    /// Shared wakeup hub. The mint lane is pinged from `on_trade_executed` right
    /// after the trade is appended to the token cache, so the TPSL2 scalp-entry
    /// arming wakes on the trade instead of re-reading the cache on a fixed timer.
    trade_signals: Arc<TradeSignals>,
    /// Tally of shed strategy pings / recomputable DB writes, surfaced by the
    /// queue-depth logger. Shared (`Arc`) with that logger task.
    shed: Arc<ShedCounters>,
}

impl IngestPipeline {
    pub fn new(
        pump_program_id: String,
        token_cache: Arc<TokenCache>,
        db_tx: mpsc::Sender<DbWriteOp>,
        strategy_tx: mpsc::Sender<StrategyPing>,
        sse_tx: broadcast::Sender<SseEvent>,
        settings_rx: watch::Receiver<AppSettings>,
        trader: Arc<PumpFunTrader>,
        trade_signals: Arc<TradeSignals>,
    ) -> Self {
        let (track_mayhem, track_post_migration) = {
            let s = settings_rx.borrow();
            (s.track_mayhem, s.track_post_migration)
        };

        // Seed the pool→mint index for tokens already migrated AND recently
        // active before this process started, so live AMM swaps for them are
        // captured immediately — without subscribing to every pool that ever
        // graduated. Quiet pools are re-added by `run_pool_subscription_refresh`
        // once they trade again; live migrations register via `register_pool`.
        // Skipped entirely when post-migration tracking is disabled.
        let pool_index: Arc<DashMap<String, String>> = Arc::new(DashMap::new());
        if track_post_migration {
            let now = Utc::now();
            for entry in token_cache.iter() {
                if entry.is_migrated && pool_is_live(entry.value(), now) {
                    if let Ok(pool) = derive_pump_swap_pool(entry.key(), &pump_program_id) {
                        pool_index.insert(pool, entry.key().clone());
                    }
                }
            }
        }

        // Honor the Mayhem policy against the seeded cache: if tracking is off at
        // startup, drop any Mayhem tokens the seed loaded so they aren't tracked
        // live (their historical rows stay in the DB).
        if !track_mayhem {
            let mayhem: Vec<String> = token_cache
                .iter()
                .filter(|e| e.token.is_mayhem_mode)
                .map(|e| e.key().clone())
                .collect();
            for mint in &mayhem {
                token_cache.remove(mint);
            }
            if !mayhem.is_empty() {
                info!(
                    "Tracking: Mayhem disabled — dropped {} seeded Mayhem token(s)",
                    mayhem.len()
                );
            }
        }

        let decoder =
            HeliusDecoder::new(pump_program_id.clone()).with_pool_index(pool_index.clone());

        Self {
            decoder,
            token_cache,
            db_tx,
            strategy_tx,
            sse_tx,
            pump_program_id,
            pool_index,
            pools_changed: Arc::new(Notify::new()),
            settings_rx,
            trader,
            trade_signals,
            shed: Arc::new(ShedCounters::default()),
        }
    }

    /// Shared pool → mint index, for the WS task's per-pool subscriptions.
    pub fn pool_index(&self) -> Arc<DashMap<String, String>> {
        self.pool_index.clone()
    }

    /// Shared shed-counter handle, for the queue-depth logger task.
    pub fn shed_counters(&self) -> Arc<ShedCounters> {
        self.shed.clone()
    }

    /// Migration signal, pinged when a new pool is added to [`Self::pool_index`].
    pub fn pools_changed(&self) -> Arc<Notify> {
        self.pools_changed.clone()
    }

    pub fn channel_pair() -> (
        mpsc::Sender<DbWriteOp>,
        mpsc::Receiver<DbWriteOp>,
        mpsc::Sender<StrategyPing>,
        mpsc::Receiver<StrategyPing>,
    ) {
        let (db_tx, db_rx) = mpsc::channel(DB_QUEUE_CAP);
        let (strategy_tx, strategy_rx) = mpsc::channel(STRATEGY_QUEUE_CAP);
        (db_tx, db_rx, strategy_tx, strategy_rx)
    }

    pub async fn run(
        self,
        mut update_rx: mpsc::Receiver<(Arc<SubscribeUpdateTransaction>, TxRelevance)>,
    ) {
        info!("LaserStream pipeline: starting");

        // Independent receiver for change notifications, so the hot path's
        // `borrow()` reads (in the event handlers) never contend with `changed()`.
        // Track the previous values of the two flags we act on, so an unrelated
        // settings change (e.g. timezone) never triggers cache/pool work.
        let mut settings_changed = self.settings_rx.clone();
        let (mut prev_mayhem, mut prev_post_migration) = {
            let s = settings_changed.borrow();
            (s.track_mayhem, s.track_post_migration)
        };

        loop {
            tokio::select! {
                maybe_update = update_rx.recv() => {
                    let Some((update, relevance)) = maybe_update else { break };
                    // Single ingest clock for this tx — used for the trades, the
                    // null-data raw-tx carrier, and the persisted blob (so it keeps
                    // the real receive time, not the DbWriter flush time).
                    let received_at = Utc::now();
                    // The client already classified this tx (curve vs AMM); forward
                    // that verdict so the decoder doesn't re-scan the logs.
                    let decoded = self.decoder.decode_relevant_pb(&update, relevance, received_at);
                    match decoded {
                        DecodeOutput::Transaction { raw_tx, mut events } => {
                            sort_events(&mut events);
                            // Read both flags this tx needs under ONE watch
                            // read-guard instead of borrowing twice per persisted tx.
                            let (track_mayhem, persist_raw) = {
                                let s = self.settings_rx.borrow();
                                (s.track_mayhem, s.persist_raw)
                            };
                            let (events, save_raw) =
                                filter_events(events, &self.token_cache, track_mayhem);
                            if events.is_empty() {
                                continue;
                            }

                            // Hand the DbWriter the typed protobuf (shared Arc) +
                            // ingest scalars; it synthesises the Helius blob off
                            // this hot path. `raw_tx` here is the null-data carrier.
                            if save_raw && persist_raw {
                                self.enqueue_db_lossy(DbWriteOp::Raw(RawBlobJob {
                                    update: update.clone(),
                                    signature: raw_tx.signature.clone(),
                                    slot: raw_tx.slot,
                                    block_time: raw_tx.block_time,
                                    received_at: raw_tx.received_at,
                                }));
                            }

                            for event in events {
                                self.apply_event(event, &raw_tx).await;
                            }
                        }
                        // `Ignored` (and any future variants) are no-ops.
                        _ => {}
                    }
                }

                // A setting changed — act only on genuine transitions of the two
                // flags this pipeline reacts to.
                _ = settings_changed.changed() => {
                    let (mayhem, post_migration) = {
                        let s = settings_changed.borrow();
                        (s.track_mayhem, s.track_post_migration)
                    };

                    // Mayhem flipped off → evict already-tracked Mayhem tokens
                    // (go-forward for new ones is enforced in `filter_events`).
                    if prev_mayhem && !mayhem {
                        self.evict_mayhem_tokens();
                    }
                    prev_mayhem = mayhem;

                    // Post-migration toggled → clear subscribed pools (off) so the
                    // decoder stops attributing AMM swaps, or re-seed live pools
                    // (on) so recording resumes for active migrated tokens.
                    if prev_post_migration != post_migration {
                        if post_migration {
                            self.reseed_live_pools();
                        } else {
                            self.clear_pools();
                        }
                        prev_post_migration = post_migration;
                    }
                }
            }
        }

        info!("LaserStream pipeline: stream closed — stopping");
    }

    /// Drop every Mayhem-mode token from the cache. Their historical DB rows are
    /// left intact; they simply stop receiving live trade/metric updates (and
    /// subsequent trades for them early-return on the cache miss). Re-enabling is
    /// go-forward — evicted tokens require a manual re-sync to track again.
    fn evict_mayhem_tokens(&self) {
        let mints: Vec<String> = self
            .token_cache
            .iter()
            .filter(|e| e.token.is_mayhem_mode)
            .map(|e| e.key().clone())
            .collect();
        for mint in &mints {
            self.token_cache.remove(mint);
        }
        info!(
            "Tracking: Mayhem disabled — evicted {} Mayhem token(s) from cache",
            mints.len()
        );
    }

    /// Drop all subscribed PumpSwap pools. The decoder attributes live AMM swaps
    /// via `pool_index`, so clearing it stops recording post-migration trades
    /// immediately. Notifies the LaserStream client to resubscribe with the now-empty
    /// pool set so Helius immediately stops matching (and billing for) those accounts —
    /// without the notify, the filter is only trimmed on the next natural reconnect.
    fn clear_pools(&self) {
        let n = self.pool_index.len();
        self.pool_index.clear();
        self.pools_changed.notify_one();
        info!("Tracking: post-migration disabled — cleared {n} pool(s); AMM trades no longer recorded");
    }

    /// Re-seed `pool_index` with the pools of currently-active migrated tokens and
    /// wake the WS task to subscribe. Inverse of [`Self::clear_pools`], used when
    /// post-migration tracking is re-enabled.
    fn reseed_live_pools(&self) {
        let now = Utc::now();
        let mut added = false;
        for entry in self.token_cache.iter() {
            if entry.is_migrated && pool_is_live(entry.value(), now) {
                if let Ok(pool) = derive_pump_swap_pool(entry.key(), &self.pump_program_id) {
                    if self.pool_index.insert(pool, entry.key().clone()).is_none() {
                        added = true;
                    }
                }
            }
        }
        if added {
            self.pools_changed.notify_one();
        }
        info!("Tracking: post-migration enabled — re-seeded live pools");
    }

    async fn apply_event(&self, event: InternalEvent, raw_tx: &RawTransaction) {
        match event {
            InternalEvent::TokenCreated(e) => self.on_token_created(e).await,
            InternalEvent::TradeExecuted(e) => self.on_trade_executed(e).await,
            InternalEvent::TokenMigrated(e) => self.on_token_migrated(e).await,
            InternalEvent::CreatorActivityDetected(e) => self.on_creator_activity(e),
            InternalEvent::LiquidityAdded(e) => self.on_liquidity(e, true, raw_tx),
            InternalEvent::LiquidityRemoved(e) => self.on_liquidity(e, false, raw_tx),
        }
    }

    /// Persist a **money-critical** write op (`Trade`/`Migration`/`Token`/`Wallet`).
    /// Uses `send().await` (real backpressure) rather than `try_send`: such a write
    /// must never be silently dropped, so a burst past `DB_QUEUE_CAP` slows the
    /// ingest loop instead. The large `DB_QUEUE_CAP` fronts these so they almost
    /// never actually block (recomputable ops are shed first — see
    /// [`Self::enqueue_db_lossy`]). The only error here is a closed channel
    /// (DbWriter gone at shutdown).
    async fn enqueue_db(&self, op: DbWriteOp) {
        if let Err(e) = self.db_tx.send(op).await {
            warn!("DbWriter channel closed — write not persisted: {e}");
        }
    }

    /// Enqueue a **recomputable** write op (`Metrics`/`Raw`). Uses `try_send`: if
    /// the DB queue is full we drop it (bumping a shed counter) rather than stall
    /// the ingest socket and risk a consumer-lag eviction. Metrics are a full
    /// snapshot recomputed on the very next trade; raw blobs are best-effort and
    /// already gated by `persist_raw`. Money-critical writes go through
    /// [`Self::enqueue_db`] and are never shed.
    fn enqueue_db_lossy(&self, op: DbWriteOp) {
        match self.db_tx.try_send(op) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.shed.db_writes.fetch_add(1, Ordering::Relaxed);
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                warn!("DbWriter channel closed — recomputable write dropped");
            }
        }
    }

    /// Ping a strategy. Uses `try_send` (non-blocking): a wakeup is only a hint to
    /// re-evaluate, and the runner re-derives all rule/position state from
    /// `runtime_cache` on each ping (plus a periodic time-exit sweep), so a
    /// coalesced/dropped ping under backpressure can't lose an exit — it fires on
    /// the next trade's ping. Dropping it here keeps strategy slowness from
    /// stalling the ingest socket. Closed channel (runner gone at shutdown) warns.
    fn ping_strategy(&self, mint: String, kind: IngestKind) {
        match self.strategy_tx.try_send(StrategyPing { mint, kind }) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.shed.strategy_pings.fetch_add(1, Ordering::Relaxed);
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                warn!("Strategy channel closed — ping not delivered");
            }
        }
    }

    fn emit_sse(&self, event: SseEvent) {
        // No dashboard connected → skip the broadcast ring write entirely (L4).
        // A cheap atomic load instead of moving the event into the buffer for
        // nobody to read; the steady state with no UI open does zero SSE work.
        if self.sse_tx.receiver_count() == 0 {
            return;
        }
        let _ = self.sse_tx.send(event);
    }

    /// Register a migrated token's canonical PumpSwap pool in the pool→mint
    /// index so the decoder can attribute live AMM swaps (which carry the pool,
    /// not the base mint) back to this token, and wake the WS task to subscribe
    /// to it. Idempotent; only a genuinely new pool pings `pools_changed`.
    fn register_pool(&self, mint: &str) {
        match derive_pump_swap_pool(mint, &self.pump_program_id) {
            Ok(pool) => {
                if self.pool_index.insert(pool, mint.to_string()).is_none() {
                    self.pools_changed.notify_one();
                }
            }
            Err(e) => warn!("Pool derivation failed for {mint}: {e}"),
        }
    }

    async fn on_token_created(&self, e: TokenCreatedEvent) {
        let mint = e.token.mint_address.clone();
        if self.token_cache.contains_key(&mint) {
            debug!("Duplicate TokenCreated for {mint} — skipping");
            return;
        }

        // info!(
        //     name = %e.token.name,
        //     symbol = %e.token.symbol,
        //     mint = %mint,
        //     "New token tracked"
        // );

        let creator = e.token.creator_wallet.clone();
        self.enqueue_db(DbWriteOp::Token(e.token.clone())).await;
        self.enqueue_db(DbWriteOp::Wallet(creator)).await;

        let token_state = TokenState::new(e.token);
        let metrics = metrics_from_state(&mint, &token_state);
        self.token_cache.insert(mint.clone(), token_state);
        self.enqueue_db_lossy(DbWriteOp::Metrics(metrics));

        self.ping_strategy(mint.clone(), IngestKind::TokenCreated);
        self.emit_sse(SseEvent::TokenCreated {
            mint,
            tx_signature: e.tx_signature,
            slot: e.slot,
            timestamp: e.timestamp,
        });
    }

    async fn on_trade_executed(&self, mut e: TradeExecutedEvent) {
        let mint = e.trade.mint_address.clone();
        let wallet = e.trade.wallet_address.clone();

        if !self.token_cache.contains_key(&mint) {
            return;
        }

        // debug!(
        //     mint = %mint,
        //     wallet = %wallet,
        //     kind = ?e.trade.trade_type,
        //     "Trade applied"
        // );

        // Capture the Copy reserve/venue/SSE facts before the trade is moved into
        // the token-state aggregate below, so the (label-JSON-carrying) Trade is
        // cloned exactly once — for the DB write — instead of twice.
        let is_amm = e.trade.venue == "amm";
        let reserve_snapshot = e
            .trade
            .virtual_token_reserves
            .zip(e.trade.virtual_sol_reserves);
        let trade_type = e.trade.trade_type;
        let sol_amount = e.trade.sol_amount;
        let token_amount = e.trade.token_amount;
        let price_per_token = e.trade.price_per_token;

        // Move the label JSON OUT of the in-memory Trade before cloning for the DB
        // write, then attach it only to the DB copy. The trades API displays the
        // labels (so the DB copy keeps them); the in-memory ring never reads
        // per-trade labels (strategy reads Token-level labels only). Doing the
        // move-then-clone avoids deep-copying the whole `instruction_labels` JSON
        // array — one full heap copy per trade, on the highest-volume event — only
        // to null it out immediately afterwards.
        let labels = std::mem::replace(&mut e.trade.instruction_labels, Value::Null);
        let mut db_trade = e.trade.clone();
        db_trade.instruction_labels = labels;
        self.enqueue_db(DbWriteOp::Trade(db_trade)).await;
        self.enqueue_db(DbWriteOp::Wallet(wallet.clone())).await;

        // Apply the trade to token state and resolve the (first-AMM-trade) pool
        // prewarm — both under a SINGLE `get_mut` guard, so one event takes one
        // write-lock on the shard instead of two, halving contention against API
        // readers + the strategy runner. The dead-token verdict is folded into the
        // metrics below as a cheap in-memory read (no DB scan, no throttle needed).
        let (metrics, to_warm) = match self.token_cache.get_mut(&mint) {
            Some(mut token_state) => {
                token_state.add_trade(e.trade);

                let metrics = metrics_from_state(&mint, &token_state);

                // First AMM trade for this mint: capture the token program (when
                // known) so the background task can warm its PumpSwap pool caches
                // off the eventual exit path. Check-and-set the warm flag in the
                // same guard; an unknown program leaves it false so a later AMM
                // trade retries — never cache a guessed pool layout.
                let to_warm = if is_amm && !token_state.amm_pool_prewarmed {
                    match token_state.token.token_program_id.clone() {
                        Some(token_program) => {
                            token_state.amm_pool_prewarmed = true;
                            Some(token_program)
                        }
                        None => None,
                    }
                } else {
                    None
                };

                (Some(metrics), to_warm)
            }
            None => (None, None),
        };

        // Wake the TPSL2 scalp-entry arming the instant the cache reflects this
        // trade (the `get_mut` guard above is dropped, so the shard lock is free).
        // No-op (one `is_empty` check) unless a position is currently arming on
        // this mint — i.e. for virtually every trade.
        self.trade_signals.notify_mint(&mint);

        if let Some(metrics) = metrics {
            self.enqueue_db_lossy(DbWriteOp::Metrics(metrics));
        }

        // Feed the trader's live reserve cache from this trade's post-trade
        // snapshot, so a subsequent buy/sell skips the on-chain reserve read. The
        // venue tag keeps curve and AMM snapshots from being mixed up.
        if let Some((token_reserves, sol_reserves)) = reserve_snapshot {
            self.trader
                .update_live_reserves(&mint, token_reserves, sol_reserves, is_amm);
        }

        // On a token's first AMM trade, warm its PumpSwap pool caches (pool facts
        // + fee-share marker + global config) in the background — moving that cold
        // fetch off the bot's eventual exit path. The check-and-set ran above under
        // the shared `get_mut`, so this only spawns the warm task.
        if let Some(token_program) = to_warm {
            let trader = self.trader.clone();
            let token_cache = self.token_cache.clone();
            let warm_mint = mint.clone();
            tokio::spawn(async move {
                if let Err(err) = trader.prewarm_amm_pool(&warm_mint, &token_program).await {
                    debug!("AMM pool prewarm failed for {warm_mint}: {err}");
                    // Clear the flag so a later AMM trade retries the warm.
                    if let Some(mut s) = token_cache.get_mut(&warm_mint) {
                        s.amm_pool_prewarmed = false;
                    }
                }
            });
        }

        self.ping_strategy(mint.clone(), IngestKind::Trade);
        self.emit_sse(SseEvent::TradeExecuted {
            mint,
            wallet,
            trade_type,
            sol_amount,
            token_amount,
            price_per_token,
            tx_signature: e.tx_signature,
            slot: e.slot,
            timestamp: e.timestamp,
        });
    }

    async fn on_token_migrated(&self, e: TokenMigratedEvent) {
        let mint = e.mint_address.clone();
        if !self.token_cache.contains_key(&mint) {
            return;
        }

        // Register the pool now (before AMM swaps begin) so the decoder can
        // attribute them and the WS task subscribes to this pool — unless
        // post-migration tracking is disabled, in which case the migration is
        // still recorded but its AMM trades are not.
        if self.settings_rx.borrow().track_post_migration {
            self.register_pool(&mint);
        }

        let metrics = self.token_cache.get_mut(&mint).map(|mut token_state| {
            token_state.is_migrated = true;
            metrics_from_state(&mint, &token_state)
        });
        if let Some(metrics) = metrics {
            self.enqueue_db_lossy(DbWriteOp::Metrics(metrics));
        }

        self.enqueue_db(DbWriteOp::Migration { mint: mint.clone() }).await;
        self.ping_strategy(mint, IngestKind::Migrated);
    }

    fn on_creator_activity(&self, e: CreatorActivityEvent) {
        if !self.token_cache.contains_key(&e.mint_address) {
            return;
        }
        self.ping_strategy(e.mint_address, IngestKind::CreatorActivity);
    }

    fn on_liquidity(&self, e: LiquidityEvent, added: bool, _raw_tx: &RawTransaction) {
        let mint = e.mint_address.clone();
        if !self.token_cache.contains_key(&mint) {
            return;
        }
        let sse = if added {
            SseEvent::LiquidityAdded {
                mint: mint.clone(),
                wallet: e.wallet_address,
                sol_amount: e.sol_amount,
                token_amount: e.token_amount,
                tx_signature: e.tx_signature,
                slot: e.slot,
                timestamp: e.timestamp,
            }
        } else {
            SseEvent::LiquidityRemoved {
                mint: mint.clone(),
                wallet: e.wallet_address,
                sol_amount: e.sol_amount,
                token_amount: e.token_amount,
                tx_signature: e.tx_signature,
                slot: e.slot,
                timestamp: e.timestamp,
            }
        };
        self.emit_sse(sse);
    }
}

fn sort_events(events: &mut [InternalEvent]) {
    // Most txs decode to a single event; skip the sort setup entirely for them.
    if events.len() < 2 {
        return;
    }
    events.sort_by_key(|e| match e {
        InternalEvent::TokenCreated(_) => 0,
        InternalEvent::TokenMigrated(_) => 1,
        InternalEvent::TradeExecuted(_) => 2,
        InternalEvent::CreatorActivityDetected(_) => 3,
        InternalEvent::LiquidityAdded(_) => 4,
        InternalEvent::LiquidityRemoved(_) => 5,
    });
}

fn filter_events(
    events: Vec<InternalEvent>,
    cache: &TokenCache,
    track_mayhem: bool,
) -> (Vec<InternalEvent>, bool) {
    let mut out = Vec::new();
    let mut save_raw = false;
    // Track mints created in THIS batch so the dev-buy TradeExecuted that
    // arrives in the same create+buy tx passes the gate below. The cache isn't
    // updated until apply_event runs, so checking it alone drops the init buy.
    let mut pending_mints: Vec<String> = Vec::new();

    for event in events {
        match &event {
            InternalEvent::TokenCreated(e) => {
                // Tracking policy: drop Mayhem-mode creates entirely when
                // disabled, so neither the token nor its raw tx is persisted.
                if e.token.is_mayhem_mode && !track_mayhem {
                    continue;
                }
                pending_mints.push(e.token.mint_address.clone());
                save_raw = true;
                out.push(event);
            }
            InternalEvent::TradeExecuted(e) => {
                if cache.contains_key(&e.trade.mint_address)
                    || pending_mints.contains(&e.trade.mint_address)
                {
                    save_raw = true;
                    out.push(event);
                }
            }
            InternalEvent::TokenMigrated(e) => {
                if cache.contains_key(&e.mint_address) {
                    save_raw = true;
                    out.push(event);
                }
            }
            InternalEvent::CreatorActivityDetected(e) => {
                if cache.contains_key(&e.mint_address) {
                    save_raw = true;
                    out.push(event);
                }
            }
            InternalEvent::LiquidityAdded(e) | InternalEvent::LiquidityRemoved(e) => {
                if cache.contains_key(&e.mint_address) {
                    save_raw = true;
                    out.push(event);
                }
            }
        }
    }

    (out, save_raw)
}

/// True when a migrated token traded recently enough to keep its PumpSwap pool
/// in the live subscription set. A token with no recorded trades, quiet beyond the
/// window, or flagged dead (dust-trading but gone — which the activity window alone
/// would miss) is treated as not live and pruned from the subscription.
fn pool_is_live(state: &TokenState, now: DateTime<Utc>) -> bool {
    if state.is_dead(now) {
        return false;
    }
    state
        .last_trade_at
        .map(|t| {
            now.signed_duration_since(t).num_seconds() < POOL_SUBSCRIBE_ACTIVITY_WINDOW_SECONDS
        })
        .unwrap_or(false)
}

/// Periodic ingest backpressure diagnostics (Step 0). Every 10s, logs how full
/// the three hot-path channels are (depth/capacity) plus the cumulative count of
/// shed strategy pings / recomputable DB writes — so a consumer-lag stall shows up
/// as queues sitting near-full (and shed counts climbing) right before a
/// reconnect. Only logs when something is actually backed up or has been shed, so
/// a healthy steady state stays quiet. Holds **weak** sender handles so it never
/// keeps a channel alive past ingest shutdown: if any upgrade fails the producers
/// are gone and the logger exits.
pub async fn run_queue_depth_logger(
    update_tx: mpsc::WeakSender<(Arc<SubscribeUpdateTransaction>, TxRelevance)>,
    db_tx: mpsc::WeakSender<DbWriteOp>,
    strategy_tx: mpsc::WeakSender<StrategyPing>,
    shed: Arc<ShedCounters>,
) {
    let mut tick = tokio::time::interval(Duration::from_secs(10));
    tick.tick().await; // consume the immediate first tick
    loop {
        tick.tick().await;

        let (Some(update_tx), Some(db_tx), Some(strategy_tx)) =
            (update_tx.upgrade(), db_tx.upgrade(), strategy_tx.upgrade())
        else {
            return; // a producer/consumer dropped — ingest is shutting down
        };

        // `max_capacity - capacity` = currently-queued (in-flight) messages.
        let upd_cap = update_tx.max_capacity();
        let upd = upd_cap - update_tx.capacity();
        let db_cap = db_tx.max_capacity();
        let db = db_cap - db_tx.capacity();
        let strat_cap = strategy_tx.max_capacity();
        let strat = strat_cap - strategy_tx.capacity();

        let shed_pings = shed.strategy_pings.load(Ordering::Relaxed);
        let shed_db = shed.db_writes.load(Ordering::Relaxed);

        if upd > 0 || db > 0 || strat > 0 || shed_pings > 0 || shed_db > 0 {
            info!(
                "Ingest queue depth: update_tx {upd}/{upd_cap}, db_tx {db}/{db_cap}, \
                 strategy_tx {strat}/{strat_cap} | shed: pings={shed_pings} db_writes={shed_db}"
            );
        }
    }
}

/// Periodic revival sweep: subscribe to the PumpSwap pools of migrated tokens
/// that have become active since the last pass but aren't yet covered. This is
/// what keeps a pool pruned from the startup set (or whose live `Migrate` event
/// was missed) from going blind permanently — once fresh trades land in the
/// cache (e.g. via a manual sync that refreshes `last_trade_at`), its pool is
/// added and the WS task subscribes on the next ping. Evicts dead/inactive pools
/// on each tick to shrink the gRPC account filter and adds newly-active ones;
/// notifies the LaserStream client immediately on any change.
pub async fn run_pool_subscription_refresh(
    token_cache: Arc<TokenCache>,
    pool_index: Arc<DashMap<String, String>>,
    pools_changed: Arc<Notify>,
    pump_program_id: String,
    settings_rx: watch::Receiver<AppSettings>,
) {
    let mut tick = tokio::time::interval(Duration::from_secs(POOL_REFRESH_INTERVAL_SECONDS));
    loop {
        tick.tick().await;

        // Respect the tracking policy: never revive pools while post-migration
        // tracking is disabled (it would undo `clear_pools`).
        if !settings_rx.borrow().track_post_migration {
            continue;
        }

        let now = Utc::now();
        let mut changed = false;

        // Evict dead pools first: remove any pool whose token is no longer
        // live so the gRPC account_include shrinks immediately rather than
        // accumulating every pool that ever migrated.
        pool_index.retain(|_pool, mint| {
            let live = token_cache
                .get(mint)
                .map(|e| pool_is_live(e.value(), now))
                .unwrap_or(false);
            if !live {
                changed = true;
            }
            live
        });

        // Add any newly-active pools not yet subscribed.
        for entry in token_cache.iter() {
            if !entry.is_migrated || !pool_is_live(entry.value(), now) {
                continue;
            }
            if let Ok(pool) = derive_pump_swap_pool(entry.key(), &pump_program_id) {
                // Coverage is an O(1) `pool_index` lookup on the derived pool —
                // no need to rebuild a full mint set every tick. `insert`
                // returning `None` means the pool was newly added.
                if pool_index.contains_key(&pool) {
                    continue;
                }
                if pool_index.insert(pool, entry.key().clone()).is_none() {
                    changed = true;
                }
            }
        }

        // Safety net for the AMM-toggle race: if clear_pools() fired while we were
        // iterating above and we just re-added pools into the freshly-cleared index,
        // undo it now. Without this check the added pools stay in pool_index
        // indefinitely — the AMM-off guard at the top of this loop only fires on
        // future ticks (the next 120 s), so escaped pools are never evicted.
        if !settings_rx.borrow().track_post_migration && !pool_index.is_empty() {
            pool_index.clear();
            changed = true;
        }

        if changed {
            pools_changed.notify_one();
        }
    }
}
