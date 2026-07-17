//! Live progress + cooperative cancellation for the grouped param-sweep.
//!
//! The sweep runs inside `spawn_blocking` on a bounded rayon pool and can fold
//! millions of (combo, token) outcomes, so — like the backtest's [`SimProgress`]
//! — it streams real `processed / total` token counts over an SSE event instead
//! of leaving the dashboard on a fake trickle bar. The engine layer
//! ([`engine`]/[`grouped_engine`]) stays strategy- and transport-agnostic by
//! talking only to the [`SweepObserver`] trait; the SSE-emitting [`SweepProgress`]
//! lives here and is wired in by the handler.
//!
//! Cancellation is cooperative: the handler flips an `AtomicBool` (via the cancel
//! endpoint) and the engine polls [`SweepObserver::cancelled`] between groups,
//! between tokens, between chunks of combos *within* a token, AND before each fresh
//! entry resolve (so a strategy with an expensive entry — swing1's full-history
//! swing scan — still bails promptly, not once per `CANCEL_CHECK_STRIDE` scans),
//! bailing fast without an extra RPC/DB hit. The one pre-fold phase the engine can't
//! see is the DuckDB corpus load; the handler polls the flag the instant that load
//! returns (see `run_grouped_sweep_job`) so a cancel during it isn't swallowed.
//!
//! [`engine`]: crate::sweep::engine
//! [`grouped_engine`]: crate::sweep::grouped_engine
//! [`SimProgress`]: crate::strategies::sim_progress::SimProgress

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use tokio::sync::broadcast;

use crate::models::ingest::SseEvent;
use crate::state::job_progress::ProgressCell;

/// Target number of progress frames over a full run; the per-tick throttle is
/// derived from this so a tiny corpus reports often and a huge one reports ~1%.
const STEPS: usize = 100;

/// What the engine needs from a progress sink — kept transport-agnostic so the
/// rayon hot loop never touches SSE/Tokio. `Sync` because the engine shares one
/// `&dyn SweepObserver` across rayon worker threads.
///
/// **Progress is measured in `(token, combo)` evaluations, not tokens.** The engine
/// evaluates every combo against every token exactly once, but it splits the combo
/// space into memory-timed **batches/shards** and folds a token once per chunk — so
/// "tokens folded" is not a stable unit (its total depends on how live RAM chunked
/// the run, which the up-front denominator can't predict; that mismatch made the bar
/// overrun 100%). Evaluations *are* stable: `Σ group_tokens × n_combos` regardless of
/// chunking, so `done` always converges to `total` exactly. The observer divides both
/// by `combos_per_token` when it reports, so the dashboard still shows nice
/// token-equivalent numbers.
pub trait SweepObserver: Sync {
    /// Declare the run's size: surviving tokens across all groups, and the combo
    /// count each token is evaluated against (`combos_per_token`). Called once after
    /// partitioning, before sweeping. Internally the total work is
    /// `total_tokens × combos_per_token` evaluations.
    fn set_total(&self, total_tokens: usize, combos_per_token: usize);
    /// Mark `combos_folded` combos evaluated for one token (called once per token per
    /// combo chunk, with the chunk's combo count — so the increments over a token's
    /// full combo set sum to `combos_per_token`, independent of how it was chunked).
    fn token_done(&self, combos_folded: usize);
    /// True once a cancel has been requested — polled in the hot loop.
    fn cancelled(&self) -> bool;
}

/// SSE-emitting observer for one phase of the grouped sweep. Broadcasts a
/// throttled [`SseEvent::SweepProgress`] frame (tagged with `phase`) as tokens
/// fold so the dashboard renders a real percentage, and surfaces the shared
/// cancel flag to the engine.
pub struct SweepProgress {
    sse_tx: broadcast::Sender<SseEvent>,
    strategy_id: String,
    /// Which phase this observer represents: `"corpus"` | `"coarse"` | `"sweep"`.
    phase: String,
    /// Total work in **evaluations** (`total_tokens × combos_per_token`).
    total: AtomicUsize,
    /// Evaluations done so far (`Σ combos_folded`).
    done: AtomicUsize,
    /// Combos each token is evaluated against — the divisor that turns the internal
    /// evaluation counters back into token-equivalents for display.
    combos_per_token: AtomicUsize,
    /// Cooperative cancel flag, shared with the cancel endpoint via app state.
    cancel: Arc<AtomicBool>,
    /// Shared readable snapshot for `/api/jobs/status` (refresh recovery). Written
    /// on every tick (cheap atomic store) even when an SSE frame is throttled, so
    /// a mid-run status read is always accurate. Pass a throwaway
    /// `Arc<ProgressCell::default()>` for phases where status recovery isn't needed.
    cell: Arc<ProgressCell>,
}

