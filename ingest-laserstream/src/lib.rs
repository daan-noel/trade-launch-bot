//! LaserStream (Yellowstone gRPC) ingest transport.
//!
//! Committed prost/tonic bindings generated from `proto/geyser.proto` (+
//! solana-storage.proto). There is no build-time protoc step; regenerate via the
//! documented Docker one-shot only if the `.proto` files change.
#![allow(dead_code)]

#[allow(clippy::all)]
#[rustfmt::skip]
pub mod proto {
    include!("generated/mod.rs");
}

pub mod adapter;
pub mod adapter_rpc;
pub mod client;
pub mod db_writer;
pub mod decoder;
pub mod ingest_health;
pub mod pipeline;

// The transport-agnostic ingest contract lives in `trading_core::ingest`.
// Re-export so existing `ingest_laserstream::{IngestHandles, TraderHook}` paths
// keep resolving and the websocket transport returns the same `IngestHandles`.
pub use trading_core::ingest::{IngestHandles, TraderHook};

use std::sync::Arc;

use tokio::sync::watch;
use tracing::info;

use trading_core::{
    models::ingest::SseEvent,
    state::{token_cache::TokenCache, trade_signals::TradeSignals},
    storage::repositories::settings_repo::AppSettings,
};

/// Spawn all ingest tasks and return handles for the supervising `select!`.
///
/// - `helius_laserstream_url` / `helius_api_key` — Helius credentials.
/// - `pump_program_id` — pump.fun bonding-curve program address.
/// - `db` — hot-path Postgres pool.
/// - `token_cache` — shared live token state.
/// - `sse_tx` — SSE broadcast channel.
/// - `settings_rx` — live settings watch channel.
/// - `live_rx` — watch receiver; consumed by the client (pauses on `false`).
/// - `trader` — TraderHook impl for reserve updates + pool pre-warming.
/// - `trade_signals` — (wallet, mint) wakeup hub for confirm loops.
#[allow(clippy::too_many_arguments)]
pub fn spawn(
    helius_laserstream_url: String,
    helius_api_key: String,
    pump_program_id: String,
    db: sqlx::PgPool,
    token_cache: Arc<TokenCache>,
    sse_tx: tokio::sync::broadcast::Sender<SseEvent>,
    settings_rx: watch::Receiver<AppSettings>,
    live_rx: watch::Receiver<bool>,
    trader: Arc<dyn TraderHook>,
    trade_signals: Arc<TradeSignals>,
) -> IngestHandles {
    info!("Ingest transport: LaserStream (gRPC)");

    let (db_tx, db_rx, strategy_tx, strategy_rx) =
        pipeline::IngestPipeline::channel_pair();

    // Tier B: typed protobuf update (shared via Arc), not a pre-built Value.
    // Buffer sized to absorb short volume bursts without stalling the gRPC socket.
    let (update_tx, update_rx) = tokio::sync::mpsc::channel::<(
        Arc<proto::geyser::SubscribeUpdateTransaction>,
        decoder::TxRelevance,
    )>(4096);

    // Weak sender handles for the queue-depth diagnostics logger.
    let update_tx_weak = update_tx.downgrade();
    let db_tx_weak = db_tx.downgrade();
    let strategy_tx_weak = strategy_tx.downgrade();

    let ingest_pipeline = pipeline::IngestPipeline::new(
        pump_program_id.clone(),
        token_cache.clone(),
        db_tx,
        strategy_tx,
        sse_tx,
        settings_rx.clone(),
        trader,
        trade_signals.clone(),
    );
    let pool_index = ingest_pipeline.pool_index();
    let pools_changed = ingest_pipeline.pools_changed();
    let shed_counters = ingest_pipeline.shed_counters();

    // End-to-end ingest liveness heartbeat + watchdog.
    let heartbeat = ingest_health::IngestHeartbeat::new();
    let db_tx_pending = db_tx_weak.clone();
    ingest_health::spawn_watchdog(
        heartbeat.clone(),
        live_rx.clone(),
        settings_rx.clone(),
        move || {
            db_tx_pending
                .upgrade()
                .map(|tx| tx.capacity() < tx.max_capacity())
                .unwrap_or(false)
        },
    );

    let producer_task = tokio::spawn(client::run(
        helius_laserstream_url,
        helius_api_key,
        pump_program_id.clone(),
        update_tx,
        live_rx,
        pool_index.clone(),
        pools_changed.clone(),
    ));

    tokio::spawn(pipeline::run_pool_subscription_refresh(
        token_cache,
        pool_index.clone(),
        pools_changed.clone(),
        pump_program_id,
        settings_rx,
    ));

    tokio::spawn(pipeline::run_queue_depth_logger(
        update_tx_weak,
        db_tx_weak,
        strategy_tx_weak,
        shed_counters,
    ));

    // Partition maintenance removed: TimescaleDB retention + compression policies
    // (defined in the migration) now manage chunk lifecycle declaratively. See
    // `trades-storage-plan.md` / `timescaledb-plan.md`.

    let pipeline_task = tokio::spawn(ingest_pipeline.run(update_rx));
    let db_writer = db_writer::DbWriter::new(db, trade_signals, heartbeat);
    let db_writer_task = tokio::spawn(db_writer.run(db_rx));

    IngestHandles {
        pool_index,
        pools_changed,
        strategy_rx,
        producer_task,
        pipeline_task,
        db_writer_task,
    }
}
