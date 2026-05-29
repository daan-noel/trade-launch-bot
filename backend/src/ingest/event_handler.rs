use sqlx::PgPool;
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, error, info, warn};

use crate::{
    ingest::decoder::{DecodeOutput, HeliusDecoder},
    models::events::InternalEvent,
    storage::repositories::transaction_repo::TransactionRepo,
};

/// Sits between the raw WebSocket feed and the rest of the system.
///
/// Responsibilities:
///   1. Decode raw Helius text frames into `InternalEvent`s via `HeliusDecoder`
///   2. Persist every Pump.fun `RawTransaction` to PostgreSQL (non-blocking)
///   3. Broadcast decoded events to all downstream subscribers
pub struct EventHandler {
    decoder: HeliusDecoder,
    event_tx: broadcast::Sender<InternalEvent>,
    /// Cheap to clone — PgPool is Arc-backed internally.
    pool: PgPool,
}

impl EventHandler {
    pub fn new(
        pump_program_id: String,
        event_tx: broadcast::Sender<InternalEvent>,
        pool: PgPool,
    ) -> Self {
        Self {
            decoder: HeliusDecoder::new(pump_program_id),
            event_tx,
            pool,
        }
    }

    /// Drive the handler until `raw_rx` is closed (WS task exited).
    pub async fn run(self, mut raw_rx: mpsc::Receiver<String>) {
        info!("EventHandler: starting");

        while let Some(raw) = raw_rx.recv().await {
            match self.decoder.decode(&raw) {
                DecodeOutput::Subscribed(id) => {
                    info!("Helius subscription confirmed — id={id}");
                }

                DecodeOutput::Transaction { raw_tx, events } => {
                    // Persist the raw transaction in a background task so it
                    // never blocks event broadcasting.
                    let pool = self.pool.clone();
                    let sig = raw_tx.signature.clone();
                    let raw_tx_clone = raw_tx.clone();

                    // ! Don't save raw transaction histories from Helius WS,
                    // ! for now
                    // tokio::spawn(async move {
                    //     let repo = TransactionRepo::new(pool);
                    //     if let Err(e) = repo.insert(&raw_tx_clone).await {
                    //         error!("Failed to persist raw tx {sig}: {e}");
                    //     }
                    // });

                    // Broadcast all decoded events
                    let n = events.len();
                    for event in events {
                        if let Err(e) = self.event_tx.send(event) {
                            // `SendError` means there are no active receivers yet
                            // (normal during startup or when no services are subscribed).
                            warn!("Event broadcast dropped (no receivers): {e}");
                        }
                    }

                    if n > 0 {
                        debug!("Broadcast {n} event(s) for tx {}", raw_tx.signature);
                    }
                }

                DecodeOutput::Ignored => {}
            }
        }

        info!("EventHandler: raw_rx closed — stopping");
    }
}
