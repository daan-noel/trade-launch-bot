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
        token::Token,
        trade::{Trade, TradeType},
    },
    state::token_cache::{TokenCache, TokenState},
    state::token_metrics::metrics_from_state,
    state::trade_signals::TradeSignals,
    storage::repositories::settings_repo::AppSettings,
};

use trading_core::ingest::TraderHook;

use super::db_writer::DbWriteOp;

pub const DB_QUEUE_CAP: usize = 16384;
pub const STRATEGY_QUEUE_CAP: usize = 512;

/// Counts of intentionally-shed messages under back-pressure.
#[derive(Default)]
pub struct ShedCounters {
    pub strategy_pings: AtomicU64,
    pub db_writes: AtomicU64,
}

pub struct IngestConsumer {
    token_cache: Arc<TokenCache>,
    db_tx: mpsc::Sender<DbWriteOp>,
    strategy_tx: mpsc::Sender<StrategyPing>,
    sse_tx: broadcast::Sender<SseEvent>,
    settings_rx: watch::Receiver<AppSettings>,
    trader: Arc<dyn TraderHook>,
    trade_signals: Arc<TradeSignals>,
    ingest_handle: Arc<IngestHandle>,
    shed: Arc<ShedCounters>,
}

impl IngestConsumer {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        token_cache: Arc<TokenCache>,
        db_tx: mpsc::Sender<DbWriteOp>,
        strategy_tx: mpsc::Sender<StrategyPing>,
        sse_tx: broadcast::Sender<SseEvent>,
        settings_rx: watch::Receiver<AppSettings>,
        trader: Arc<dyn TraderHook>,
        trade_signals: Arc<TradeSignals>,
        ingest_handle: Arc<IngestHandle>,
    ) -> (Self, Arc<ShedCounters>) {
        let shed = Arc::new(ShedCounters::default());
        (
            Self {
                token_cache,
                db_tx,
                strategy_tx,
                sse_tx,
                settings_rx,
                trader,
                trade_signals,
                ingest_handle,
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
        let token = token_from_event(e);
        self.enqueue_db(DbWriteOp::Token(token.clone())).await;
        self.enqueue_db(DbWriteOp::Wallet(creator)).await;

        let token_state = TokenState::new(token);
        let metrics = metrics_from_state(&mint, &token_state);
        self.token_cache.insert(mint.clone(), token_state);
        self.enqueue_db_lossy(DbWriteOp::Metrics(metrics));

        self.ping_strategy(mint.clone(), IngestKind::TokenCreated);
        self.emit_sse(SseEvent::TokenCreated {
            mint,
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
        let db_trade = {
            let mut t = core_trade.clone();
            t.instruction_labels = labels_json;
            t
        };
        core_trade.instruction_labels = Value::Null;

        self.enqueue_db(DbWriteOp::Trade(db_trade)).await;
        self.enqueue_db(DbWriteOp::Wallet(wallet.clone())).await;

        let (metrics, to_warm) = match self.token_cache.get_mut(&mint) {
            Some(mut token_state) => {
                token_state.add_trade(core_trade);
                let metrics = metrics_from_state(&mint, &token_state);
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

        self.trade_signals.notify_mint(&mint);

        if let Some(metrics) = metrics {
            self.enqueue_db_lossy(DbWriteOp::Metrics(metrics));
        }

        if let Some((token_reserves, sol_reserves)) = reserve_snapshot {
            // The trader takes reserves as `f64` (spot-price ratio math; it's a
            // standalone lib). Cast the raw token reserves at this boundary.
            self.trader.update_live_reserves(&mint, token_reserves as f64, sol_reserves, is_amm);
        }

        if let Some(token_program) = to_warm {
            let trader = self.trader.clone();
            let token_cache = self.token_cache.clone();
            let warm_mint = mint.clone();
            tokio::spawn(async move {
                if let Err(err) = trader.prewarm_amm_pool(&warm_mint, &token_program).await {
                    debug!("AMM pool prewarm failed for {warm_mint}: {err}");
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
            amount_sol,
            token_amount,
            price_per_token,
            tx_signature: e.signature,
            slot: e.slot,
            timestamp: e.block_time,
        });
    }

    async fn on_token_migrated(&self, e: TokenMigrated, track_post_migration: bool) {
        let mint = e.mint.clone();

        // The decode task auto-registered the pool; undo if tracking is off.
        if !track_post_migration {
            self.ingest_handle.untrack_pools(&[mint.clone()]);
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
        self.ping_strategy(e.mint.clone(), IngestKind::CreatorActivity);
    }

    fn on_liquidity(&self, e: LiquidityEvent) {
        let sse = if e.added {
            SseEvent::LiquidityAdded {
                mint: e.mint.clone(),
                wallet: e.wallet,
                amount_sol: e.amount_sol,
                token_amount: e.token_amount,
                tx_signature: e.signature,
                slot: e.slot,
                timestamp: e.block_time,
            }
        } else {
            SseEvent::LiquidityRemoved {
                mint: e.mint.clone(),
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
        let n = self.ingest_handle.pool_index().len();
        self.ingest_handle.pool_index().clear();
        self.ingest_handle.pools_changed().notify_one();
        info!("Tracking: post-migration disabled — cleared {n} pool(s); AMM trades no longer recorded");
    }

    fn reseed_live_pools(&self) {
        let now = Utc::now();
        let mut mints_to_seed: Vec<String> = Vec::new();
        for entry in self.token_cache.iter() {
            if entry.is_migrated && pool_is_live(entry.value(), now) {
                mints_to_seed.push(entry.key().clone());
            }
        }
        if !mints_to_seed.is_empty() {
            self.ingest_handle.track_pools(&mints_to_seed);
        }
        info!("Tracking: post-migration enabled — re-seeded {} live pool(s)", mints_to_seed.len());
    }

    // ── Sink helpers ───────────────────────────────────────────────────────────

    async fn enqueue_db(&self, op: DbWriteOp) {
        if let Err(e) = self.db_tx.send(op).await {
            warn!("DbWriter channel closed — write not persisted: {e}");
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
