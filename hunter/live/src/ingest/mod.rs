//! Host adapter for the `ingest-laserstream` crate.
//!
//! Builds the [`Ingest`] session, starts it, spawns the consumer + DB-writer
//! tasks, and starts the watchdog OS thread. The composition root (`main.rs`)
//! calls [`spawn_ingest`] once and gets the handles it needs to drive the
//! supervising `tokio::select!`.

pub mod consumer;
pub mod db_writer;
pub mod held_pools;
pub mod watchdog;

pub use held_pools::HeldPoolGate;

use std::sync::Arc;

use sqlx::PgPool;
use tokio::sync::{broadcast, mpsc, watch, Notify};
use tokio::task::JoinHandle;

use ingest_laserstream::{Ingest, IngestConfig, IngestHandle, PoolIndex, Protocol, PushHooks};

use trading_core::{
    models::ingest::{SseEvent, StrategyPing},
    state::trade_signals::TradeSignals,
    state::token_cache::TokenCache,
    storage::repositories::settings_repo::AppSettings,
};

use trading_core::ingest::TraderHook;

use consumer::{IngestConsumer, DB_QUEUE_CAP, DB_RETRY_CAP, STRATEGY_QUEUE_CAP};
use db_writer::DbWriter;
use watchdog::{DbHeartbeat, spawn_watchdog};

pub struct IngestSpawnResult {
    pub pool_index: PoolIndex,
    pub pools_changed: Arc<Notify>,
    pub strategy_rx: mpsc::Receiver<StrategyPing>,
    pub consumer_task: JoinHandle<()>,
    pub db_writer_task: JoinHandle<()>,
    pub ingest_handle: Arc<IngestHandle>,
    /// Retains AMM pool subscriptions for unsettled real positions.
    pub held_pools: HeldPoolGate,
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
    push_hooks: PushHooks,
) -> IngestSpawnResult {
    let (strategy_tx, strategy_rx) =
        mpsc::channel::<StrategyPing>(STRATEGY_QUEUE_CAP);

    let (db_tx, db_rx) =
        mpsc::channel::<db_writer::DbWriteOp>(DB_QUEUE_CAP);
    let (db_retry_tx, mut db_retry_rx) =
        mpsc::channel::<db_writer::DbWriteOp>(DB_RETRY_CAP);

    let live = *live_rx.borrow();

    let (event_rx, handle) = Ingest::builder()
        .endpoint(endpoint)
        .api_key(api_key)
        .protocol(Protocol::pump_fun())
        .config(IngestConfig::default())
        .push_hooks(push_hooks)
        .build()
        .expect("ingest builder failed")
        .start(live);

    let handle = Arc::new(handle);
    let held_pools = HeldPoolGate::new(handle.clone(), settings_rx.clone());

    let heartbeat = DbHeartbeat::new();

    let (consumer, _shed) = IngestConsumer::new(
        token_cache,
        db_tx.clone(),
        db_retry_tx,
        strategy_tx,
        sse_tx,
        settings_rx.clone(),
        trader,
        trade_signals.clone(),
        handle.clone(),
        held_pools.clone(),
    );

    let db_writer = DbWriter::new(db, trade_signals, heartbeat.clone());

    let consumer_task = tokio::spawn(consumer.run(event_rx));
    let db_writer_task = tokio::spawn(db_writer.run(db_rx));

    // Drain durable writes that timed out on the hot path into `db_tx` without
    // blocking the ingest loop. `send().await` applies backpressure here so rows
    // are retained across a PG stall until the writer catches up (or the channel
    // closes on shutdown).
    {
        let db_tx = db_tx.clone();
        tokio::spawn(async move {
            while let Some(op) = db_retry_rx.recv().await {
                if db_tx.send(op).await.is_err() {
                    break;
                }
            }
        });
    }

    let db_tx_weak = db_tx.downgrade();
    spawn_watchdog(
        heartbeat,
        live_rx,
        settings_rx.clone(),
        move || {
            db_tx_weak
                .upgrade()
                .map(|tx| tx.capacity() < tx.max_capacity())
                .unwrap_or(false)
        },
    );

    // Forward gap-replay settings to the ingest transport whenever the operator
    // changes them via the settings page. Reads the current value immediately so
    // the initial state is always applied before the first reconnect.
    {
        let h = handle.clone();
        let mut s_rx = settings_rx.clone();
        tokio::spawn(async move {
            loop {
                {
                    let s = s_rx.borrow_and_update();
                    h.set_gap_replay(s.gap_replay_on_reconnect, s.gap_replay_max_window_secs);
                }
                if s_rx.changed().await.is_err() {
                    break;
                }
            }
        });
    }

    let pool_index = handle.pool_index();
    let pools_changed = handle.pools_changed();

    IngestSpawnResult {
        pool_index,
        pools_changed,
        strategy_rx,
        consumer_task,
        db_writer_task,
        ingest_handle: handle,
        held_pools,
    }
}