impl SweepProgress {
    pub fn new(
        sse_tx: broadcast::Sender<SseEvent>,
        strategy_id: impl Into<String>,
        cancel: Arc<AtomicBool>,
        cell: Arc<ProgressCell>,
        phase: impl Into<String>,
    ) -> Self {
        Self {
            sse_tx,
            strategy_id: strategy_id.into(),
            phase: phase.into(),
            total: AtomicUsize::new(0),
            done: AtomicUsize::new(0),
            combos_per_token: AtomicUsize::new(1),
            cancel,
            cell,
        }
    }

    /// Emit a frame. `processed_evals` and the stored total are **evaluations**; both
    /// are divided by `combos_per_token` so the wire (and dashboard) see
    /// token-equivalents that converge exactly to the surviving-token count.
    fn send(&self, processed_evals: usize) {
        let cpt = self.combos_per_token.load(Ordering::Relaxed).max(1);
        let total_evals = self.total.load(Ordering::Relaxed);
        let _ = self.sse_tx.send(SseEvent::SweepProgress {
            strategy_id: self.strategy_id.clone(),
            phase: self.phase.clone(),
            processed: (processed_evals / cpt) as u64,
            total: (total_evals / cpt) as u64,
        });
    }
}

impl SweepObserver for SweepProgress {
    fn set_total(&self, total_tokens: usize, combos_per_token: usize) {
        let cpt = combos_per_token.max(1);
        self.combos_per_token.store(cpt, Ordering::Relaxed);
        // Work is counted in (token, combo) evaluations — the plan-invariant unit.
        let total_evals = total_tokens.saturating_mul(cpt);
        self.total.store(total_evals, Ordering::Relaxed);
        self.cell.set_total(total_tokens as u64);
        // Emit the initial `0 / total` frame so a subscriber can switch from
        // indeterminate to determinate before the first fold.
        self.send(0);
    }

    fn token_done(&self, combos_folded: usize) {
        let prev = self.done.fetch_add(combos_folded, Ordering::Relaxed);
        let n = prev + combos_folded;
        let total_evals = self.total.load(Ordering::Relaxed);
        let cpt = self.combos_per_token.load(Ordering::Relaxed).max(1);
        self.cell.set_processed((n / cpt) as u64);
        // Throttle to ~STEPS frames/run; always emit the final evaluation. Increments
        // can be > 1, so emit whenever this step *crossed* a throttle boundary (a
        // simple `is_multiple_of` would miss most crossings now).
        let step = (total_evals / STEPS).max(1);
        if n >= total_evals || (prev / step) != (n / step) {
            self.send(n);
        }
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

/// No-op observer for tests / call sites that don't report progress.
#[cfg(test)]
pub struct NoopObserver;

#[cfg(test)]
impl SweepObserver for NoopObserver {
    fn set_total(&self, _total_tokens: usize, _combos_per_token: usize) {}
    fn token_done(&self, _combos_folded: usize) {}
    fn cancelled(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::job_progress::ProgressCell;

    /// Drive an evaluation stream through `SweepProgress` with a given combo-chunking
    /// (`chunkings` must partition `combos_per_token` — it stands in for one memory-
    /// timed batch/shard split), then return the final reported `(processed, total)`
    /// token-equivalents from the status cell.
    fn drive(total_tokens: usize, combos_per_token: usize, chunkings: &[usize]) -> (u64, u64) {
        assert_eq!(
            chunkings.iter().sum::<usize>(),
            combos_per_token,
            "chunkings must cover every combo exactly once"
        );
        let (tx, _rx) = broadcast::channel(4096);
        let cell = Arc::new(ProgressCell::default());
        let cancel = Arc::new(AtomicBool::new(false));
        let p = SweepProgress::new(tx, "generic", cancel, cell.clone(), "sweep");
        p.set_total(total_tokens, combos_per_token);
        // The engine fires `token_done(chunk)` once per token per combo chunk.
        for _ in 0..total_tokens {
            for &c in chunkings {
                p.token_done(c);
            }
        }
        cell.snapshot()
    }

    #[test]
    fn progress_is_plan_invariant_and_converges_to_token_count() {
        // The SAME run, chunked three different ways (one batch, uneven batches, and
        // all-singleton combos — the extremes a memory-timed shard/batch plan can
        // produce). Every one must land on exactly processed == total == the
        // surviving-token count. No overrun, no undershoot, regardless of chunking —
        // that is the whole point of counting evaluations instead of token-folds.
        let tokens = 250;
        let combos = 60;
        assert_eq!(drive(tokens, combos, &[60]), (tokens as u64, tokens as u64));
        assert_eq!(drive(tokens, combos, &[25, 25, 10]), (tokens as u64, tokens as u64));
        let singletons = vec![1usize; combos];
        assert_eq!(drive(tokens, combos, &singletons), (tokens as u64, tokens as u64));
    }

    #[test]
    fn total_is_reported_in_token_equivalents_not_evaluations() {
        // The denominator must be the token count (100), never tokens×combos (50_000)
        // — the display stays token-scaled even though work is tracked in evaluations.
        let (processed, total) = drive(100, 500, &[500]);
        assert_eq!(total, 100);
        assert_eq!(processed, 100);
    }
}
