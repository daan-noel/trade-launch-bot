//! The ingest → DB pipeline (LIVE box). Builds the borrowed transport, then
//! drains `IngestEvent`s on a hot recv loop that does **no DB I/O** — it forwards
//! durable work onto a bounded channel drained by a separate
//! [`DbWriter`](super::db_writer) task. A wedged DB therefore backs up the channel
//! (and, once full, backpressures the transport) instead of stalling the recv
//! loop; the [`watchdog`](super::watchdog) force-exits the process if writes stop
//! while work is queued.
//!
//! Only the tx-tracking state machine lives here: `raw_txs` is persisted only for
//! txs that produced a semantic event (the trailing `RawTx` event uses a per-tx
//! flag), so `raw_txs` stays the source-of-truth for exactly what `trades`
//! projects. That decision is pure stream ordering — no DB — so it stays on the
//! hot path; all mapping + interning + inserts happen in the `DbWriter`.

use std::sync::Arc;

use ingest_laserstream::{Ingest, IngestConfig, IngestEvent, IngestHandle, Protocol};
use sqlx::PgPool;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tracing::{info, warn};

use super::db_writer::{DbWriteOp, DbWriter, CHANNEL_CAPACITY};
use super::pumpfun::PumpFunAdapter;
use super::watchdog::{spawn_watchdog, DbHeartbeat};
use crate::sse::SseHub;

/// Build the transport and spawn the ingest → DB pipeline (consumer + writer +
/// watchdog). Boots **paused** (`start(false)`): no gRPC subscription opens until
/// an operator flips the runtime toggle on (`PUT /api/ingest {live:true}` →
/// `handle.set_live(true)`). Returns the consumer's join handle plus the control
/// handle (the caller owns it for the process lifetime to expose that runtime
/// pause/resume toggle over HTTP).
pub async fn spawn_ingest(
    pool: PgPool,
    endpoint: String,
    api_key: String,
    trades_notify: Arc<Notify>,
    sse: SseHub,
) -> anyhow::Result<(JoinHandle<()>, Arc<IngestHandle>)> {
    let adapter = PumpFunAdapter::resolve(&pool).await?;
    let (event_rx, handle) = Ingest::builder()
        .endpoint(endpoint)
        .api_key(api_key)
        .protocol(Protocol::pump_fun())
        .config(IngestConfig::default())
        .build()?
        .start(false);
    let handle = Arc::new(handle);

    // Decoupled writer: hot recv loop → bounded channel → DbWriter task.
    let (tx, rx) = mpsc::channel::<DbWriteOp>(CHANNEL_CAPACITY);
    let heartbeat = DbHeartbeat::new();
    tokio::spawn(DbWriter::new(pool, adapter, heartbeat.clone(), trades_notify, sse).run(rx));

    // Watchdog on its own OS thread: force-exit if the writer stops committing
    // while the queue is backed up and the stream is live. `is_live` reads the
    // ingest toggle; `work_pending` reads the channel occupancy via a weak sender
    // (so the watchdog doesn't keep the channel alive after the consumer exits).
    let live_handle = handle.clone();
    let weak_tx = tx.downgrade();
    spawn_watchdog(
        heartbeat,
        move || live_handle.is_live(),
        move || {
            weak_tx
                .upgrade()
                .map(|s| s.max_capacity() - s.capacity() > 0)
                .unwrap_or(false)
        },
    );

    info!("ingest transport spawned, paused (pump.fun/SOL adapter) — enable via PUT /api/ingest");
    let join = tokio::spawn(run_consumer(event_rx, tx));
    Ok((join, handle))
}

/// Hot recv loop: forward durable work to the writer, tracking which txs produced
/// a semantic event so only those get their `RawTx` persisted. No DB, no mapping.
async fn run_consumer(mut event_rx: Receiver<IngestEvent>, tx: Sender<DbWriteOp>) {
    let mut tracked_in_tx = false;
    while let Some(ev) = event_rx.recv().await {
        let op = match ev {
            IngestEvent::Trade(t) => {
                tracked_in_tx = true;
                DbWriteOp::Trade(Box::new(t))
            }
            IngestEvent::TokenCreated(tc) => {
                tracked_in_tx = true;
                DbWriteOp::TokenCreated(Box::new(tc))
            }
            IngestEvent::RawTx(r) => {
                let forward = tracked_in_tx;
                tracked_in_tx = false;
                if !forward {
                    continue;
                }
                DbWriteOp::Raw(Box::new(r))
            }
            // TokenMigrated / Liquidity / CreatorActivity: not projected yet.
            _ => continue,
        };
        // `send().await` is the durable-write backpressure: if the writer is
        // behind, this blocks and stops draining the transport rather than
        // dropping real rows.
        if tx.send(op).await.is_err() {
            warn!("ingest DbWriter channel closed — consumer exiting");
            break;
        }
    }
    warn!("ingest event channel closed — transport ended");
}
