//! Host-side ingest consumer: translates [`IngestEvent`] to `trading_core` domain
//! types and routes to token-cache, DB queue, strategy runner, SSE, and trader.
//!
//! This is the former `ingest-laserstream::pipeline` — rewritten to consume the
//! standalone crate's event channel instead of driving the decoder directly.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use chrono::Utc;
use serde_json::{json, Value};
use tokio::sync::{broadcast, mpsc, watch};
use tracing::{debug, info, warn};
use uuid::Uuid;

use ingest_laserstream::{
    IngestHandle,
    event::{BuyInstructionArgs, CreatorActivityEvent, IngestEvent, LiquidityEvent, Side, TokenCreated, TokenMigrated, Trade as IlTrade, Venue},
};

use trading_core::{
    config::constants::POOL_SUBSCRIBE_ACTIVITY_WINDOW_SECONDS,
    models::{
        ingest::{IngestKind, SseEvent, StrategyPing},
        token::{create_meta, Token},
        trade::{Trade, TradeType},
    },
    state::token_cache::{TokenCache, TokenState},
    state::token_metrics::metrics_from_state,
    state::trade_signals::TradeSignals,
    storage::repositories::settings_repo::AppSettings,
};

use trading_core::ingest::TraderHook;

use super::db_writer::DbWriteOp;
use super::held_pools::HeldPoolGate;

pub const DB_QUEUE_CAP: usize = 16384;
pub const STRATEGY_QUEUE_CAP: usize = 512;
/// Dedicated create-ping lane. Creates are rare vs trade pings; keeping them off
/// the trade queue means a saturated AMM/curve stream can never shed a snipe.
pub const CREATE_STRATEGY_QUEUE_CAP: usize = 256;
/// Rate-limit for the strategy-shed warning: log the first drop, then every Nth.
/// Loud enough to surface a deaf engine within seconds, quiet enough that a brief
/// burst under load does not flood the log.
const STRATEGY_SHED_LOG_EVERY: u64 = 5_000;
/// Bounded overflow for durable writes that could not enter the hot `db_tx`
/// without blocking. A dedicated retry task drains this into `db_tx` with
/// `send().await` so a PG stall does not silently drop Trade/Token/Wallet/
/// Migration rows (C2). When this buffer is also full the write is shed and
/// counted.
pub const DB_RETRY_CAP: usize = 4096;

/// `LATENCY_TRACE=1` ⇒ trade/migrate/creator pings carry their transport receive
/// stamp, so `snipe_latency` can report `recv_to_ping_ms` / `ping_to_decide_ms`
/// for a **flow-triggered** rule instead of only the create lane.
///
/// Off by default, and deliberately a flag rather than always-on: the create lane
/// fires a few times a second, the trade lane hundreds, and stamping costs a mint
/// `String` clone plus a `DashMap` insert **per ping** — the per-event allocation
/// the hot-path budget forbids. Turn it on to measure, read the distribution, turn
/// it back off. Read once and cached: an env lookup per trade is itself a hot-path
/// cost.
fn latency_trace() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        matches!(
            std::env::var("LATENCY_TRACE").as_deref(),
            Ok("1") | Ok("true") | Ok("TRUE")
        )
    })
}

/// Counts of intentionally-shed messages under back-pressure.
#[derive(Default)]
pub struct ShedCounters {
    pub strategy_pings: AtomicU64,
    /// Create-lane sheds only — a non-zero count here is a missed snipe under
    /// engine stall, not trade-queue pressure.
    pub create_pings: AtomicU64,
    pub db_writes: AtomicU64,
    /// Durable ops deferred onto the retry buffer (not shed — still pending).
    pub db_deferred: AtomicU64,
}

pub struct IngestConsumer {
    token_cache: Arc<TokenCache>,
    db_tx: mpsc::Sender<DbWriteOp>,
    /// Overflow for durable writes that could not enter `db_tx` without blocking.
    db_retry_tx: mpsc::Sender<DbWriteOp>,
    /// Trade / migrate / creator-activity pings (general lane).
    strategy_tx: mpsc::Sender<StrategyPing>,
    /// `TokenCreated` pings only — never shares capacity with trade volume.
    create_tx: mpsc::Sender<StrategyPing>,
    sse_tx: broadcast::Sender<SseEvent>,
    settings_rx: watch::Receiver<AppSettings>,
    trader: Arc<dyn TraderHook>,
    trade_signals: Arc<TradeSignals>,
    ingest_handle: Arc<IngestHandle>,
    held_pools: HeldPoolGate,
    /// Real trading wallet — used to early-observe own legs for fill confirm.
    trading_wallet: String,
    shed: Arc<ShedCounters>,
}

