//! Action-scoped progress for Stop / Stop All.
//!
//! The engine already marks each open position `ExitPending` and pushes
//! `strategy_position_update` as sells confirm. This module adds the rollup
//! (`action_progress` SSE) so the Rules bulk/per-row Stop buttons can show
//! "Stopping 3/8" instead of a static rule count.

use std::collections::HashSet;

use tokio::sync::broadcast;
use trading_core::models::ingest::SseEvent;
use uuid::Uuid;

/// Emit one `action_progress` frame (best-effort; no subscribers → send is a no-op).
pub fn emit(
    sse_tx: &broadcast::Sender<SseEvent>,
    action_id: Uuid,
    kind: &str,
    rule_id: Option<Uuid>,
    status: &str,
    done: u64,
    total: u64,
    error: Option<String>,
) {
    let _ = sse_tx.send(SseEvent::ActionProgress {
        action_id,
        mint_address: None,
        rule_id,
        kind: kind.to_string(),
        status: status.to_string(),
        done,
        total,
        error,
    });
}

/// Watch `strategy_position_update` frames for `position_ids` and emit rollup
/// progress until every tracked position reaches a terminal exit status (or the
/// SSE bus closes). Spawns a background task — the HTTP handler returns immediately.
///
/// Subscribes **before** returning so the caller can kick off `close_rule` /
/// `close_mode` without racing the first `ExitPending`/`End` frames.
pub fn spawn_stop_watcher(
    sse_tx: broadcast::Sender<SseEvent>,
    action_id: Uuid,
    rule_id: Option<Uuid>,
    position_ids: HashSet<Uuid>,
) {
    let total = position_ids.len() as u64;
    if total == 0 {
        emit(&sse_tx, action_id, "stop", rule_id, "done", 0, 0, None);
        return;
    }

    // Subscribe first — close fires immediately after this returns.
    let mut rx = sse_tx.subscribe();
    emit(&sse_tx, action_id, "stop", rule_id, "running", 0, total, None);

    tokio::spawn(async move {
        let mut remaining = position_ids;
        let mut closed = 0u64;
        let mut failed = 0u64;

        while !remaining.is_empty() {
            match rx.recv().await {
                Ok(SseEvent::StrategyPositionUpdate {
                    position_id,
                    status,
                    ..
                }) if remaining.contains(&position_id) => {
                    // "No longer draining": a clean close, a never-filled buy, or
                    // an exit the engine handed off (stuck/unconfirmed — the row
                    // stays open in the attention lane, but the stop is done with it).
                    let terminal = matches!(
                        status.as_str(),
                        "End" | "EntryFailed" | "ExitStuck" | "ExitUnconfirmed"
                    );
                    if !terminal {
                        continue;
                    }
                    remaining.remove(&position_id);
                    if status == "End" {
                        closed += 1;
                    } else {
                        failed += 1;
                    }
                    let done = closed + failed;
                    if remaining.is_empty() {
                        let (status, error) = if failed == 0 {
                            ("done", None)
                        } else if closed == 0 {
                            ("failed", Some("all position exits failed".into()))
                        } else {
                            ("partial", Some("some position exits failed".into()))
                        };
                        emit(
                            &sse_tx,
                            action_id,
                            "stop",
                            rule_id,
                            status,
                            done,
                            total,
                            error,
                        );
                    } else {
                        emit(
                            &sse_tx,
                            action_id,
                            "stop",
                            rule_id,
                            "running",
                            done,
                            total,
                            None,
                        );
                    }
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Closed) => break,
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    // Missed frames — keep waiting; position rows still settle
                    // eventually and a later frame (or reload) self-heals the UI.
                }
            }
        }
    });
}
