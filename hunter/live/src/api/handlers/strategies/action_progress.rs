//! Action-scoped progress for Stop / Stop All.
//!
//! The engine already marks each open position `ExitPending` and pushes
//! `strategy_position_update` as sells confirm. This module adds the rollup
//! (`action_progress` SSE) so the Rules bulk/per-row Stop buttons can show
//! "Stopping 3/8" instead of a static rule count.
//!
//! **Postgres is the authority, SSE is only the fast path.** Completion must never
//! be derived *solely* from `strategy_position_update` frames: they ride the same
//! 512-slot broadcast bus that carries one frame per ingested trade, so under live
//! feed load a `Lagged` gap silently drops terminal frames that are emitted exactly
//! once and never repeat. `remaining` then never empties, and the Rules row sits on
//! a "Stopping…" spinner with Stop **and** Pause disabled until the
//! page was reloaded. It also had no deadline, so a position the stop could not
//! actually drive parked the spinner forever. Now every `Lagged`, a periodic tick,
//! and the give-up deadline all re-read the tracked rows from PG, so a lost frame
//! costs at most [`RESYNC_EVERY`] of progress lag and never correctness.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use tokio::sync::broadcast;
use trading_core::models::ingest::SseEvent;
use trading_core::storage::repositories::strategy_repo::StrategyRepo;
use uuid::Uuid;

/// How often the watcher re-reads its tracked rows from PG. Also the worst-case
/// progress lag when SSE frames are dropped. One small `id = ANY($1)` query per
/// tick, only while a stop is in flight.
const RESYNC_EVERY: Duration = Duration::from_secs(3);

/// Hard ceiling on one Stop action. A paper close is bounded by the paper fill
/// wait (~2 s) and a real one by the exit retry ladder, so anything still open
/// here is not going to settle because of *this* action — the reaper owns it.
/// Reaching this reports honestly instead of spinning.
const GIVE_UP_AFTER: Duration = Duration::from_secs(180);

/// Whether a Stop still has work to do on a position in this status.
///
/// The ONE decider, shared by the handlers (which rows to count) and the watcher
/// (when a row is settled). `End`/`EntryFailed` are terminal, and
/// `ExitStuck`/`ExitUnconfirmed` are engine-terminal — the row stays OPEN in the
/// attention lane, but the stop is done with it and **nothing will ever emit
/// another frame for it**. Counting those was the second way the spinner hung:
/// `find_open_positions` is "not End/EntryFailed", so every already-stuck row of
/// the rule was in the watch set with no work behind it.
pub fn stop_in_flight(status: &str) -> bool {
    !matches!(status, "End" | "EntryFailed" | "ExitStuck" | "ExitUnconfirmed")
}

/// Emit one `action_progress` frame (best-effort; no subscribers → send is a no-op).
#[allow(clippy::too_many_arguments)]
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

/// Running count of one Stop action.
struct Tally {
    remaining: HashSet<Uuid>,
    total: u64,
    closed: u64,
    failed: u64,
}

impl Tally {
    /// Settle one tracked position. `cleanly` = it reached `End` (a confirmed
    /// exit fill); anything else that ends the wait counts as failed, including a
    /// row that vanished. Every removal increments exactly one counter, so
    /// `done() == total - remaining.len()` always holds.
    fn settle(&mut self, id: Uuid, cleanly: bool) -> bool {
        if !self.remaining.remove(&id) {
            return false;
        }
        if cleanly {
            self.closed += 1;
        } else {
            self.failed += 1;
        }
        true
    }

    fn done(&self) -> u64 {
        self.closed + self.failed
    }

    /// Terminal `(status, error)` for the action as it stands.
    fn outcome(&self) -> (&'static str, Option<String>) {
        let stranded = self.remaining.len();
        if stranded > 0 {
            return (
                if self.closed > 0 { "partial" } else { "failed" },
                Some(format!(
                    "timed out with {stranded} position(s) still open — \
                     the recovery reaper owns them now"
                )),
            );
        }
        if self.failed == 0 {
            ("done", None)
        } else if self.closed == 0 {
            ("failed", Some("all position exits failed".into()))
        } else {
            ("partial", Some("some position exits failed".into()))
        }
    }
}

