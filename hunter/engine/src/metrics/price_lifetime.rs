//! `m_price_lifetime` — incremental price-path state (static metrics).
//!
//! * `stall` — seconds since the price last set a **new all-time high**. The
//!   peak clock starts at token creation, so before the first trade `stall`
//!   reads as "seconds quiet since birth"; the first trade (which establishes
//!   the peak) resets it. Only a strictly higher price resets it thereafter —
//!   a trade at or below the peak lets the clock keep running, which is what
//!   makes `stall` read "how long since this token last made progress".
//! * `trail` — percent the current price sits **below its peak**
//!   (`(peak − current) / peak · 100`), the classic trailing-drawdown reading.
//!   Undefined (`NaN`) until the first trade — no peak, nothing to trail from.
//!
//! Price is the canonical curve-spot price (same value the exit ladder folds).
//! Peak/last-price are tracked incrementally — O(1) per trade, no history.

use super::{secs_between, MetricId, Ts};

/// Incremental `m_price_lifetime` state.
#[derive(Debug, Clone, PartialEq)]
pub struct PriceLifetimeState {
    /// Highest price seen so far. `None` until the first trade.
    peak_price: Option<f64>,
    /// Most recently observed price. `None` until the first trade.
    last_price: Option<f64>,
    /// Instant the peak was last set (init = creation time).
    peak_at: Ts,
}

impl PriceLifetimeState {
    /// Fresh state for a token created at `created_at`.
    pub fn new(created_at: Ts) -> Self {
        Self { peak_price: None, last_price: None, peak_at: created_at }
    }

    /// Fold one trade's price (ignores non-finite prices). A price strictly above
    /// the running peak — including the very first — sets a new all-time high and
    /// restarts the stall clock.
    pub fn on_trade(&mut self, price: f64, at: Ts) {
        if !price.is_finite() {
            return;
        }
        let new_high = match self.peak_price {
            Some(peak) => price > peak,
            None => true,
        };
        if new_high {
            self.peak_price = Some(price);
            self.peak_at = at;
        }
        self.last_price = Some(price);
    }

    /// Most recently observed price (`NaN` before the first trade) — the "last
    /// known price" a tick-driven TP/SL check reads.
    pub fn last_price(&self) -> f64 {
        self.last_price.unwrap_or(f64::NAN)
    }

    /// `stall` — seconds since the price last set a new all-time high.
    ///
    /// Floored at zero: corpus rows are ordered by **slot**, and Solana's
    /// `block_time` is a stake-weighted estimate that can regress a few seconds
    /// across slots, so an ATH can carry a timestamp slightly ahead of the rows
    /// that follow it. Without the floor that leaked through as negative stall
    /// (a "quiet time" reading below zero is never meaningful).
    pub fn stall(&self, now: Ts) -> f64 {
        secs_between(self.peak_at, now).max(0.0)
    }

    /// `trail` — percent below the peak; `NaN` before the first trade or if the
    /// peak is non-positive (no meaningful drawdown reference).
    pub fn trail(&self) -> f64 {
        match (self.peak_price, self.last_price) {
            (Some(peak), Some(current)) if peak > 0.0 => (peak - current) / peak * 100.0,
            _ => f64::NAN,
        }
    }

    /// Value of one `m_price_lifetime` metric. Non-price-lifetime ids yield `NaN`
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
    fn stall_counts_from_creation_then_resets_only_on_new_highs() {
        let created = ts(0);
        let mut s = PriceLifetimeState::new(created);
        // No trade yet: quiet since birth.
        assert_eq!(s.stall(ts(5)), 5.0);
        // First trade at t=5 establishes the peak → clock resets.
        s.on_trade(1.0, ts(5));
        assert_eq!(s.stall(ts(8)), 3.0);
        // Same price at t=8 is not a new high → clock keeps running from t=5.
        s.on_trade(1.0, ts(8));
        assert_eq!(s.stall(ts(10)), 5.0);
        // A HIGHER price at t=10 is a new ATH → resets.
        s.on_trade(2.0, ts(10));
        assert_eq!(s.stall(ts(12)), 2.0);
        // A LOWER price at t=14 is not a new high → clock keeps running from t=10.
        s.on_trade(1.5, ts(14));
        assert_eq!(s.stall(ts(16)), 6.0);
        // Back above the old peak at t=18 → resets again.
        s.on_trade(2.5, ts(18));
        assert_eq!(s.stall(ts(20)), 2.0);
    }

    #[test]
    fn stall_never_reads_negative_on_regressed_block_time() {
        let mut s = PriceLifetimeState::new(ts(0));
        // ATH stamped at t=10; a following row carries an earlier block_time.
        s.on_trade(5.0, ts(10));
        assert_eq!(s.stall(ts(4)), 0.0);
    }

    #[test]
    fn trail_is_percent_below_peak() {
        let mut s = PriceLifetimeState::new(ts(0));
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
        let mut s = PriceLifetimeState::new(ts(0));
        s.on_trade(10.0, ts(1));
        s.on_trade(f64::NAN, ts(2));
        assert_eq!(s.trail(), 0.0);
        assert_eq!(s.stall(ts(2)), 1.0); // NaN trade did not count as a new high
    }
}
