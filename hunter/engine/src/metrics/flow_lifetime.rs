//! `m_flow_lifetime` — lifetime (since-token-birth) flow aggregates (static metrics).
//!
//! Sibling of [`super::flow_window`] with the same JSON metric names
//! (`buy` / `sell` / `gross_flow` / `net_flow`) but no trailing window — totals
//! only grow. No fingerprint config (unlike `m_flow_split`).
//!
//! * `buy` — sum of buy SOL,
//! * `sell` — sum of sell SOL,
//! * `gross_flow` — `buy + sell` (total churn),
//! * `net_flow` — `buy − sell` (directional pressure).
//!
//! O(1) per trade: two running counters. Non-finite or negative SOL is ignored
//! (same poison-feed guard as the window group).

use super::{MetricId, Side};

/// Lifetime flow accumulators for one token.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FlowLifetimeState {
    buy: f64,
    sell: f64,
}

impl FlowLifetimeState {
    /// Fold one trade into the lifetime totals.
    pub fn on_trade(&mut self, side: Side, sol: f64) {
        if !sol.is_finite() || sol < 0.0 {
            return;
        }
        match side {
            Side::Buy => self.buy += sol,
            Side::Sell => self.sell += sol,
        }
    }

    /// Value of one `m_flow_lifetime` metric. Non-lifetime ids yield `NaN`
    /// (unreachable — `TokenTrack` routes by group).
    pub fn value(&self, id: MetricId) -> f64 {
        match id {
            MetricId::LifeBuy => self.buy,
            MetricId::LifeSell => self.sell,
            MetricId::LifeGrossFlow => self.buy + self.sell,
            MetricId::LifeNetFlow => self.buy - self.sell,
            _ => f64::NAN,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flows_accumulate_for_life() {
        let mut s = FlowLifetimeState::default();
        s.on_trade(Side::Buy, 3.0);
        s.on_trade(Side::Sell, 1.0);
        s.on_trade(Side::Buy, 2.0);
        assert_eq!(s.value(MetricId::LifeBuy), 5.0);
        assert_eq!(s.value(MetricId::LifeSell), 1.0);
        assert_eq!(s.value(MetricId::LifeGrossFlow), 6.0);
        assert_eq!(s.value(MetricId::LifeNetFlow), 4.0);
    }

    #[test]
    fn non_finite_or_negative_sol_ignored() {
        let mut s = FlowLifetimeState::default();
        s.on_trade(Side::Buy, f64::NAN);
        s.on_trade(Side::Buy, -1.0);
        s.on_trade(Side::Buy, 2.0);
        assert_eq!(s.value(MetricId::LifeBuy), 2.0);
    }
}