/// Re-read every still-tracked row from PG and settle the ones that are no longer
/// in flight. Returns `true` if anything changed. A read error changes nothing —
/// the next tick retries.
async fn resync(repo: &StrategyRepo, tally: &mut Tally) -> bool {
    let ids: Vec<Uuid> = tally.remaining.iter().copied().collect();
    let rows = match repo.find_position_statuses(&ids).await {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!("stop watcher: position re-sync failed: {e}");
            return false;
        }
    };
    let by_id: HashMap<Uuid, String> = rows.into_iter().collect();
    let mut changed = false;
    for id in ids {
        match by_id.get(&id) {
            Some(status) if stop_in_flight(status) => {}
            Some(status) => changed |= tally.settle(id, status == "End"),
            // Row gone (deleted while stopping) — nothing left to wait on.
            None => changed |= tally.settle(id, false),
        }
    }
    changed
}

/// Watch for `position_ids` to leave the in-flight set and emit rollup progress
/// until every one settles (or [`GIVE_UP_AFTER`] elapses). Spawns a background
/// task — the HTTP handler returns immediately.
///
/// Subscribes **before** returning so the caller can kick off `close_rule` /
/// `close_mode` without racing the first `ExitPending`/`End` frames.
pub fn spawn_stop_watcher(
    sse_tx: broadcast::Sender<SseEvent>,
    repo: StrategyRepo,
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
        let mut tally = Tally {
            remaining: position_ids,
            total,
            closed: 0,
            failed: 0,
        };

        let mut resync_tick = tokio::time::interval(RESYNC_EVERY);
        resync_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        resync_tick.tick().await; // the immediate first tick
        let deadline = tokio::time::Instant::now() + GIVE_UP_AFTER;

        while !tally.remaining.is_empty() {
            let changed = tokio::select! {
                frame = rx.recv() => match frame {
                    Ok(SseEvent::StrategyPositionUpdate { position_id, status, .. }) => {
                        !stop_in_flight(&status) && tally.settle(position_id, status == "End")
                    }
                    Ok(_) => false,
                    // Bus closed (shutdown) — PG is still the truth, so take one
                    // last look rather than reporting a half-finished action.
                    Err(broadcast::error::RecvError::Closed) => {
                        resync(&repo, &mut tally).await;
                        break;
                    }
                    // Dropped frames are never re-delivered: re-read from PG.
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(action = %action_id, dropped = n,
                            "stop watcher: SSE lagged — re-syncing from PG");
                        resync(&repo, &mut tally).await
                    }
                },
                _ = resync_tick.tick() => resync(&repo, &mut tally).await,
                _ = tokio::time::sleep_until(deadline) => {
                    resync(&repo, &mut tally).await;
                    break;
                }
            };

            if changed && !tally.remaining.is_empty() {
                let done = tally.done();
                emit(&sse_tx, action_id, "stop", rule_id, "running", done, tally.total, None);
            }
        }

        let (status, error) = tally.outcome();
        if !tally.remaining.is_empty() {
            tracing::warn!(
                action = %action_id, stranded = tally.remaining.len(),
                "stop watcher: gave up with positions still open"
            );
        }
        emit(&sse_tx, action_id, "stop", rule_id, status, tally.done(), tally.total, error);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The partition the handlers and the watcher both depend on: a stop drives
    /// the three live statuses and is done with the four the engine never
    /// re-emits for. Drift here re-opens the forever-spinner.
    #[test]
    fn stop_drives_exactly_the_live_statuses() {
        for s in ["BuySubmitted", "Holding", "ExitPending"] {
            assert!(stop_in_flight(s), "{s} still has stop work");
        }
        for s in ["End", "EntryFailed", "ExitStuck", "ExitUnconfirmed"] {
            assert!(!stop_in_flight(s), "{s} is settled for a stop");
        }
    }

    #[test]
    fn settle_keeps_done_equal_to_total_minus_remaining() {
        let (a, b, c) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let mut t = Tally {
            remaining: [a, b, c].into_iter().collect(),
            total: 3,
            closed: 0,
            failed: 0,
        };
        assert!(t.settle(a, true));
        assert!(t.settle(b, false));
        // A duplicate frame for an already-settled id must not double-count.
        assert!(!t.settle(a, true));
        assert_eq!(t.done(), t.total - t.remaining.len() as u64);
        assert_eq!((t.closed, t.failed), (1, 1));
    }

    #[test]
    fn stranded_positions_report_instead_of_claiming_done() {
        let mut t = Tally {
            remaining: [Uuid::new_v4()].into_iter().collect(),
            total: 2,
            closed: 1,
            failed: 0,
        };
        let (status, error) = t.outcome();
        assert_eq!(status, "partial");
        assert!(error.is_some());

        let id = *t.remaining.iter().next().unwrap();
        t.settle(id, true);
        assert_eq!(t.outcome().0, "done");
    }
}
