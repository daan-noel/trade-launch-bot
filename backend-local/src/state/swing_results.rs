//! Completed "Swing Detection All" result store.
//!
//! A run can scan thousands of (uncapped) token histories and take minutes. The
//! batch endpoint used to hold one HTTP connection open per ≤200-mint chunk and
//! return the result down it; any mid-run drop — the dev proxy / browser idle cut
//! / the ingest watchdog restarting the process under load — severed that socket
//! and surfaced on the client as a `FETCH_ERROR`, even though the detached scan
//! finished fine.
//!
//! Now the start endpoint returns immediately, the detached run stores its
//! terminal outcome here, and the client collects it with a quick GET once the
//! `swing_detection_finished` SSE fires — there is no long-held connection to
//! sever. The exact twin of [`SimResults`](crate::state::sim_results::SimResults),
//! diverging only in key type: a swing run is keyed by the client
//! `crypto.randomUUID()` `String` (matching [`SwingRunCache`]), not a rule `Uuid`.
//!
//! Entries are taken (removed) on fetch — single delivery — and, as a backstop
//! against a run whose result is never collected (the client navigated away),
//! lazily evicted past [`RESULT_TTL`] on every insert.
//!
//! [`SwingRunCache`]: crate::state::swing_run_cache::SwingRunCache

use std::time::{Duration, Instant};

use dashmap::DashMap;

/// How long a finished result is retained for collection. The client fetches it
/// immediately on the `swing_detection_finished` SSE, so this only has to outlive
/// that round-trip plus a brief reconnect — a few minutes is ample.
const RESULT_TTL: Duration = Duration::from_secs(600);

/// Terminal outcome of a swing-detection run, ready to serve from the result
/// endpoint.
pub enum SwingOutcome {
    /// Success — the `SwingBatchResponse` already serialized to a JSON string.
    /// Serialized once at completion; the endpoint returns the bytes verbatim so
    /// a large (uncapped) payload is never re-serialized.
    Done(String),
    /// User-requested cancel — served as `{"cancelled": true}` at HTTP 200.
    Cancelled,
    /// Failure — `status` is the HTTP code the result endpoint returns and
    /// `message` the body error.
    Failed { status: u16, message: String },
}

/// Store of finished swing-run outcomes, keyed by client run id.
#[derive(Default)]
pub struct SwingResults {
    map: DashMap<String, (Instant, SwingOutcome)>,
}

impl SwingResults {
    pub fn new() -> Self {
        Self::default()
    }

    /// Store a run's terminal outcome, first evicting any results older than
    /// [`RESULT_TTL`] (lazy GC — the map only ever holds a handful of user-driven
    /// runs, so the scan is trivial). Overwrites any prior outcome for the same
    /// run id: a re-run supersedes a stale, uncollected result.
    pub fn insert(&self, run_id: String, outcome: SwingOutcome) {
        self.map.retain(|_, (at, _)| at.elapsed() < RESULT_TTL);
        self.map.insert(run_id, (Instant::now(), outcome));
    }

    /// Take (remove + return) the stored outcome for a run, if any. `None` means
    /// the run never started, is still running, was already collected, or expired.
    pub fn take(&self, run_id: &str) -> Option<SwingOutcome> {
        self.map.remove(run_id).map(|(_, (_, outcome))| outcome)
    }

    /// Drop any stored outcome for a run — called when a fresh run starts so a
    /// stale result can't be collected for the new run.
    pub fn clear(&self, run_id: &str) {
        self.map.remove(run_id);
    }
}
