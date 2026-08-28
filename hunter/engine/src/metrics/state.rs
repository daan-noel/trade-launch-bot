//! `m_state` — instantaneous, rule-independent token state (static metrics).
//!
//! * `time` — seconds since token creation. **Monotonic** (drives derived
//!   unsatisfiability); always defined (needs only the two instants).
//! * `liquidity` — SOL reserves, taken from the most recent trade's canonical
//!   `reserve_sol`. Undefined (`NaN`) until the first trade — with no market
//!   data there is no liquidity to compare, and a `NaN` satisfies no condition
//!   (evaluator contract), so a rule can never fire on absent data.
//! * `first_slot_buy` — total buy SOL that landed in the token's CREATION slot.
//!   Seeded once when the creation slot settles and never moves. Undefined (`NaN`)
//!   until then, so a rule gated on it cannot fire on a token whose launch has not
//!   been counted yet — which is also why it is a phase-`Full` fact, not a birth one.
//!
//! `ix_count` and `prior_launches` are **fingerprint axes**, not metrics
//! (`hunter_engine::fingerprint::axis`): both are fixed at creation, so they select
//! WHICH tokens a rule arms on rather than WHEN it fires, and a fact belongs to one
//! vocabulary only.
//!
//! Static state is shared by every rule armed on the token (computed once).

use super::{secs_between, MetricId, Ts};

/// Incremental `m_state` state: just the last observed reserves.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StateMetrics {
    /// SOL reserves at the most recent trade. `None` until the first trade.
    reserve_sol: Option<f64>,
    /// Buy SOL in the token's creation slot, seeded when that slot settles.
    /// `None` until then — the count does not exist yet.
    first_slot_buy: Option<f64>,
}

impl StateMetrics {
    /// Fold one trade's reserves into the snapshot (ignores non-finite values).
    pub fn on_trade(&mut self, reserve_sol: f64) {
        if reserve_sol.is_finite() {
            self.reserve_sol = Some(reserve_sol);
        }
    }

    /// Seed the creation slot's buy total (human SOL). Called once, when the slot
    /// settles — NOT at `TokenCreated`, where the number does not exist yet.
    ///
    /// A `0` here is a REAL value (a launch nobody bought into), so absence is
    /// carried by not calling this rather than by seeding `0`. A non-finite value is
    /// ignored:
    /// a poisoned feed reading must leave the metric unknown, not satisfy a `<=` gate.
    pub fn seed_first_slot_buy(&mut self, sol: f64) {
        if sol.is_finite() && sol >= 0.0 {
            self.first_slot_buy = Some(sol);
        }
    }

    /// `first_slot_buy` — creation-slot buy SOL; `NaN` until the slot settles.
    pub fn first_slot_buy(&self) -> f64 {
        self.first_slot_buy.unwrap_or(f64::NAN)
    }

    /// `time` — seconds since creation. Free function: needs no state.
    pub fn time(created_at: Ts, now: Ts) -> f64 {
        secs_between(created_at, now)
    }

    /// `liquidity` — last observed SOL reserves; `NaN` before the first trade.
    pub fn liquidity(&self) -> f64 {
        self.reserve_sol.unwrap_or(f64::NAN)
    }

    /// Value of one `m_state` metric. Non-snapshot ids yield `NaN`
    /// (unreachable — `TokenTrack` routes by group).
    pub fn value(&self, id: MetricId, created_at: Ts, now: Ts) -> f64 {
        match id {
            MetricId::Time => Self::time(created_at, now),
            MetricId::Liquidity => self.liquidity(),
            MetricId::FirstSlotBuy => self.first_slot_buy(),
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
        assert_eq!(StateMetrics::time(created, ts(0)), 0.0);
        assert_eq!(StateMetrics::time(created, ts(30)), 30.0);
        // Sub-second precision to the millisecond (500 ms ticks).
        let half = created + Duration::milliseconds(500);
        assert_eq!(StateMetrics::time(created, half), 0.5);
    }

    #[test]
    fn liquidity_is_nan_until_first_trade_then_last_reserves() {
        let mut s = StateMetrics::default();
        assert!(s.liquidity().is_nan());
        s.on_trade(12.5);
        assert_eq!(s.liquidity(), 12.5);
        s.on_trade(9.0); // most recent wins
        assert_eq!(s.liquidity(), 9.0);
    }

    /// The launch axis is unknown until the creation slot settles, and `0` is a real
    /// launch (nobody bought), so the two must stay distinguishable — a `0` standing
    /// in for "not counted yet" would let `first_slot_buy <= 1` pass on every token
    /// the instant it is born.
    #[test]
    fn first_slot_buy_is_unknown_until_seeded_and_zero_is_real() {
        let mut s = StateMetrics::default();
        assert!(s.first_slot_buy().is_nan());
        s.seed_first_slot_buy(0.0);
        assert_eq!(s.first_slot_buy(), 0.0);
        s.seed_first_slot_buy(14.25);
        assert_eq!(s.first_slot_buy(), 14.25);
        // A poisoned reading leaves the last good value rather than erasing it.
        s.seed_first_slot_buy(f64::NAN);
        assert_eq!(s.first_slot_buy(), 14.25);
        s.seed_first_slot_buy(-1.0);
        assert_eq!(s.first_slot_buy(), 14.25);
    }

    #[test]
    fn non_finite_reserves_ignored() {
        let mut s = StateMetrics::default();
        s.on_trade(5.0);
        s.on_trade(f64::NAN);
        s.on_trade(f64::INFINITY);
        assert_eq!(s.liquidity(), 5.0);
    }
}
