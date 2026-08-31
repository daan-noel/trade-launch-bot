//! Host adapter for the `ingest-pumpfun` read-stack.
//!
//! Builds the [`Ingest`] session, starts it, spawns the consumer + DB-writer
//! tasks, and starts the watchdog OS thread. The composition root (`main.rs`)
//! calls [`spawn_ingest`] once and gets the handles it needs to drive the
//! supervising `tokio::select!`.

pub mod consumer;
pub mod db_writer;
pub mod feed_lag;
pub mod held_pools;
pub mod pool_reconcile;
pub mod watchdog;

pub use held_pools::HeldPoolGate;

use std::sync::Arc;

use sqlx::PgPool;
use tokio::sync::{broadcast, mpsc, watch, Notify};
use tokio::task::JoinHandle;

use ingest_pumpfun::{
    FeedKind, Ingest, IngestConfig, IngestHandle, NatsConfig, PoolIndex, Protocol, PushHooks,
};

use trading_core::{
    models::ingest::{SseEvent, StrategyPing},
    state::trade_signals::TradeSignals,
    state::token_cache::TokenCache,
    storage::repositories::settings_repo::AppSettings,
};

use trading_core::ingest::TraderHook;

use consumer::{
    IngestConsumer, CREATE_STRATEGY_QUEUE_CAP, DB_QUEUE_CAP, DB_RETRY_CAP, STRATEGY_QUEUE_CAP,
};
use db_writer::DbWriter;
use watchdog::{DbHeartbeat, spawn_watchdog};

pub use watchdog::BootGate;

/// Map the operator's `ingest.curve_source` string onto a feed.
///
/// An unknown value, or `"nats"` with no relay configured, falls back to gRPC —
/// the curve must never end up pointed at a feed that cannot run.
fn curve_feed_of(settings: &AppSettings, has_nats: bool) -> FeedKind {
    match settings.curve_source.trim().to_ascii_lowercase().as_str() {
        "nats" if has_nats => FeedKind::Nats,
        "nats" => {
            tracing::warn!(
                "ingest: curve_source=nats but NATS_URL is unset - staying on LaserStream"
            );
            FeedKind::Grpc
        }
        "grpc" | "" => FeedKind::Grpc,
        other => {
            tracing::warn!("ingest: unknown curve_source {other:?} - using grpc");
            FeedKind::Grpc
        }
    }
}

