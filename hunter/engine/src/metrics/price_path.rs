//! `m_price_path` — incremental price-path state (static metrics).
//!
//! * `stall` — seconds since the price last **moved** (changed in either
//!   direction). The move clock starts at token creation, so before the first
//!   trade `stall` reads as "seconds quiet since birth"; the first trade (price
//!   appearing) counts as a move and resets it.
//! * `trail` — percent the current price sits **below its peak**
//!   (`(peak − current) / peak · 100`), the classic trailing-drawdown reading.
//!   Undefined (`NaN`) until the first trade — no peak, nothing to trail from.
//!
//! Price is the canonical curve-spot price (same value the exit ladder folds).
//! Peak/last-price are tracked incrementally — O(1) per trade, no history.

use super::{secs_between, MetricId, Ts};

/// Incremental `m_price_path` state.
#[derive(Debug, Clone, PartialEq)]
pub struct PricePathState {
    /// Highest price seen so far. `None` until the first trade.
    peak_price: Option<f64>,
    /// Most recently observed price. `None` until the first trade.
    last_price: Option<f64>,
    /// Instant the price last changed (init = creation time).
    last_move_at: Ts,
}

impl PricePathState {
    /// Fresh state for a token created at `created_at`.
    pub fn new(created_at: Ts) -> Self {
        Self { peak_price: None, last_price: None, last_move_at: created_at }
    }

    /// Fold one trade's price (ignores non-finite prices). A price differing
    /// from the last observed one — including the very first — is a "move".
    pub fn on_trade(&mut self, price: f64, at: Ts) {
        if !price.is_finite() {
            return;
        }
        if self.last_price != Some(price) {
            self.last_move_at = at;
        }
        self.last_price = Some(price);
        self.peak_price = Some(match self.peak_price {
            Some(peak) => peak.max(price),
            None => price,
        });
    }

    /// `stall` — seconds since the price last moved.
    pub fn stall(&self, now: Ts) -> f64 {
        secs_between(self.last_move_at, now)
    }

    /// `trail` — percent below the peak; `NaN` before the first trade or if the
    /// peak is non-positive (no meaningful drawdown reference).
    pub fn trail(&self) -> f64 {
        match (self.peak_price, self.last_price) {
            (Some(peak), Some(current)) if peak > 0.0 => (peak - current) / peak * 100.0,
            _ => f64::NAN,
        }
    }

    /// Value of one `m_price_path` metric. Non-price-path ids yield `NaN`
    /// (unreachable — `TokenTrack` routes by group).
    pub fn value(&self, id: MetricId, now: Ts) -> f64 {
        match id {
            MetricId::Stall => self.stall(now),
            MetricId::Trail => self.trail(),
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
    fn stall_counts_from_creation_then_resets_on_moves() {
        let created = ts(0);
        let mut s = PricePathState::new(created);
        // No trade yet: quiet since birth.
        assert_eq!(s.stall(ts(5)), 5.0);
        // First trade at t=5 is a move → clock resets.
        s.on_trade(1.0, ts(5));
        assert_eq!(s.stall(ts(8)), 3.0);
        // Same price at t=8 is NOT a move → clock keeps running from t=5.
        s.on_trade(1.0, ts(8));
        assert_eq!(s.stall(ts(10)), 5.0);
        // Different price at t=10 resets.
        s.on_trade(2.0, ts(10));
        assert_eq!(s.stall(ts(12)), 2.0);
    }

    #[test]
    fn trail_is_percent_below_peak() {
        let mut s = PricePathState::new(ts(0));
        assert!(s.trail().is_nan()); // no data yet
        s.on_trade(10.0, ts(1));
        assert_eq!(s.trail(), 0.0); // at the peak
        s.on_trade(12.0, ts(2)); // new peak, still at peak
        assert_eq!(s.trail(), 0.0);
        s.on_trade(9.0, ts(3)); // 25% below peak of 12
        assert!((s.trail() - 25.0).abs() < 1e-9);
    }

    #[test]
    fn non_finite_price_ignored() {
        let mut s = PricePathState::new(ts(0));
        s.on_trade(10.0, ts(1));
        s.on_trade(f64::NAN, ts(2));
        assert_eq!(s.trail(), 0.0);
        assert_eq!(s.stall(ts(2)), 1.0); // NaN trade did not count as a move
    }
}