impl IngestConsumer {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        token_cache: Arc<TokenCache>,
        db_tx: mpsc::Sender<DbWriteOp>,
        db_retry_tx: mpsc::Sender<DbWriteOp>,
        strategy_tx: mpsc::Sender<StrategyPing>,
        create_tx: mpsc::Sender<StrategyPing>,
        sse_tx: broadcast::Sender<SseEvent>,
        settings_rx: watch::Receiver<AppSettings>,
        trader: Arc<dyn TraderHook>,
        trade_signals: Arc<TradeSignals>,
        ingest_handle: Arc<IngestHandle>,
        held_pools: HeldPoolGate,
        trading_wallet: String,
    ) -> (Self, Arc<ShedCounters>) {
        let shed = Arc::new(ShedCounters::default());
        (
            Self {
                token_cache,
                db_tx,
                db_retry_tx,
                strategy_tx,
                create_tx,
                sse_tx,
                settings_rx,
                trader,
                trade_signals,
                ingest_handle,
                held_pools,
                trading_wallet,
                shed: shed.clone(),
            },
            shed,
        )
    }

    pub async fn run(self, mut event_rx: mpsc::Receiver<IngestEvent>) {
        let mut settings_changed = self.settings_rx.clone();
        let (mut prev_mayhem, mut prev_post_migration) = {
            let s = settings_changed.borrow();
            (s.track_mayhem, s.track_post_migration)
        };
        // True when any semantic event from the current tx was processed (used
        // to decide whether to persist its RawTx blob).
        let mut tracked_in_current_tx = false;

        loop {
            tokio::select! {
                maybe_ev = event_rx.recv() => {
                    let Some(ev) = maybe_ev else { break };
                    match ev {
                        IngestEvent::TokenCreated(e) => {
                            let (track_mayhem, persist_raw) = {
                                let s = self.settings_rx.borrow();
                                (s.track_mayhem, s.persist_raw)
                            };
                            if !e.is_mayhem_mode || track_mayhem {
                                tracked_in_current_tx = true;
                                self.on_token_created(e, persist_raw).await;
                            }
                        }
                        IngestEvent::Trade(e) => {
                            let persist_raw = self.settings_rx.borrow().persist_raw;
                            if self.token_cache.contains_key(&e.mint) {
                                tracked_in_current_tx = true;
                                self.on_trade(e, persist_raw).await;
                            }
                        }
                        IngestEvent::TokenMigrated(e) => {
                            let track_post_migration = self.settings_rx.borrow().track_post_migration;
                            if self.token_cache.contains_key(&e.mint) {
                                tracked_in_current_tx = true;
                                self.on_token_migrated(e, track_post_migration).await;
                            }
                        }
                        IngestEvent::Liquidity(e) => {
                            if self.token_cache.contains_key(&e.mint) {
                                tracked_in_current_tx = true;
                                self.on_liquidity(e);
                            }
                        }
                        IngestEvent::CreatorActivity(e) => {
                            if self.token_cache.contains_key(&e.mint) {
                                tracked_in_current_tx = true;
                                self.on_creator_activity(e);
                            }
                        }
                        IngestEvent::RawTx(raw) => {
                            let persist_raw = self.settings_rx.borrow().persist_raw;
                            if tracked_in_current_tx && persist_raw {
                                self.enqueue_db_lossy(DbWriteOp::Raw(super::db_writer::RawBlobJob {
                                    signature: raw.signature,
                                    slot: raw.slot,
                                    tx_index: raw.tx_index,
                                    block_time: raw.block_time,
                                    payload: raw.payload,
                                }));
                            }
                            tracked_in_current_tx = false;
                        }
                    }
                }

                _ = settings_changed.changed() => {
                    let (mayhem, post_migration) = {
                        let s = settings_changed.borrow();
                        (s.track_mayhem, s.track_post_migration)
                    };

                    if prev_mayhem && !mayhem {
                        self.evict_mayhem_tokens();
                    }
                    prev_mayhem = mayhem;

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

        info!("Ingest consumer: event channel closed — stopping");
    }

    // ── Event handlers ─────────────────────────────────────────────────────────

    async fn on_token_created(&self, e: TokenCreated, _persist_raw: bool) {
        let mint = e.mint.clone();
        if self.token_cache.contains_key(&mint) {
            debug!("Duplicate TokenCreated for {mint} — skipping");
            return;
        }

        let creator = e.creator.clone();
        let signature = e.signature.clone();
        let slot = e.slot;
        let block_time = e.block_time;
        let received_at = e.received_at;
        let token = token_from_event(e);

        // Hot path first: track the token in the in-RAM cache and fire the strategy
        // fingerprint match BEFORE the durable enqueue, so a backpressured DbWriter
        // can't delay an entry decision (H2). The strategy reads `token_cache`, not
        // the DB, so this ordering is safe.
        let token_state = TokenState::new(token.clone());
        let metrics = metrics_from_state(&mint, &token_state);
        self.token_cache.insert(mint.clone(), token_state);
        self.ping_strategy(mint.clone(), IngestKind::TokenCreated, Some(received_at));

        self.enqueue_db_durable(DbWriteOp::Token(token));
        self.enqueue_db_durable(DbWriteOp::Wallet(creator));
        self.enqueue_db_lossy(DbWriteOp::Metrics(metrics));

        self.emit_sse(SseEvent::TokenCreated {
            mint_address: mint,
            tx_signature: signature,
            slot,
            timestamp: block_time,
        });
    }

    async fn on_trade(&self, e: IlTrade, _persist_raw: bool) {
        let mint = e.mint.clone();
        let wallet = e.wallet.clone();

        let is_amm = e.venue == Venue::Amm;
        let reserve_snapshot = e.reserves.virtual_token.zip(e.reserves.virtual_sol);
        let trade_type = trade_type(e.side);
        let amount_sol = e.sol;
        let token_amount = e.tokens;
        let price_per_token = e.price;

        let labels_json = labels_to_json(&e.instruction_labels);
        let mut core_trade = trade_from_event(&e);
        // Captured before `core_trade` moves into the cache — the SSE frame below
        // needs it so a live-appended row shows the same fee a refetch would.
        let fee_sol = core_trade.fee_sol;
        let db_trade = {
            let mut t = core_trade.clone();
            t.instruction_labels = labels_json;
            t
        };
        // Hash from the event labels before nulling the cache projection — keeps
        // flow-split metrics correct while avoiding hash work under DashMap.
        let (ix_hash, wallet_hash, marker_bits) = {
            use hunter_engine::metrics::flow_split::{ix_hash_opt, marker_bits, wallet_hash};
            (
                ix_hash_opt(&e.instruction_labels),
                wallet_hash(&wallet),
                marker_bits(&e.instruction_labels),
            )
        };
        core_trade.instruction_labels = Value::Null;

        // ── Hot path first (H2) ────────────────────────────────────────────────
        // Update the in-RAM cache, refresh the trader's live reserves, and ping the
        // strategy BEFORE the durable Trade/Wallet enqueue, so a backpressured
        // DbWriter can't stall a real exit's decision. `notify_mint` stays *after*
        // the Trade enqueue so the sell-confirm feed still observes the row-in-flight
        // in the same order as before (the confirm loop re-queries `trades` on the
        // next notify; waking it before the write is even queued could miss a
        // last-leg clear under backpressure).
        //
        // Keep the DashMap mut guard short: precomputed hashes + AMM observe outside.
        let (metrics, amm_token_program) = match self.token_cache.get_mut(&mint) {
            Some(mut token_state) => {
                token_state.add_trade_hashed(core_trade, ix_hash, wallet_hash, marker_bits);
                let tp = if is_amm && !token_state.amm_pool_prewarmed {
                    token_state.token.token_program_id.clone()
                } else {
                    None
                };
                (Some(metrics_from_state(&mint, &token_state)), tp)
            }
            None => (None, None),
        };
        if let (Some(token_program), Some(keys)) =
            (amm_token_program, e.amm_swap_accounts.as_deref())
        {
            if self
                .trader
                .observe_amm_swap_accounts(&mint, &token_program, keys)
            {
                if let Some(mut token_state) = self.token_cache.get_mut(&mint) {
                    token_state.amm_pool_prewarmed = true;
                }
            }
        }

        if let Some((token_reserves, sol_reserves)) = reserve_snapshot {
            // The trader takes reserves as `f64` (spot-price ratio math; it's a
            // standalone lib). Cast the raw token reserves at this boundary.
            self.trader.update_live_reserves(&mint, token_reserves as f64, sol_reserves, is_amm);
        }

        // Stamped only under `LATENCY_TRACE` — see `latency_trace()`. Off (the
        // default) this is `None` and the trade lane behaves exactly as it did
        // before the flag existed.
        self.ping_strategy(
            mint.clone(),
            IngestKind::Trade,
            latency_trace().then_some(e.received_at),
        );

        // Own-wallet preview + early wake *before* durable enqueue so buy/sell
        // confirm can resolve without waiting on DbWriter commit.
        if wallet == self.trading_wallet {
            self.trade_signals.observe_own_leg(
                &wallet,
                &mint,
                &e.signature,
                e.tokens,
                e.sol,
                e.block_time,
                e.slot,
            );
        }

        // ── Durable sinks (non-blocking; defer under backpressure) ─────────────
        self.enqueue_db_durable(DbWriteOp::Trade(db_trade));
        self.enqueue_db_durable(DbWriteOp::Wallet(wallet.clone()));

        // Wake mint-lane watchers after the Trade write is queued (or deferred).
        self.trade_signals.notify_mint(&mint);

        if let Some(metrics) = metrics {
            self.enqueue_db_lossy(DbWriteOp::Metrics(metrics));
        }

        self.emit_sse(SseEvent::TradeExecuted {
            mint_address: mint,
            wallet,
            trade_type,
            amount_sol,
            token_amount,
            price_per_token,
            fee_sol,
            tx_signature: e.signature,
            tx_index: e.tx_index,
            leg_index: e.leg_index,
            reserve_sol: e.reserves.virtual_sol,
            // Token reserves are raw u64 on the event; chart/REST use f64 amounts.
            reserve_token: e.reserves.virtual_token.map(|v| v as f64),
            venue: venue_str(e.venue).to_string(),
            // Moved, not cloned — the DB row got its own JSON copy above and the
            // flow-split hashes were taken by reference, so `e` is done with them.
            instruction_labels: e.instruction_labels,
            slot: e.slot,
            timestamp: e.block_time,
        });
    }

    async fn on_token_migrated(&self, e: TokenMigrated, track_post_migration: bool) {
        let mint = e.mint.clone();

        // The decode task auto-registered the pool. Keep it when the operator wants
        // all post-migration AMM traffic, OR when an unsettled real position needs
        // the feed for zero-RPC pool harvest + sell-confirm. Untracking a held mint
        // reintroduces the getTransaction cold burst on every AMM exit.
        if !track_post_migration && !self.held_pools.contains(&mint) {
            self.ingest_handle.untrack_pools(&[mint.clone()]);
        } else if self.held_pools.contains(&mint) {
            // Ensure the held set's subscribe path stays in sync if the auto-
            // register was raced by a clear_pools; track is idempotent.
            self.held_pools.track_migrated(&mint);
        }

        let metrics = self.token_cache.get_mut(&mint).map(|mut token_state| {
            token_state.is_migrated = true;
            metrics_from_state(&mint, &token_state)
        });
        if let Some(metrics) = metrics {
            self.enqueue_db_lossy(DbWriteOp::Metrics(metrics));
        }

        // Ping the strategy (re-routes any in-flight exit to the AMM) off the in-RAM
        // `is_migrated` flag set above — before the durable enqueue, so a DB hiccup
        // can't delay the re-route (H2).
        self.ping_strategy(mint.clone(), IngestKind::Migrated, None);
        self.enqueue_db_durable(DbWriteOp::Migration { mint });
    }

    fn on_creator_activity(&self, e: CreatorActivityEvent) {
        self.ping_strategy(e.mint.clone(), IngestKind::CreatorActivity, None);
    }

    fn on_liquidity(&self, e: LiquidityEvent) {
        let sse = if e.added {
            SseEvent::LiquidityAdded {
                mint_address: e.mint.clone(),
                wallet: e.wallet,
                amount_sol: e.amount_sol,
                token_amount: e.token_amount,
                tx_signature: e.signature,
                slot: e.slot,
                timestamp: e.block_time,
            }
        } else {
            SseEvent::LiquidityRemoved {
                mint_address: e.mint.clone(),
                wallet: e.wallet,
                amount_sol: e.amount_sol,
                token_amount: e.token_amount,
                tx_signature: e.signature,
                slot: e.slot,
                timestamp: e.block_time,
            }
        };
        self.emit_sse(sse);
    }

    // ── Policy transitions ─────────────────────────────────────────────────────

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

    fn clear_pools(&self) {
        // Preserve pools backing unsettled real positions — those are not optional
        // "record AMM history" subscriptions; exits depend on them.
        let held = self.held_pools.snapshot();
        let n = self.ingest_handle.pool_index().len();
        self.ingest_handle.pool_index().clear();
        if !held.is_empty() {
            self.ingest_handle.track_pools(&held);
        }
        self.ingest_handle.pools_changed().notify_one();
        info!(
            cleared = n,
            held = held.len(),
            "Tracking: post-migration disabled — cleared pools; kept {} held-position pool(s)",
            held.len()
        );
    }

    fn reseed_live_pools(&self) {
        let now = Utc::now();
        let mut mints_to_seed: Vec<String> = Vec::new();
        for entry in self.token_cache.iter() {
            if entry.is_migrated
                && (pool_is_live(entry.value(), now) || self.held_pools.contains(entry.key()))
            {
                mints_to_seed.push(entry.key().clone());
            }
        }
        // Held mints may not still be in the activity window / cache — keep them.
        for mint in self.held_pools.snapshot() {
            if !mints_to_seed.iter().any(|m| m == &mint) {
                mints_to_seed.push(mint);
            }
        }
        if !mints_to_seed.is_empty() {
            self.ingest_handle.track_pools(&mints_to_seed);
        }
        info!("Tracking: post-migration enabled — re-seeded {} live pool(s)", mints_to_seed.len());
    }

    // ── Sink helpers ───────────────────────────────────────────────────────────

    /// Enqueue a hot-path durable write without awaiting. Prefer `try_send` into
    /// `db_tx`; on full, defer into the bounded retry buffer (drained by a
    /// background task). Only when that buffer is also full is the write shed.
    /// Strategy ping + reserve update already ran before this call, so a
    /// defer/shed never delays an exit decision — only the durable trail.
    fn enqueue_db_durable(&self, op: DbWriteOp) {
        match self.db_tx.try_send(op) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(op)) => {
                match self.db_retry_tx.try_send(op) {
                    Ok(()) => {
                        let n = self.shed.db_deferred.fetch_add(1, Ordering::Relaxed) + 1;
                        warn!(
                            deferred_total = n,
                            "DbWriter backpressured — durable write deferred"
                        );
                    }
                    Err(mpsc::error::TrySendError::Full(_)) => {
                        self.shed.db_writes.fetch_add(1, Ordering::Relaxed);
                        warn!("DbWriter + retry buffer full — hot-path durable write shed");
                    }
                    Err(mpsc::error::TrySendError::Closed(_)) => {
                        self.shed.db_writes.fetch_add(1, Ordering::Relaxed);
                        warn!("DbWriter retry channel closed — write not persisted");
                    }
                }
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                warn!("DbWriter channel closed — write not persisted");
            }
        }
    }

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

    /// Ping the decision loop. A full queue sheds the ping — the engine is the
    /// slow consumer and must never back-pressure ingest.
    ///
    /// `TokenCreated` rides a dedicated lane so trade-volume saturation cannot
    /// drop a snipe. Shedding is LOUD by design. A wedged engine sheds 100% of
    /// pings while ingest keeps writing to PG, so every external signal
    /// (tokens/trades landing, feed healthy) looks normal while no rule is ever
    /// evaluated. That stayed invisible for 14 h on 2026-07-30 because this arm
    /// only bumped a counter nothing read. Never make it silent again.
    fn ping_strategy(&self, mint: String, kind: IngestKind, received_at: Option<chrono::DateTime<chrono::Utc>>) {
        let ping = StrategyPing { mint, kind, received_at };
        let (tx, shed_counter, queue_cap, lane) = match kind {
            IngestKind::TokenCreated => (
                &self.create_tx,
                &self.shed.create_pings,
                CREATE_STRATEGY_QUEUE_CAP,
                "create",
            ),
            _ => (
                &self.strategy_tx,
                &self.shed.strategy_pings,
                STRATEGY_QUEUE_CAP,
                "trade",
            ),
        };
        match tx.try_send(ping) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                let n = shed_counter.fetch_add(1, Ordering::Relaxed) + 1;
                if n == 1 || n.is_multiple_of(STRATEGY_SHED_LOG_EVERY) {
                    warn!(
                        shed_total = n,
                        queue_cap,
                        lane,
                        "strategy ping queue full — engine is not consuming; \
                         rules are NOT being evaluated"
                    );
                }
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                warn!(lane, "Strategy channel closed — ping not delivered");
            }
        }
    }

    fn emit_sse(&self, event: SseEvent) {
        if self.sse_tx.receiver_count() == 0 {
            return;
        }
        let _ = self.sse_tx.send(event);
    }
}

