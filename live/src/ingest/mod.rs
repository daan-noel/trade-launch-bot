//! Host adapter for the `ingest-laserstream` crate.
//!
//! Builds the [`Ingest`] session, starts it, spawns the consumer + DB-writer
//! tasks, and starts the watchdog OS thread. The composition root (`main.rs`)
//! calls [`spawn_ingest`] once and gets the handles it needs to drive the
//! supervising `tokio::select!`.

pub mod consumer;
pub mod db_writer;
pub mod watchdog;

use std::sync::Arc;

use sqlx::PgPool;
use tokio::sync::{broadcast, mpsc, watch, Notify};
use tokio::task::JoinHandle;

use ingest_laserstream::{Ingest, IngestConfig, IngestHandle, PoolIndex, Protocol};

use trading_core::{
    models::ingest::{SseEvent, StrategyPing},
    state::trade_signals::TradeSignals,
    state::token_cache::TokenCache,
    storage::repositories::settings_repo::AppSettings,
};

use trading_core::ingest::TraderHook;

use consumer::{IngestConsumer, DB_QUEUE_CAP, STRATEGY_QUEUE_CAP};
use db_writer::DbWriter;
use watchdog::{DbHeartbeat, spawn_watchdog};

pub struct IngestSpawnResult {
    pub pool_index: PoolIndex,
    pub pools_changed: Arc<Notify>,
    pub strategy_rx: mpsc::Receiver<StrategyPing>,
    pub consumer_task: JoinHandle<()>,
    pub db_writer_task: JoinHandle<()>,
    pub ingest_handle: Arc<IngestHandle>,
}

#[allow(clippy::too_many_arguments)]
pub async fn spawn_ingest(
    endpoint: String,
    api_key: String,
    db: PgPool,
    token_cache: Arc<TokenCache>,
    sse_tx: broadcast::Sender<SseEvent>,
    settings_rx: watch::Receiver<AppSettings>,
    live_rx: watch::Receiver<bool>,
    trader: Arc<dyn TraderHook>,
    trade_signals: Arc<TradeSignals>,
) -> IngestSpawnResult {
    let (strategy_tx, strategy_rx) =
        mpsc::channel::<StrategyPing>(STRATEGY_QUEUE_CAP);

    let (db_tx, db_rx) =
        mpsc::channel::<db_writer::DbWriteOp>(DB_QUEUE_CAP);

    let live = *live_rx.borrow();

    let (event_rx, handle) = Ingest::builder()
        .endpoint(endpoint)
        .api_key(api_key)
        .protocol(Protocol::pump_fun())
        .config(IngestConfig::default())
        .build()
        .expect("ingest builder failed")
        .start(live);

    let handle = Arc::new(handle);

    let heartbeat = DbHeartbeat::new();

    let (consumer, _shed) = IngestConsumer::new(
        token_cache,
        db_tx.clone(),
        strategy_tx,
        sse_tx,
        settings_rx.clone(),
        trader,
        trade_signals.clone(),
        handle.clone(),
    );

    let db_writer = DbWriter::new(db, trade_signals, heartbeat.clone());

    let consumer_task = tokio::spawn(consumer.run(event_rx));
    let db_writer_task = tokio::spawn(db_writer.run(db_rx));

    let db_tx_weak = db_tx.downgrade();
    spawn_watchdog(
        heartbeat,
        live_rx,
        settings_rx,
        move || {
            db_tx_weak
                .upgrade()
                .map(|tx| tx.capacity() < tx.max_capacity())
                .unwrap_or(false)
        },
    );

    let pool_index = handle.pool_index();
    let pools_changed = handle.pools_changed();

    IngestSpawnResult {
        pool_index,
        pools_changed,
        strategy_rx,
        consumer_task,
        db_writer_task,
        ingest_handle: handle,
    }
}
