use std::sync::Arc;

use chrono::Utc;
use dashmap::DashMap;
use tokio::sync::{broadcast, mpsc, Notify};
use tracing::{debug, info, warn};

use crate::{
    ingest::{
        db_writer::{DbWriteOp, TokenMetricsWrite},
        decoder::{DecodeOutput, HeliusDecoder},
    },
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
};

const DB_QUEUE_CAP: usize = 4096;
const STRATEGY_QUEUE_CAP: usize = 512;

/// Single hot-path owner: decode → filter → order → cache → queue DB → ping strategy / SSE.
pub struct IngestPipeline {
    decoder: HeliusDecoder,
    token_cache: Arc<TokenCache>,
    db_tx: mpsc::Sender<DbWriteOp>,
    strategy_tx: mpsc::Sender<StrategyPing>,
    sse_tx: broadcast::Sender<SseEvent>,
    pump_program_id: String,
    /// pool → mint index shared with the decoder (to attribute live PumpSwap
    /// swaps, which carry the pool but not the base mint) and with the WS task
    /// (to drive per-pool subscriptions). Holds migrated tokens' pools.
    pool_index: Arc<DashMap<String, String>>,
    /// Pinged whenever a token migrates and a new pool is added to `pool_index`,
    /// so the WS task subscribes to it without dropping the connection.
    pools_changed: Arc<Notify>,
}

impl IngestPipeline {
    pub fn new(
        pump_program_id: String,
        token_cache: Arc<TokenCache>,
        db_tx: mpsc::Sender<DbWriteOp>,
        strategy_tx: mpsc::Sender<StrategyPing>,
        sse_tx: broadcast::Sender<SseEvent>,
    ) -> Self {
        // Seed the pool→mint index for tokens already migrated before this
        // process started, so live AMM swaps for them are captured immediately.
        // Tokens created/migrating during this session register their pool
        // lazily (see `register_pool`).
        let pool_index: Arc<DashMap<String, String>> = Arc::new(DashMap::new());
        for entry in token_cache.iter() {
            if entry.is_migrated {
                if let Ok(pool) = derive_pump_swap_pool(entry.key(), &pump_program_id) {
                    pool_index.insert(pool, entry.key().clone());
                }
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
        }
    }

    /// Shared pool → mint index, for the WS task's per-pool subscriptions.
    pub fn pool_index(&self) -> Arc<DashMap<String, String>> {
        self.pool_index.clone()
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

    pub async fn run(self, mut raw_rx: mpsc::Receiver<String>) {
        info!("IngestPipeline: starting");

        while let Some(raw) = raw_rx.recv().await {
            match self.decoder.decode(&raw) {
                DecodeOutput::Subscribed(id) => {
                    info!("Helius subscription confirmed — id={id}");
                }
                DecodeOutput::Transaction { raw_tx, mut events } => {
                    sort_events(&mut events);
                    let (events, save_raw) = filter_events(events, &self.token_cache);
                    if events.is_empty() {
                        continue;
                    }

                    if save_raw {
                        self.enqueue_db(DbWriteOp::Raw(raw_tx.clone()));
                    }

                    for event in events {
                        self.apply_event(event, &raw_tx).await;
                    }
                }
                DecodeOutput::Ignored => {}
            }
        }

        info!("IngestPipeline: raw_rx closed — stopping");
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

    fn enqueue_db(&self, op: DbWriteOp) {
        if let Err(e) = self.db_tx.try_send(op) {
            warn!("DbWriter queue full — dropping write: {e}");
        }
    }

    fn ping_strategy(&self, mint: String, kind: IngestKind) {
        if let Err(e) = self.strategy_tx.try_send(StrategyPing { mint, kind }) {
            warn!("Strategy queue full — dropping ping: {e}");
        }
    }

    fn emit_sse(&self, event: SseEvent) {
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

        info!(
            name = %e.token.name,
            symbol = %e.token.symbol,
            mint = %mint,
            "New token tracked"
        );

        let creator = e.token.creator_wallet.clone();
        self.enqueue_db(DbWriteOp::Token(e.token.clone()));
        self.enqueue_db(DbWriteOp::Wallet(creator));

        let token_state = TokenState::new(e.token);
        let metrics = metrics_from_state(&mint, &token_state, false);
        self.token_cache.insert(mint.clone(), token_state);
        self.enqueue_db(DbWriteOp::Metrics(metrics));

        self.ping_strategy(mint.clone(), IngestKind::TokenCreated);
        self.emit_sse(SseEvent::TokenCreated {
            mint,
            tx_signature: e.tx_signature,
            slot: e.slot,
            timestamp: e.timestamp,
        });
    }

    async fn on_trade_executed(&self, e: TradeExecutedEvent) {
        let mint = e.trade.mint_address.clone();
        let wallet = e.trade.wallet_address.clone();

        if !self.token_cache.contains_key(&mint) {
            return;
        }

        debug!(
            mint = %mint,
            wallet = %wallet,
            kind = ?e.trade.trade_type,
            "Trade applied"
        );

        self.enqueue_db(DbWriteOp::Trade(e.trade.clone()));
        self.enqueue_db(DbWriteOp::Wallet(wallet.clone()));

        if let Some(mut token_state) = self.token_cache.get_mut(&mint) {
            token_state.add_trade(e.trade.clone());
            let metrics = metrics_from_state(&mint, &token_state, true);
            drop(token_state);
            self.enqueue_db(DbWriteOp::Metrics(metrics));
        }

        self.ping_strategy(mint.clone(), IngestKind::Trade);
        self.emit_sse(SseEvent::TradeExecuted {
            mint,
            wallet,
            trade_type: e.trade.trade_type,
            sol_amount: e.trade.sol_amount,
            token_amount: e.trade.token_amount,
            price_per_token: e.trade.price_per_token,
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
        // attribute them and the WS task subscribes to this pool.
        self.register_pool(&mint);

        if let Some(mut token_state) = self.token_cache.get_mut(&mint) {
            token_state.is_migrated = true;
            let metrics = metrics_from_state(&mint, &token_state, false);
            drop(token_state);
            self.enqueue_db(DbWriteOp::Metrics(metrics));
        }

        self.enqueue_db(DbWriteOp::Migration { mint: mint.clone() });
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
) -> (Vec<InternalEvent>, bool) {
    let mut out = Vec::new();
    let mut save_raw = false;

    for event in events {
        match &event {
            InternalEvent::TokenCreated(_) => {
                save_raw = true;
                out.push(event);
            }
            InternalEvent::TradeExecuted(e) => {
                if cache.contains_key(&e.trade.mint_address) {
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

fn metrics_from_state(mint: &str, state: &TokenState, recompute_rugged: bool) -> TokenMetricsWrite {
    let age_seconds = Utc::now()
        .signed_duration_since(state.token.created_at)
        .num_seconds();
    TokenMetricsWrite {
        mint: mint.to_string(),
        ath_price: state.ath_price,
        ath_timestamp: state.ath_timestamp,
        age_seconds: Some(age_seconds as i64),
        volume: state.volume_sol_total,
        market_cap: state.market_cap,
        trade_count: state.trade_count as i64,
        last_trade_at: state.last_trade_at,
        current_price: state.current_price,
        is_migrated: state.is_migrated,
        creator_wallet: state.token.creator_wallet.clone(),
        recompute_rugged,
    }
}