pub struct IngestSpawnResult {
    pub pool_index: PoolIndex,
    pub pools_changed: Arc<Notify>,
    /// Trade / migrate / creator-activity strategy pings.
    pub strategy_rx: mpsc::Receiver<StrategyPing>,
    /// `TokenCreated` strategy pings (create fast lane).
    pub create_rx: mpsc::Receiver<StrategyPing>,
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
    // NATS relay for the bonding curve. Empty => the transport is never built and
    // the curve stays on LaserStream regardless of the operator setting.
    nats_url: String,
    nats_subject: String,
    db: PgPool,
    token_cache: Arc<TokenCache>,
    sse_tx: broadcast::Sender<SseEvent>,
    settings_rx: watch::Receiver<AppSettings>,
    live_rx: watch::Receiver<bool>,
    trader: Arc<dyn TraderHook>,
    trade_signals: Arc<TradeSignals>,
    push_hooks: PushHooks,
    trading_wallet: String,
    // Set by the engine loop once startup finishes — the watchdog only polices
    // the steady state (see `BootGate`).
    boot_gate: BootGate,
) -> IngestSpawnResult {
    let (strategy_tx, strategy_rx) =
        mpsc::channel::<StrategyPing>(STRATEGY_QUEUE_CAP);
    let (create_tx, create_rx) =
        mpsc::channel::<StrategyPing>(CREATE_STRATEGY_QUEUE_CAP);

    let (db_tx, db_rx) =
        mpsc::channel::<db_writer::DbWriteOp>(DB_QUEUE_CAP);
    let (db_retry_tx, mut db_retry_rx) =
        mpsc::channel::<db_writer::DbWriteOp>(DB_RETRY_CAP);

    let live = *live_rx.borrow();

    // The NATS feed is built whenever a relay is configured, even when the
    // operator has the curve on gRPC: it then idles disconnected, which is what
    // makes `set_curve_feed` an instant switch instead of a restart.
    let nats = (!nats_url.trim().is_empty()).then(|| NatsConfig {
        url: nats_url.trim().to_string(),
        subject: nats_subject.trim().to_string(),
        ..NatsConfig::default()
    });
    let initial_curve = curve_feed_of(&settings_rx.borrow(), nats.is_some());

    let (event_rx, handle) = Ingest::builder()
        .endpoint(endpoint)
        .api_key(api_key)
        .nats(nats)
        .curve_feed(initial_curve)
        .protocol(Protocol::pump_fun())
        .config(IngestConfig::default())
        .push_hooks(push_hooks)
        .build()
        .expect("ingest builder failed")
        .start(live);

    let handle = Arc::new(handle);
    let held_pools = HeldPoolGate::new(handle.clone(), settings_rx.clone());

    // Cloned before the consumer / DB writer take ownership below — the pool
    // reconciler needs the same cache and the same pool.
    let token_cache_for_pools = token_cache.clone();
    let db_for_pools = db.clone();

    let heartbeat = DbHeartbeat::new();

    let (consumer, _shed) = IngestConsumer::new(
        token_cache,
        db_tx.clone(),
        db_retry_tx,
        strategy_tx,
        create_tx,
        sse_tx,
        settings_rx.clone(),
        trader,
        trade_signals.clone(),
        handle.clone(),
        held_pools.clone(),
        trading_wallet,
    );

    let db_writer = DbWriter::new(db, trade_signals, heartbeat.clone());

    let consumer_task = tokio::spawn(consumer.run(event_rx));
    let db_writer_task = tokio::spawn(db_writer.run(db_rx));

    // Drain durable writes that could not enter `db_tx` without blocking into
    // `db_tx` without blocking the ingest loop. `send().await` applies
    // backpressure here so rows are retained across a PG stall until the writer
    // catches up (or the channel closes on shutdown).
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

    // The watchdog trips on "live but no successful DB write within the timeout" —
    // not gated on db_tx queue depth (that proxy misses upstream stalls).
    spawn_watchdog(heartbeat, live_rx, settings_rx.clone(), boot_gate);

    // The one owner of the tracked AMM pool set: re-derives the held mints from
    // `strategy_positions` and moves the subscription to match. It both SUBSCRIBES
    // a bag whose pool nothing tracked (exit-path fix) and drops pools no position
    // or setting asks for (the metered-filter fix). See `pool_reconcile`.
    pool_reconcile::spawn_pool_reconciler(
        handle.clone(),
        held_pools.clone(),
        token_cache_for_pools,
        trading_core::storage::repositories::strategy_repo::StrategyRepo::new(db_for_pools),
        settings_rx.clone(),
    );

    // Forward gap-replay settings to the ingest transport whenever the operator
    // changes them via the settings page. Reads the current value immediately so
    // the initial state is always applied before the first reconnect.
    {
        let h = handle.clone();
        let has_nats = !nats_url.trim().is_empty();
        let mut s_rx = settings_rx.clone();
        tokio::spawn(async move {
            loop {
                {
                    let s = s_rx.borrow_and_update();
                    h.set_gap_replay(s.gap_replay_on_reconnect, s.gap_replay_max_window_secs);
                    // The same watch carries the curve-feed switch, so flipping
                    // it on the Settings page re-points the curve with no restart.
                    h.set_curve_feed(curve_feed_of(&s, has_nats));
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
        create_rx,
        consumer_task,
        db_writer_task,
        ingest_handle: handle,
        held_pools,
    }
}