// ── Translators ───────────────────────────────────────────────────────────────

fn token_from_event(e: TokenCreated) -> Token {
    Token {
        id: Uuid::new_v4(),
        mint_address: e.mint,
        creator_wallet: e.creator,
        name: e.name,
        symbol: e.symbol,
        token_program_id: e.token_program_id,
        bonding_curve_address: e.bonding_curve,
        initial_supply_token: e.initial_supply,
        initial_buy_sol: e.initial_buy_sol,
        initial_buy_instruction: e.initial_buy_instruction.as_ref().map(buy_ix_to_json),
        cu_limit: e.cu_limit,
        cu_price: e.cu_price,
        is_mayhem_mode: e.is_mayhem_mode,
        is_cashback_enabled: e.is_cashback_enabled,
        instruction_labels: labels_to_json(&e.instruction_labels),
        creation_tx_signature: e.signature,
        creation_slot: Some(e.slot),
        first_slot_buy_sol: None,
        first_slot_sell_sol: None,
        meta: create_meta(e.uri.as_deref()),
        created_at: e.block_time,
    }
}

fn trade_from_event(e: &IlTrade) -> Trade {
    Trade {
        id: Uuid::new_v4(),
        mint_address: e.mint.clone(),
        wallet_address: e.wallet.clone(),
        trade_type: trade_type(e.side),
        amount_sol: e.sol,
        token_amount: e.tokens,
        price_per_token: e.price,
        // Per-TRANSACTION network fee, repeated on every leg the tx produced —
        // see `Trade::fee_sol`. `None` when the source carried no fee.
        fee_sol: e
            .fee_lamports
            .map(|l| trading_core::config::constants::lamports_to_sol(l as i64)),
        tx_signature: e.signature.clone(),
        tx_index: e.tx_index,
        leg_index: e.leg_index,
        slot: e.slot,
        block_time: e.block_time,
        received_at: e.received_at,
        reserve_sol: e.reserves.virtual_sol,
        reserve_token: e.reserves.virtual_token,
        real_reserve_sol: e.reserves.real_sol,
        real_token_reserves: e.reserves.real_token,
        instruction_type: e.instruction_type.clone(),
        instruction_labels: Value::Null,
        venue: venue_str(e.venue).to_string(),
    }
}

