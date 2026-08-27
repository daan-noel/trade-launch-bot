//! `m_flow_lifetime` — lifetime (since-token-birth) flow aggregates (static metrics).
//!
//! Sibling of [`super::flow_window`] with the same JSON metric names
//! (`buy` / `sell` / `gross_flow` / `net_flow` / `trade_count`) but no trailing
//! window — totals only grow. No fingerprint config (unlike `m_flow_split`).
//!
//! * `buy` — sum of buy SOL,
//! * `sell` — sum of sell SOL,
//! * `gross_flow` — `buy + sell` (total churn),
//! * `net_flow` — `buy − sell` (directional pressure),
//! * `trade_count` — how many trades landed.
//!
//! O(1) per trade: three running counters. Non-finite or negative SOL is ignored
//! (same poison-feed guard as the window group) — and a trade dropped that way is
//! NOT counted, so `trade_count` matches its window sibling on the same tape.

use super::{MetricId, Side};

/// Lifetime flow accumulators for one token.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FlowLifetimeState {
    buy: f64,
    sell: f64,
    trades: u64,
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
        self.trades += 1;
    }

    /// Value of one `m_flow_lifetime` metric. Non-lifetime ids yield `NaN`
    /// (unreachable — `TokenTrack` routes by group).
    pub fn value(&self, id: MetricId) -> f64 {
        match id {
            MetricId::LifeBuy => self.buy,
            MetricId::LifeSell => self.sell,
            MetricId::LifeGrossFlow => self.buy + self.sell,
            MetricId::LifeNetFlow => self.buy - self.sell,
            MetricId::LifeTradeCount => self.trades as f64,
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
        assert_eq!(s.value(MetricId::LifeTradeCount), 3.0);
    }

    /// The property an upper bound depends on: `trade_count` only ever grows, so
    /// `<= N` is a one-way door and the arm is disarmed as unsatisfiable rather than
    /// re-checked once a token has crossed it.
    #[test]
    fn lifetime_trade_count_only_grows() {
        let mut s = FlowLifetimeState::default();
        let mut last = 0.0;
        for (side, sol) in [(Side::Buy, 3.0), (Side::Sell, 9.0), (Side::Sell, 0.01), (Side::Buy, 1.0)] {
            s.on_trade(side, sol);
            let now = s.value(MetricId::LifeTradeCount);
            assert!(now > last, "trade_count went {last} -> {now}");
            last = now;
        }
        // `net_flow` moved both ways over the same tape; the count did not.
        assert!(s.value(MetricId::LifeNetFlow) < 0.0);
        assert_eq!(last, 4.0);
    }

    #[test]
    fn non_finite_or_negative_sol_ignored() {
        let mut s = FlowLifetimeState::default();
        s.on_trade(Side::Buy, f64::NAN);
        s.on_trade(Side::Buy, -1.0);
        s.on_trade(Side::Buy, 2.0);
        assert_eq!(s.value(MetricId::LifeBuy), 2.0);
        assert_eq!(
            s.value(MetricId::LifeTradeCount),
            1.0,
            "a poisoned trade is dropped from the SOL sums, so it must not be counted either"
        );
    }
}
