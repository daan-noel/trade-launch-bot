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
//! Keyed by `rule_id` (UUIDs never collide across tpsl1/tpsl2/swing1). Each entry
//! also records the [`sim_key`](trading_core::strategies::match_keys::sim_key) and
//! analysis window so an unchanged re-run can short-circuit without re-walking
//! trades. Lazily evicted past [`RESULT_TTL`] on every insert.

use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde_json::Value;
use uuid::Uuid;

/// How long a finished result is retained. The Simulated table pages/sorts against
/// it interactively (many quick follow-up reads), so this must comfortably outlive
/// a browsing session — including a long "Simulate All" batch where the first rules
/// finish long before the last, then get clicked into for detail. 60 min so an
/// unchanged single-rule re-open hits the `peek_if` fast path instead of re-walking
/// trades. `lab` is single-user on the workstation (RAM headroom); the twin
/// `analysis_cache::CACHE_TTL` is kept in lock-step.
const RESULT_TTL: Duration = Duration::from_secs(3600);

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

struct SimEntry {
    at: Instant,
    sim_key: String,
    from: Option<i64>,
    to: Option<i64>,
    outcome: SimOutcome,
}

/// Strategy-agnostic store of finished simulation outcomes, keyed by `rule_id`.
#[derive(Default)]
pub struct SimResults {
    map: DashMap<Uuid, SimEntry>,
}

impl SimResults {
    pub fn new() -> Self {
        Self::default()
    }

    fn gc(&self) {
        self.map.retain(|_, e| e.at.elapsed() < RESULT_TTL);
    }

    fn range_ts(from: Option<DateTime<Utc>>, to: Option<DateTime<Utc>>) -> (Option<i64>, Option<i64>) {
        (from.map(|t| t.timestamp()), to.map(|t| t.timestamp()))
    }

    /// Store a run's terminal outcome, first evicting stale entries. Overwrites any
    /// prior outcome for the same rule.
    pub fn insert(
        &self,
        rule_id: Uuid,
        sim_key: impl Into<String>,
        from: Option<DateTime<Utc>>,
        to: Option<DateTime<Utc>>,
        outcome: SimOutcome,
    ) {
        self.gc();
        let (from, to) = Self::range_ts(from, to);
        self.map.insert(
            rule_id,
            SimEntry {
                at: Instant::now(),
                sim_key: sim_key.into(),
                from,
                to,
                outcome,
            },
        );
    }

    /// Take (remove + return) the stored outcome for a rule. Single delivery — used
    /// by the legacy whole-blob `/result` collector.
    pub fn take(&self, rule_id: &Uuid) -> Option<SimOutcome> {
        self.map.remove(rule_id).map(|(_, e)| e.outcome)
    }

    /// Borrow the stored outcome — the Simulated table's pager reads repeatedly.
    pub fn peek(&self, rule_id: &Uuid) -> Option<SimOutcome> {
        self.map.get(rule_id).and_then(|e| {
            (e.at.elapsed() < RESULT_TTL).then(|| e.outcome.clone())
        })
    }

    /// Borrow only when the stored entry matches the current rule config + window.
    pub fn peek_if(
        &self,
        rule_id: &Uuid,
        sim_key: &str,
        from: Option<DateTime<Utc>>,
        to: Option<DateTime<Utc>>,
    ) -> Option<SimOutcome> {
        let (from, to) = Self::range_ts(from, to);
        self.map.get(rule_id).and_then(|e| {
            if e.at.elapsed() >= RESULT_TTL {
                return None;
            }
            if e.sim_key != sim_key || e.from != from || e.to != to {
                return None;
            }
            Some(e.outcome.clone())
        })
    }

    /// Drop any stored outcome for a rule — called when a fresh run starts so a
    /// stale result can't be collected for the new run.
    pub fn clear(&self, rule_id: &Uuid) {
        self.map.remove(rule_id);
    }
}
