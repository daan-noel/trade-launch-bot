//! The arm ledger's row + funnel models.
//!
//! An **arming episode** is one `(rule, mint)` pair from the moment the engine
//! arms on it to whatever ends it. A position is what an episode that ends in
//! `entered` becomes; every other ending is a token the rule watched and did not
//! buy — the half of a rule's behavior `strategy_positions` cannot show.
//!
//! Plan: `docs/plans/strategies/arm-ledger.md`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Why an arming episode ended. The engine's `DisarmReason` vocabulary plus
/// `entered` — the Enter path emits no `ArmedChanged`, so the position sink
/// closes that episode itself.
///
/// The strings are the wire form, the DB `CHECK`, and the frontend's filter
/// operands; `live`'s `disarm_reason_str` maps the engine enum onto them, so
/// this list and that mapper are the same vocabulary in two crates. The
/// `end_reason_vocabulary_matches_disarm_reason` guard in `live` pins them.
pub const ARM_END_REASONS: [&str; 6] =
    ["entered", "dead", "migrated", "unsatisfiable", "paused", "duplicate_identity"];

/// One arming episode, as the Console Arms section reads it.
///
/// `waited_sec` is server-computed off the one SQL expression the sort and the
/// filter whitelists share, so the column cannot sort by one fact and filter by
/// another. It runs to `now()` while the episode is live.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct StrategyArm {
    pub rule_id: Uuid,
    pub mint_address: String,
    /// `real` | `paper`, frozen at arm time.
    pub mode: String,
    pub armed_at: DateTime<Utc>,
    /// `None` while the engine is still evaluating entry on this pair.
    pub ended_at: Option<DateTime<Utc>>,
    /// One of [`ARM_END_REASONS`]; `None` while live.
    pub end_reason: Option<String>,
    /// The position this episode became — set only with `end_reason = "entered"`.
    pub position_id: Option<Uuid>,
    /// Token symbol via the `tokens` LEFT JOIN (`None` when the token row is
    /// gone — `tokens` and this ledger expire on different clocks).
    pub symbol: Option<String>,
    /// Seconds from `armed_at` to `ended_at`, or to now while live.
    pub waited_sec: Option<f64>,
}

/// The arm funnel over a cohort — how selective a rule is, in one row.
///
/// `armed` is every episode in the window (live ones included), so
/// `armed - entered - Σ(disarms)` is exactly the count still waiting. Counts are
/// aggregated in Postgres; no rows ship for this.
#[derive(Debug, Clone, Default, Serialize, Deserialize, sqlx::FromRow)]
pub struct ArmFunnel {
    /// Every episode armed in the window.
    pub armed: i64,
    /// Episodes that became a position.
    pub entered: i64,
    /// Still evaluating entry (`ended_at IS NULL`).
    pub live: i64,
    pub dead: i64,
    pub migrated: i64,
    pub unsatisfiable: i64,
    pub paused: i64,
    pub duplicate_identity: i64,
    /// `entered / armed × 100` — the rule's hit rate on what it armed. `0` when
    /// nothing armed. Computed here rather than in SQL so the zero-denominator
    /// case has one answer.
    pub entry_rate_pct: f64,
    /// Median seconds an episode waited before ending. `None` when nothing ended.
    pub median_waited_sec: Option<f64>,
}

impl ArmFunnel {
    /// Fill the derived rate from the counts. The repo builds the counts in one
    /// aggregate query and calls this, so the division lives in exactly one place.
    pub fn with_entry_rate(mut self) -> Self {
        self.entry_rate_pct =
            if self.armed > 0 { self.entered as f64 / self.armed as f64 * 100.0 } else { 0.0 };
        self
    }
}

/// One ledger write, queued by the engine sink and drained by the writer task.
///
/// `Armed` and `Ended` carry the full episode key (`rule_id`, `mint_address`,
/// `armed_at`) because the writer batches both sides and may see them in the same
/// flush — it resolves an end against a not-yet-inserted arm without a read.
#[derive(Debug, Clone)]
pub enum ArmLedgerWrite {
    Armed { rule_id: Uuid, mint_address: String, mode: String, armed_at: DateTime<Utc> },
    Ended {
        rule_id: Uuid,
        mint_address: String,
        armed_at: DateTime<Utc>,
        ended_at: DateTime<Utc>,
        end_reason: String,
        position_id: Option<Uuid>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_rate_has_one_answer_at_zero() {
        assert_eq!(ArmFunnel::default().with_entry_rate().entry_rate_pct, 0.0);
    }

    #[test]
    fn entry_rate_is_entered_over_armed() {
        let f = ArmFunnel { armed: 400, entered: 20, ..Default::default() }.with_entry_rate();
        assert!((f.entry_rate_pct - 5.0).abs() < 1e-9);
    }
}
