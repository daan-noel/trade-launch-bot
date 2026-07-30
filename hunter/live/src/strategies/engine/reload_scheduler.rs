//! Debounced background rule reloads for HTTP mutations.
//!
//! Rule CRUD/lifecycle writes PG first, then schedules an engine reload here so
//! the HTTP response is not held for up to [`super::RELOAD_ACK_TIMEOUT`]. Bursty
//! mutations coalesce into one reload pass (see also decision-loop ack coalescing).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::broadcast;
use trading_core::models::ingest::SseEvent;

use super::{EngineHandle, RELOAD_ACK_TIMEOUT};

const RELOAD_RETRY_ATTEMPTS: u32 = 3;
const RELOAD_RETRY_DELAY: Duration = Duration::from_secs(2);

/// Coalesces concurrent HTTP-triggered reloads into one background task.
#[derive(Debug, Default)]
pub struct EngineReloadScheduler {
    running: AtomicBool,
    dirty: AtomicBool,
}

impl EngineReloadScheduler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue a reload after a rule/fingerprint mutation. On success, emits
    /// `tpsl_rules_changed` so clients refetch without waiting on HTTP.
    pub fn schedule(
        self: Arc<Self>,
        engine: EngineHandle,
        sse_tx: broadcast::Sender<SseEvent>,
    ) {
        self.dirty.store(true, Ordering::Release);
        if self.running.swap(true, Ordering::AcqRel) {
            return;
        }

        tokio::spawn(async move {
            self.run(engine, sse_tx).await;
        });
    }

    async fn run(self: Arc<Self>, engine: EngineHandle, sse_tx: broadcast::Sender<SseEvent>) {
        loop {
            self.dirty.store(false, Ordering::Release);
            if reload_with_retry(&engine).await {
                let _ = sse_tx.send(SseEvent::TpslRulesChanged {
                    strategy: "generic".into(),
                });
            }
            if !self.dirty.load(Ordering::Acquire) {
                break;
            }
        }
        self.running.store(false, Ordering::Release);
    }
}

async fn reload_with_retry(engine: &EngineHandle) -> bool {
    for attempt in 1..=RELOAD_RETRY_ATTEMPTS {
        match engine.reload_rules().await {
            Ok(()) => return true,
            Err(e) => {
                tracing::warn!(
                    attempt,
                    max = RELOAD_RETRY_ATTEMPTS,
                    timeout_secs = RELOAD_ACK_TIMEOUT.as_secs(),
                    "background engine reload failed: {e}"
                );
                if attempt < RELOAD_RETRY_ATTEMPTS {
                    tokio::time::sleep(RELOAD_RETRY_DELAY).await;
                }
            }
        }
    }
    tracing::error!(
        "engine reload failed after {RELOAD_RETRY_ATTEMPTS} attempts — \
         strategy_rules PG state and the live engine may diverge until the next mutation"
    );
    false
}
