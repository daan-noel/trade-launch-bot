//! `m_flow` over the coin's whole life, untagged: buy / sell SOL and print counts.
//!
//! O(1) per trade: four running counters. Non-finite or negative SOL is ignored (the
//! same poison-feed guard as the window) — and a trade dropped that way is NOT counted,
//! so `trade_count` matches its window twin on the same tape.

use super::{Metric, Side};

/// Lifetime flow accumulators for one token.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FlowLifetimeState {
    buy: f64,
    sell: f64,
    trades: u64,
    buys: u64,
}

impl FlowLifetimeState {
    /// Fold one trade into the lifetime totals.
    pub fn on_trade(&mut self, side: Side, sol: f64) {
        if !sol.is_finite() || sol < 0.0 {
            return;
        }
        match side {
            Side::Buy => {
                self.buy += sol;
                self.buys += 1;
            }
            Side::Sell => self.sell += sol,
        }
        self.trades += 1;
    }

    /// One lifetime `m_flow` read. Any other metric yields `NaN`.
    pub fn value(&self, id: Metric) -> f64 {
        match id {
            Metric::BuySol => self.buy,
            Metric::SellSol => self.sell,
            Metric::GrossSol => self.buy + self.sell,
            Metric::NetSol => self.buy - self.sell,
            Metric::TradeCount => self.trades as f64,
            Metric::BuyCount => self.buys as f64,
            Metric::SellCount => (self.trades - self.buys) as f64,
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
        assert_eq!(s.value(Metric::BuySol), 5.0);
        assert_eq!(s.value(Metric::SellSol), 1.0);
        assert_eq!(s.value(Metric::GrossSol), 6.0);
        assert_eq!(s.value(Metric::NetSol), 4.0);
        assert_eq!(s.value(Metric::TradeCount), 3.0);
        assert_eq!(s.value(Metric::BuyCount), 2.0);
        assert_eq!(s.value(Metric::SellCount), 1.0);
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
            let now = s.value(Metric::TradeCount);
            assert!(now > last, "trade_count went {last} -> {now}");
            last = now;
        }
        // `net_flow` moved both ways over the same tape; the count did not.
        assert!(s.value(Metric::NetSol) < 0.0);
        assert_eq!(last, 4.0);
    }

    #[test]
    fn non_finite_or_negative_sol_ignored() {
        let mut s = FlowLifetimeState::default();
        s.on_trade(Side::Buy, f64::NAN);
        s.on_trade(Side::Buy, -1.0);
        s.on_trade(Side::Buy, 2.0);
        assert_eq!(s.value(Metric::BuySol), 2.0);
        assert_eq!(
            s.value(Metric::TradeCount),
            1.0,
            "a poisoned trade is dropped from the SOL sums, so it must not be counted either"
        );
    }
}
