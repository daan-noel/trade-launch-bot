//! Long-lived per-rule "last simulation" rollup — deliberately **not** the same
//! store as [`SimResults`](super::sim_results). That cache holds the full
//! per-token row set (potentially large) behind a 10-minute TTL sized for the
//! Simulated table's interactive paging session; evicting it after 10 minutes is
//! fine because a stale miss just costs a re-run. A rollup is a handful of
//! scalars — cheap enough to keep for the life of the process — and it powers an
//! always-visible column on the rules table, so it must not vanish on the same
//! timer for reasons the user can't see. Kept until the rule is re-simulated (or
//! the process restarts); no DB write (lab is single-user, workstation RAM — see
//! `sim_results` for the same rationale).

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::Serialize;
use uuid::Uuid;

/// Rollup of one finished backtest's per-token rows.
#[derive(Clone)]
pub struct SimSummary {
    pub n_fired: usize,
    pub closed: usize,
    pub win_rate: f64,
    pub avg_pnl_pct: f64,
    pub total_pnl_sol: f64,
    pub computed_at: DateTime<Utc>,
    /// The rule's `sim_key` at computation time — compared against the rule's
    /// *current* key at read time so an edited-since-last-run rule can be
    /// flagged stale without discarding the number.
    pub sim_key: String,
}

/// Wire shape for the rules table — `sim_key` stays server-side; `is_stale`
/// is the derived comparison against the rule's current key.
#[derive(Serialize)]
pub struct SimSummaryView {
    pub n_fired: usize,
    pub closed: usize,
    pub win_rate: f64,
    pub avg_pnl_pct: f64,
    pub total_pnl_sol: f64,
    pub computed_at: DateTime<Utc>,
    pub is_stale: bool,
}

impl SimSummary {
    pub fn view(&self, current_sim_key: &str) -> SimSummaryView {
        SimSummaryView {
            n_fired: self.n_fired,
            closed: self.closed,
            win_rate: self.win_rate,
            avg_pnl_pct: self.avg_pnl_pct,
            total_pnl_sol: self.total_pnl_sol,
            computed_at: self.computed_at,
            is_stale: self.sim_key != current_sim_key,
        }
    }
}

/// Strategy-agnostic store of last-simulation rollups, keyed by `rule_id` (UUIDs
/// never collide across tpsl1/tpsl2/swing1, same as `SimResults`).
#[derive(Default)]
pub struct SimSummaryCache {
    map: DashMap<Uuid, SimSummary>,
}

impl SimSummaryCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, rule_id: Uuid, summary: SimSummary) {
        self.map.insert(rule_id, summary);
    }

    pub fn get(&self, rule_id: &Uuid) -> Option<SimSummary> {
        self.map.get(rule_id).map(|e| e.clone())
    }
}
