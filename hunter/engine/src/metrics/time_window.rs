//! `m_time_window` — trailing-window flow aggregates (dynamic metrics).
//!
//! Strict param `window_size_sec` (`w`): the trailing window is `(now − w, now]`.
//! Over the trades in that window:
//! * `buy` — sum of buy SOL,
//! * `sell` — sum of sell SOL,
//! * `gross_flow` — `buy + sell` (total churn),
//! * `net_flow` — `buy − sell` (directional pressure).
//!
//! **Dynamic** = the value depends on a per-rule strict param, so state is
//! **deduped by `window_size_sec`**: every rule asking for a 10 s window shares
//! one buffer. A ring buffer of `(ts, signed_sol)` with running `buy`/`sell`
//! sums keeps both fold and read O(1) amortized, no per-event allocation.

use std::collections::VecDeque;

use chrono::Duration;

use super::{MetricId, Side, Ts};

/// Dedup key for a window size. Window sizes come from rule params (finite,
/// `> 0`), so millisecond-rounding gives a stable integer identity that two
/// rules requesting the same window collapse onto.
pub fn window_key(window_secs: f64) -> u64 {
    (window_secs * 1000.0).round().max(0.0) as u64
}

/// One trailing-window aggregator for a single `window_size_sec`.
#[derive(Debug, Clone)]
pub struct WindowState {
    window_secs: f64,
    /// `(timestamp, signed SOL)` — buy positive, sell negative; oldest at front.
    buf: VecDeque<(Ts, f64)>,
    /// Running sums over `buf` (kept in sync on push/evict), so reads are O(1).
    buy: f64,
    sell: f64,
}

impl WindowState {
    pub fn new(window_secs: f64) -> Self {
        Self { window_secs, buf: VecDeque::new(), buy: 0.0, sell: 0.0 }
    }

    /// The window this aggregator tracks.
    pub fn window_secs(&self) -> f64 {
        self.window_secs
    }

    /// Fold one trade, then drop anything that fell out of the window at `at`.
    /// Non-finite or negative SOL is ignored (guards a poisoned feed value).
    pub fn on_trade(&mut self, side: Side, sol: f64, at: Ts) {
        if !sol.is_finite() || sol < 0.0 {
            return;
        }
        match side {
            Side::Buy => {
                self.buf.push_back((at, sol));
                self.buy += sol;
            }
            Side::Sell => {
                self.buf.push_back((at, -sol));
                self.sell += sol;
            }
        }
        self.evict(at);
    }

    /// Drop entries older than the window as of `now` — trailing window is
    /// `(now − w, now]`, so an entry exactly `w` old stays. Called on every
    /// trade and every tick so reads always see the window as of the last event.
    pub fn evict(&mut self, now: Ts) {
        let width = Duration::milliseconds(window_key(self.window_secs) as i64);
        let cutoff = now - width;
        while let Some(&(ts, signed)) = self.buf.front() {
            if ts < cutoff {
                self.buf.pop_front();
                if signed >= 0.0 {
                    self.buy -= signed;
                } else {
                    self.sell += signed; // signed < 0 ⇒ subtracts from the sell sum
                }
            } else {
                break;
            }
        }
    }

    /// Value of one `m_time_window` metric over the current window contents.
    pub fn value(&self, id: MetricId) -> f64 {
        match id {
            MetricId::Buy => self.buy,
            MetricId::Sell => self.sell,
            MetricId::GrossFlow => self.buy + self.sell,
            MetricId::NetFlow => self.buy - self.sell,
            _ => f64::NAN,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn ts(secs: f64) -> Ts {
        Utc.timestamp_opt(1_700_000_000, 0).unwrap()
            + Duration::milliseconds((secs * 1000.0) as i64)
    }

    #[test]
    fn window_key_dedupes_equal_sizes() {
        assert_eq!(window_key(10.0), window_key(10.0));
        assert_eq!(window_key(10.0), 10_000);
        assert_ne!(window_key(10.0), window_key(5.0));
    }

    #[test]
    fn flows_sum_over_the_window() {
        let mut w = WindowState::new(10.0);
        w.on_trade(Side::Buy, 3.0, ts(0.0));
        w.on_trade(Side::Sell, 1.0, ts(1.0));
        w.on_trade(Side::Buy, 2.0, ts(2.0));
        assert_eq!(w.value(MetricId::Buy), 5.0);
        assert_eq!(w.value(MetricId::Sell), 1.0);
        assert_eq!(w.value(MetricId::GrossFlow), 6.0);
        assert_eq!(w.value(MetricId::NetFlow), 4.0);
    }

    #[test]
    fn old_trades_fall_out_of_the_window() {
        let mut w = WindowState::new(10.0);
        w.on_trade(Side::Buy, 5.0, ts(0.0));
        // Boundary: an entry exactly 10 s old is still in (cutoff is exclusive).
        w.evict(ts(10.0));
        assert_eq!(w.value(MetricId::Buy), 5.0);
        // One millisecond past the window edge → it drops.
        w.evict(ts(10.001));
        assert_eq!(w.value(MetricId::Buy), 0.0);
        assert_eq!(w.value(MetricId::GrossFlow), 0.0);
    }

    #[test]
    fn eviction_keeps_running_sums_correct() {
        let mut w = WindowState::new(5.0);
        w.on_trade(Side::Buy, 1.0, ts(0.0));
        w.on_trade(Side::Sell, 2.0, ts(3.0));
        // A new trade at t=6 pushes t=0 out of the 5 s window.
        w.on_trade(Side::Buy, 4.0, ts(6.0));
        assert_eq!(w.value(MetricId::Buy), 4.0); // t=0 buy gone
        assert_eq!(w.value(MetricId::Sell), 2.0); // t=3 sell still in
        assert_eq!(w.value(MetricId::NetFlow), 2.0);
    }

    #[test]
    fn non_finite_or_negative_sol_ignored() {
        let mut w = WindowState::new(10.0);
        w.on_trade(Side::Buy, f64::NAN, ts(0.0));
        w.on_trade(Side::Buy, -1.0, ts(0.0));
        w.on_trade(Side::Buy, 2.0, ts(0.0));
        assert_eq!(w.value(MetricId::Buy), 2.0);
    }
}