fn trade_type(side: Side) -> TradeType {
    match side {
        Side::Buy => TradeType::Buy,
        Side::Sell => TradeType::Sell,
    }
}

fn venue_str(venue: Venue) -> &'static str {
    match venue {
        Venue::Curve => "curve",
        Venue::Amm => "amm",
    }
}

fn labels_to_json(labels: &[String]) -> Value {
    json!(labels)
}

fn buy_ix_to_json(args: &BuyInstructionArgs) -> Value {
    match args {
        BuyInstructionArgs::Buy { token_amount, max_sol_cost } => json!({
            "type": "Buy",
            "token_amount": token_amount,
            "max_cost_lamports": max_sol_cost,
        }),
        BuyInstructionArgs::BuyV2 { token_amount, max_sol_cost } => json!({
            "type": "BuyV2",
            "token_amount": token_amount,
            "max_cost_lamports": max_sol_cost,
        }),
        BuyInstructionArgs::BuyExactSolIn { spendable_sol_in, min_tokens_out } => json!({
            "type": "BuyExactSolIn",
            "spendable_lamports_in": spendable_sol_in,
            "min_tokens_out": min_tokens_out,
        }),
        BuyInstructionArgs::BuyExactQuoteIn { spendable_sol_in, min_tokens_out } => json!({
            "type": "BuyExactQuoteIn",
            "spendable_lamports_in": spendable_sol_in,
            "min_tokens_out": min_tokens_out,
        }),
        BuyInstructionArgs::BuyExactQuoteInV2 { spendable_sol_in, min_tokens_out } => json!({
            "type": "BuyExactQuoteInV2",
            "spendable_lamports_in": spendable_sol_in,
            "min_tokens_out": min_tokens_out,
        }),
    }
}

fn pool_is_live(state: &TokenState, now: chrono::DateTime<chrono::Utc>) -> bool {
    match state.last_trade_at {
        Some(last) => {
            let age_secs = (now - last).num_seconds();
            age_secs <= POOL_SUBSCRIBE_ACTIVITY_WINDOW_SECONDS as i64
        }
        None => false,
    }
}
