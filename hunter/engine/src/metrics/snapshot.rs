//! `m_snapshot` — instantaneous, rule-independent token state (static metrics).
//!
//! * `time` — seconds since token creation. **Monotonic** (drives derived
//!   unsatisfiability); always defined (needs only the two instants).
//! * `liquidity` — SOL reserves, taken from the most recent trade's canonical
//!   `reserve_sol`. Undefined (`NaN`) until the first trade — with no market
//!   data there is no liquidity to compare, and a `NaN` satisfies no condition
//!   (evaluator contract), so a rule can never fire on absent data.
//! * `ix_count` — how many instructions the token's CREATION transaction carried.
//!   Seeded once from `TokenCreated` and never moves, so it is a token property, not
//!   a market reading. Undefined (`NaN`) when the creation fingerprint carried no
//!   labels, which is how an unknown launch stays unmatched by any `ix_count` gate.
//! * `prior_launches` — how many tokens the creator launched before this one. Seeded
//!   once from the reducer's running per-creator tally and never moves. Undefined
//!   (`NaN`) when the creator is unknown, so an unknown creator stays unmatched
//!   rather than reading as a first launch.
//!
//! Static state is shared by every rule armed on the token (computed once).

use super::{secs_between, MetricId, Ts};

/// Incremental `m_snapshot` state: just the last observed reserves.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SnapshotState {
    /// SOL reserves at the most recent trade. `None` until the first trade.
    reserve_sol: Option<f64>,
    /// Instruction count of the creation transaction, seeded from `TokenCreated`.
    /// `None` when the fingerprint carried no labels.
    ix_count: Option<u32>,
    /// Creator's launches strictly before this token, seeded from `TokenCreated`.
    /// `None` when the creator is unknown.
    prior_launches: Option<u32>,
}

impl SnapshotState {
    /// Fold one trade's reserves into the snapshot (ignores non-finite values).
    pub fn on_trade(&mut self, reserve_sol: f64) {
        if reserve_sol.is_finite() {
            self.reserve_sol = Some(reserve_sol);
        }
    }

    /// Seed the creation-transaction instruction count. Called once, from
    /// `TokenCreated`; an empty label sequence stays `None` ("unknown launch") rather
    /// than becoming a real `0`, which would satisfy an `ix_count <= 5` gate.
    pub fn seed_ix_count(&mut self, n: usize) {
        if n > 0 {
            self.ix_count = Some(n as u32);
        }
    }

    /// `ix_count` — creation-transaction instruction count; `NaN` when unknown.
    pub fn ix_count(&self) -> f64 {
        self.ix_count.map_or(f64::NAN, f64::from)
    }

    /// Seed the creator's prior-launch count. Called once, from `TokenCreated`.
    ///
    /// Unlike [`seed_ix_count`](Self::seed_ix_count) a `0` here is a REAL value — the
    /// creator's first launch is the whole point of the metric — so absence has to be
    /// carried by not calling this at all, never by seeding `0`.
    pub fn seed_prior_launches(&mut self, n: u32) {
        self.prior_launches = Some(n);
    }

    /// `prior_launches` — the creator's launches before this token; `NaN` when the
    /// creator is unknown.
    pub fn prior_launches(&self) -> f64 {
        self.prior_launches.map_or(f64::NAN, f64::from)
    }

    /// `time` — seconds since creation. Free function: needs no state.
    pub fn time(created_at: Ts, now: Ts) -> f64 {
        secs_between(created_at, now)
    }

    /// `liquidity` — last observed SOL reserves; `NaN` before the first trade.
    pub fn liquidity(&self) -> f64 {
        self.reserve_sol.unwrap_or(f64::NAN)
    }

    /// Value of one `m_snapshot` metric. Non-snapshot ids yield `NaN`
    /// (unreachable — `TokenTrack` routes by group).
    pub fn value(&self, id: MetricId, created_at: Ts, now: Ts) -> f64 {
        match id {
            MetricId::Time => Self::time(created_at, now),
            MetricId::Liquidity => self.liquidity(),
            MetricId::IxCount => self.ix_count(),
            MetricId::PriorLaunches => self.prior_launches(),
            _ => f64::NAN,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone, Utc};

    fn ts(secs: i64) -> Ts {
        Utc.timestamp_opt(1_700_000_000, 0).unwrap() + Duration::seconds(secs)
    }

    #[test]
    fn time_is_seconds_since_creation() {
        let created = ts(0);
        assert_eq!(SnapshotState::time(created, ts(0)), 0.0);
        assert_eq!(SnapshotState::time(created, ts(30)), 30.0);
        // Sub-second precision to the millisecond (500 ms ticks).
        let half = created + Duration::milliseconds(500);
        assert_eq!(SnapshotState::time(created, half), 0.5);
    }

    #[test]
    fn liquidity_is_nan_until_first_trade_then_last_reserves() {
        let mut s = SnapshotState::default();
        assert!(s.liquidity().is_nan());
        s.on_trade(12.5);
        assert_eq!(s.liquidity(), 12.5);
        s.on_trade(9.0); // most recent wins
        assert_eq!(s.liquidity(), 9.0);
    }

    /// `0` prior launches is the metric's most useful value, so it must survive the
    /// round-trip as `0.0` and not be swallowed as "unknown" the way `ix_count`'s
    /// empty-label case is.
    #[test]
    fn prior_launches_zero_is_a_real_value_not_absence() {
        let mut s = SnapshotState::default();
        assert!(s.prior_launches().is_nan(), "unseeded creator is unknown, not first");
        s.seed_prior_launches(0);
        assert_eq!(s.prior_launches(), 0.0);
        s.seed_prior_launches(137);
        assert_eq!(s.prior_launches(), 137.0);
    }

    #[test]
    fn non_finite_reserves_ignored() {
        let mut s = SnapshotState::default();
        s.on_trade(5.0);
        s.on_trade(f64::NAN);
        s.on_trade(f64::INFINITY);
        assert_eq!(s.liquidity(), 5.0);
    }
}
