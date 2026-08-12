//! The arm ledger's write path — `ArmedChanged` → batched `strategy_arms` rows.
//!
//! `EffectSink::on_armed_changed` runs inside the decision fold's effect drain,
//! which is the hot path: it queues an [`ArmLedgerWrite`] and returns. This task
//! drains the queue, batches a flush window's worth, and issues **one** INSERT
//! plus **one** UPDATE. A per-episode round trip would put the arm rate — which
//! is unbounded, an arm costs nothing on chain — straight onto the pool.
//!
//! Ordering inside a flush matters: an episode can arm and end in the same
//! window (a token that dies on its creation slot), and the UPDATE keys on a row
//! the INSERT is about to write. So the flush always inserts before it ends.
//!
//! Plan: `docs/plans/strategies/arm-ledger.md`.

use std::time::Duration;

use tokio::sync::mpsc;
use tracing::{info, warn};

use trading_core::models::strategy_arm::ArmLedgerWrite;
use trading_core::storage::repositories::arm_repo::{ArmEndRow, ArmInsertRow, ArmRepo};

/// Queue depth. Sized for a burst of arms across every rule on a launch spike;
/// past this the send is dropped **loudly** rather than blocking the fold — a
/// wedged writer must never become backpressure on trade decisions, and a silent
/// drop would be an invisible hole in the ledger.
const QUEUE_CAP: usize = 8_192;

/// Flush cadence. The ledger is a review surface, not a live one (the Waiting
/// lane reads the in-RAM registry), so latency here buys batch size.
const FLUSH_INTERVAL: Duration = Duration::from_millis(500);

/// Max rows per statement. Postgres caps a statement at 65 535 bind parameters;
/// the end write binds 6 per row, so this leaves an order of magnitude of headroom.
const MAX_BATCH: usize = 1_000;

/// The sink's handle onto the ledger. Cloneable, sync, and non-blocking — the
/// whole point is that the effect drain never awaits a database.
#[derive(Clone)]
pub struct ArmLedger {
    tx: Option<mpsc::Sender<ArmLedgerWrite>>,
}

impl ArmLedger {
    /// A ledger that drops everything — for `lab`-style composition and tests,
    /// where no `strategy_arms` write path exists.
    pub fn disabled() -> Self {
        Self { tx: None }
    }

    /// Queue one write. Never blocks, never awaits.
    pub fn record(&self, write: ArmLedgerWrite) {
        let Some(tx) = &self.tx else { return };
        if let Err(e) = tx.try_send(write) {
            // Loud by rule: a dropped arm is a hole in the ledger that reads
            // exactly like "the rule never armed on that token".
            warn!("arm ledger: dropped a write ({e}) — the ledger is now incomplete");
        }
    }
}

/// Spawn the writer and hand back the sink-side handle.
pub fn spawn_arm_ledger(repo: ArmRepo) -> (ArmLedger, tokio::task::JoinHandle<()>) {
    let (tx, rx) = mpsc::channel(QUEUE_CAP);
    let handle = tokio::spawn(run(repo, rx));
    (ArmLedger { tx: Some(tx) }, handle)
}

async fn run(repo: ArmRepo, mut rx: mpsc::Receiver<ArmLedgerWrite>) {
    let mut ticker = tokio::time::interval(FLUSH_INTERVAL);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut pending: Vec<ArmLedgerWrite> = Vec::with_capacity(MAX_BATCH);
    info!("arm ledger writer running");
    loop {
        tokio::select! {
            got = rx.recv() => {
                match got {
                    Some(w) => {
                        pending.push(w);
                        if pending.len() >= MAX_BATCH {
                            flush(&repo, &mut pending).await;
                        }
                    }
                    // Sender dropped (shutdown) — write what is in hand and stop.
                    None => {
                        flush(&repo, &mut pending).await;
                        info!("arm ledger writer stopped");
                        return;
                    }
                }
            }
            _ = ticker.tick() => flush(&repo, &mut pending).await,
        }
    }
}

/// Split one window into its two statements and issue them **arms first**, so an
/// episode that armed and ended inside the same window still finds its row.
async fn flush(repo: &ArmRepo, pending: &mut Vec<ArmLedgerWrite>) {
    if pending.is_empty() {
        return;
    }
    let mut arms: Vec<ArmInsertRow> = Vec::new();
    let mut ends: Vec<ArmEndRow> = Vec::new();
    for w in pending.drain(..) {
        match w {
            ArmLedgerWrite::Armed { rule_id, mint_address, mode, armed_at } => {
                arms.push((rule_id, mint_address, mode, armed_at));
            }
            ArmLedgerWrite::Ended {
                rule_id,
                mint_address,
                armed_at,
                ended_at,
                end_reason,
                position_id,
            } => ends.push((rule_id, mint_address, armed_at, ended_at, end_reason, position_id)),
        }
    }
    if let Err(e) = repo.insert_arms(&arms).await {
        warn!(n = arms.len(), "arm ledger: insert failed: {e}");
    }
    if let Err(e) = repo.end_arms(&ends).await {
        warn!(n = ends.len(), "arm ledger: end failed: {e}");
    }
}
