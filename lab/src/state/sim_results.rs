//! Completed-simulation result store.
//!
//! A backtest can run for minutes over uncapped data. The simulate endpoint used
//! to hold one HTTP connection open for the whole run and stream the result down
//! it; any mid-run drop — the dev proxy / browser idle cut / the ingest watchdog
//! restarting the process under load — severed that socket and surfaced on the
//! client as a `FETCH_ERROR`, even though the detached backtest finished fine.
//!
//! Now the start endpoint returns immediately, the detached run stores its
//! terminal outcome here, and the client collects it once the
//! `simulation_finished` SSE fires — there is no long-held connection to sever.
//!
//! The success payload is kept as the **parsed** per-token rows (`Vec<Value>`, one
//! object per `BacktestTokenResult`) behind an `Arc`, not a JSON string: the
//! Simulated token table pages/sorts/filters it **server-side in memory** (the
//! unified `TableRequest` contract) via repeated [`peek`](SimResults::peek)s, so it
//! must survive multiple reads. The results are already fully resident (lab is
//! single-user, workstation RAM), so an in-RAM query needs no DB — see
//! `strategies::sim_query`.
//!
//! Keyed by `rule_id` (UUIDs never collide across tpsl1/tpsl2/swing1). Lazily
//! evicted past [`RESULT_TTL`] on every insert as a backstop against a run whose
//! result is never collected.

use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use serde_json::Value;
use uuid::Uuid;

/// How long a finished result is retained. The Simulated table pages/sorts against
/// it interactively (many quick follow-up reads), so this must comfortably outlive
/// a browsing session.
const RESULT_TTL: Duration = Duration::from_secs(600);

/// Terminal outcome of a backtest, ready to serve from the result endpoints.
#[derive(Clone)]
pub enum SimOutcome {
    /// Success — the per-token results parsed to a JSON array (`Arc`-shared so a
    /// page read is a refcount bump, not a copy of a potentially large payload).
    Done(Arc<Vec<Value>>),
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
    /// runs). Overwrites any prior outcome for the same rule: a re-run supersedes a
    /// stale, uncollected result.
    pub fn insert(&self, rule_id: Uuid, outcome: SimOutcome) {
        self.map.retain(|_, (at, _)| at.elapsed() < RESULT_TTL);
        self.map.insert(rule_id, (Instant::now(), outcome));
    }

    /// Take (remove + return) the stored outcome for a rule, if any. Single
    /// delivery — used by the legacy whole-blob `/result` collector. `None` means
    /// the run never started, is still running, was already taken, or expired.
    pub fn take(&self, rule_id: &Uuid) -> Option<SimOutcome> {
        self.map.remove(rule_id).map(|(_, (_, outcome))| outcome)
    }

    /// Borrow (clone, **not** remove) the stored outcome for a rule — the Simulated
    /// table's server-side pager reads it repeatedly (page/sort/filter round-trips)
    /// without consuming it. The `Done` payload clone is a cheap `Arc` bump.
    pub fn peek(&self, rule_id: &Uuid) -> Option<SimOutcome> {
        self.map.get(rule_id).map(|e| e.value().1.clone())
    }

    /// Drop any stored outcome for a rule — called when a fresh run starts so a
    /// stale result can't be collected for the new run.
    pub fn clear(&self, rule_id: &Uuid) {
        self.map.remove(rule_id);
    }
}
