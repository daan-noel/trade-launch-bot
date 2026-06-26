//! Completed-simulation result store.
//!
//! A backtest can run for minutes over uncapped data. The simulate endpoint used
//! to hold one HTTP connection open for the whole run and stream the result down
//! it; any mid-run drop — the dev proxy / browser idle cut / the ingest watchdog
//! restarting the process under load — severed that socket and surfaced on the
//! client as a `FETCH_ERROR`, even though the detached backtest finished fine.
//!
//! Now the start endpoint returns immediately, the detached run stores its
//! terminal outcome here, and the client collects it with a quick GET once the
//! `simulation_finished` SSE fires — there is no long-held connection to sever.
//!
//! Keyed by `rule_id` (UUIDs never collide across tpsl1/tpsl2), strategy-agnostic
//! like [`sim_cancels`](crate::state::local_state::LocalState::sim_cancels) and
//! [`sim_progress`](crate::state::local_state::LocalState::sim_progress). Entries are
//! taken (removed) on fetch — single delivery — and, as a backstop against a run
//! whose result is never collected (the client navigated away), lazily evicted
//! past [`RESULT_TTL`] on every insert.

use std::time::{Duration, Instant};

use dashmap::DashMap;
use uuid::Uuid;

/// How long a finished result is retained for collection. The client fetches it
/// immediately on the `simulation_finished` SSE, so this only has to outlive that
/// round-trip plus a brief reconnect — a few minutes is ample.
const RESULT_TTL: Duration = Duration::from_secs(600);

/// Terminal outcome of a backtest, ready to serve from the result endpoint.
pub enum SimOutcome {
    /// Success — the `Vec<BacktestTokenResult>` already serialized to a JSON
    /// array string. Serialized once at completion; the endpoint returns the
    /// bytes verbatim so a large (uncapped) payload is never re-serialized.
    Done(String),
    /// User-requested cancel — served as `{"cancelled": true}` at HTTP 200.
    Cancelled,
    /// Failure — `status` is the HTTP code the result endpoint returns (404 rule
    /// not found, 400 invalid rule, 500 otherwise) and `message` the body error.
    Failed { status: u16, message: String },
}

/// Strategy-agnostic store of finished simulation outcomes, keyed by `rule_id`.
#[derive(Default)]
pub struct SimResults {
    map: DashMap<Uuid, (Instant, SimOutcome)>,
}

impl SimResults {
    pub fn new() -> Self {
        Self::default()
    }

    /// Store a run's terminal outcome, first evicting any results older than
    /// [`RESULT_TTL`] (lazy GC — the map only ever holds a handful of user-driven
    /// runs, so the scan is trivial). Overwrites any prior outcome for the same
    /// rule: a re-run supersedes a stale, uncollected result.
    pub fn insert(&self, rule_id: Uuid, outcome: SimOutcome) {
        self.map.retain(|_, (at, _)| at.elapsed() < RESULT_TTL);
        self.map.insert(rule_id, (Instant::now(), outcome));
    }

    /// Take (remove + return) the stored outcome for a rule, if any. `None` means
    /// the run never started, is still running, was already collected, or expired.
    pub fn take(&self, rule_id: &Uuid) -> Option<SimOutcome> {
        self.map.remove(rule_id).map(|(_, (_, outcome))| outcome)
    }

    /// Drop any stored outcome for a rule — called when a fresh run starts so a
    /// stale result can't be collected for the new run.
    pub fn clear(&self, rule_id: &Uuid) {
        self.map.remove(rule_id);
    }
}
