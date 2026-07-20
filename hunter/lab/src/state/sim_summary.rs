//! Legacy in-RAM per-rule "last simulation" rollup.
//!
//! The Simulate page now hydrates columns from disk-backed
//! [`SimResults`](super::sim_results) meta (survives lab restart; no TTL). This
//! cache is unused by the hot path and kept only so older call sites compile.

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::Serialize;
use trading_core::strategies::kernel::RunSummary;
use uuid::Uuid;

/// Rollup of one finished backtest's per-token rows.
///
/// Carries the core kernel's [`RunSummary`] two-band shape verbatim rather than a hand-picked
/// subset, so the rules table's last-simulation column reads from the *same*
/// realized-only aggregate the grouped sweep and a live/paper run report. The
/// old five-scalar shape had its own `total_pnl_sol` that silently included
/// still-open marks (parity plan B4).
#[derive(Clone)]
pub struct SimSummary {
    pub metrics: RunSummary,
    pub computed_at: DateTime<Utc>,
    /// The rule's `sim_key` at computation time — compared against the rule's
    /// *current* key at read time so an edited-since-last-run rule can be
    /// flagged stale without discarding the number.
    pub sim_key: String,
}

/// Wire shape for the rules table — `sim_key` stays server-side; `is_stale`
/// is the derived comparison against the rule's current key. The metrics are
/// **flattened**, so this serializes to the shared `RunSummary` `realized`/`mtm` bands plus
/// the two rules-table-only extras.
#[derive(Serialize)]
pub struct SimSummaryView {
    #[serde(flatten)]
    pub metrics: RunSummary,
    pub computed_at: DateTime<Utc>,
    pub is_stale: bool,
}

impl SimSummary {
    pub fn view(&self, current_sim_key: &str) -> SimSummaryView {
        SimSummaryView {
            metrics: self.metrics.clone(),
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
