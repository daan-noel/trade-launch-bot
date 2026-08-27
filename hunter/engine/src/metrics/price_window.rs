//! `m_price_window` — trailing-window price extrema (dynamic metrics).
//!
//! Strict param `window_size_sec` (`w`): the trailing window is `[now − w, now]`
//! and **includes the current trade**. Over the trade prices in that window:
//! * `trail` — percent the current price sits **below the window high**:
//!   `(win_high − price) / win_high · 100`, `>= 0`. The dip trigger — entry
//!   condition `trail >= 12` with `window_size_sec: 30` means "12%+ below the 30 s
//!   high".
//! * `rise` — percent the current price sits **above the window low**:
//!   `(price − win_low) / win_low · 100`, `>= 0` (symmetric; breakout/momentum
//!   entries later).
//!
//! Sibling of the lifetime [`PriceLifetimeState`](super::price_lifetime::PriceLifetimeState):
//! `m_price_lifetime.trail` anchors on the **lifetime** peak, this on a **rolling**
//! one, which is why the two share the metric name `trail` across distinct groups
//! (the `m_flow_split` / `m_flow_split_window` precedent).
//!
//! **Dynamic** = the value depends on a per-rule strict param, so state is
//! **deduped by `window_size_sec`**. Two monotonic deques of `(price, at)` — one
//! decreasing (front = window max), one increasing (front = window min) — keep
//! both the extremum read and the push O(1) amortized with no per-event allocation
//! after warmup (`VecDeque` reuse). Reads filter deque entries to `[now − w, now]`
//! (via [`super::flow_window::in_window`]) so a regressed `block_time` on a
//! later-processed trade cannot pull in future-dated prints from earlier slots.

use std::collections::VecDeque;

use chrono::Duration;

use super::flow_window::window_width;
use super::{MetricId, Ts};

/// One trailing-window extrema tracker for a single `window_size_sec`.
#[derive(Debug, Clone)]
pub struct PriceWindowState {
    window_secs: f64,
    /// Precomputed window width. The reads below filter three deque walks by
    /// `[now − w, now]`; deriving the width per **element** (what the shared
    /// `in_window` does) put a float multiply + round inside every one of them.
    width: Duration,
    /// Monotonic **decreasing** by price (front = window max); newest at back.
    max_deque: VecDeque<(f64, Ts)>,
    /// Monotonic **increasing** by price (front = window min); newest at back.
    min_deque: VecDeque<(f64, Ts)>,
}

impl PriceWindowState {
    pub fn new(window_secs: f64) -> Self {
        Self {
            window_secs,
            width: window_width(window_secs),
            max_deque: VecDeque::new(),
            min_deque: VecDeque::new(),
        }
    }

    /// Whether `at` lies in this tracker's window at `now` — the per-instance twin
    /// of [`super::flow_window::in_window`], reusing the precomputed width.
    #[inline]
    fn holds(&self, at: Ts, now: Ts) -> bool {
        at >= now - self.width && at <= now
    }

    /// The window this tracker covers.
    pub fn window_secs(&self) -> f64 {
        self.window_secs
    }

    /// Fold one trade's price, then drop anything that fell out of the window at
    /// `at`. Non-finite prices are ignored (guards a poisoned feed value). The just
    /// pushed price becomes the back of both deques — the "current price" reads.
    pub fn on_trade(&mut self, price: f64, at: Ts) {
        if !price.is_finite() {
            return;
        }
        // Maintain the decreasing max deque: pop dominated (<= new) tail, push.
        while let Some(&(p, _)) = self.max_deque.back() {
            if p <= price {
                self.max_deque.pop_back();
            } else {
                break;
            }
        }
        self.max_deque.push_back((price, at));
        // Maintain the increasing min deque: pop dominated (>= new) tail, push.
        while let Some(&(p, _)) = self.min_deque.back() {
            if p >= price {
                self.min_deque.pop_back();
            } else {
                break;
            }
        }
        self.min_deque.push_back((price, at));
        self.evict(at);
    }

    /// Drop entries older than the window as of `now` — trailing window is
    /// `[now − w, now]`, so an entry exactly `w` old stays. Called on every trade
    /// and every tick so reads always see the window as of the last event.
    ///
    /// Comparison-only (`at < cutoff`), so a regressed `block_time` (Solana's
    /// stake-weighted estimate can slip a few seconds across slots) only shrinks the
    /// eviction set — it never panics, never clears a still-in-window deque, and
    /// never reads negative. The newest trade carries the largest `at`, so it is
    /// evicted last; while either deque is non-empty its back is a valid current
    /// price (mirrors the `stall` floor precedent in `PriceLifetimeState`).
    pub fn evict(&mut self, now: Ts) {
        let cutoff = now - self.width;
        while let Some(&(_, at)) = self.max_deque.front() {
            if at < cutoff {
                self.max_deque.pop_front();
            } else {
                break;
            }
        }
        while let Some(&(_, at)) = self.min_deque.front() {
            if at < cutoff {
                self.min_deque.pop_front();
            } else {
                break;
            }
        }
    }

    /// Highest price in `[now − w, now]` (`None` when empty).
    fn win_high(&self, now: Ts) -> Option<f64> {
        self.max_deque
            .iter()
            .filter(|&&(_, at)| self.holds(at, now))
            .map(|&(p, _)| p)
            .max_by(|a, b| a.partial_cmp(b).unwrap())
    }

    /// Lowest price in `[now − w, now]` (`None` when empty).
    fn win_low(&self, now: Ts) -> Option<f64> {
        self.min_deque
            .iter()
            .filter(|&&(_, at)| self.holds(at, now))
            .map(|&(p, _)| p)
            .min_by(|a, b| a.partial_cmp(b).unwrap())
    }

    /// Most recent price in `[now − w, now]` (`None` when empty).
    fn current_price(&self, now: Ts) -> Option<f64> {
        self.max_deque
            .iter()
            .filter(|&&(_, at)| self.holds(at, now))
            .max_by_key(|&&(_, at)| at)
            .map(|&(p, _)| p)
    }

    /// `trail` — percent below the rolling window high at `now`; `NaN` on an empty
    /// window or a non-positive high (no meaningful drawdown reference).
    pub fn trail(&self, now: Ts) -> f64 {
        match (self.win_high(now), self.current_price(now)) {
            (Some(high), Some(cur)) if high > 0.0 => (high - cur) / high * 100.0,
            _ => f64::NAN,
        }
    }

    /// `rise` — percent above the rolling window low at `now`; `NaN` on an empty
    /// window or a non-positive low.
    pub fn rise(&self, now: Ts) -> f64 {
        match (self.win_low(now), self.current_price(now)) {
            (Some(low), Some(cur)) if low > 0.0 => (cur - low) / low * 100.0,
            _ => f64::NAN,
        }
    }

    /// Value of one `m_price_window` metric over `[now − w, now]`. Non price-window
    /// ids yield `NaN` (unreachable — `TokenTrack` routes by group).
    pub fn value(&self, id: MetricId, now: Ts) -> f64 {
        match id {
            MetricId::WinTrail => self.trail(now),
            MetricId::WinRise => self.rise(now),
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
    fn nan_before_first_trade() {
        let s = PriceWindowState::new(30.0);
        let now = ts(0.0);
        assert!(s.trail(now).is_nan());
        assert!(s.rise(now).is_nan());
    }

    #[test]
    fn trail_is_percent_below_rolling_high() {
        let mut s = PriceWindowState::new(30.0);
        s.on_trade(10.0, ts(0.0));
        assert_eq!(s.trail(ts(0.0)), 0.0); // at the high
        s.on_trade(12.0, ts(1.0)); // new high, still at it
        assert_eq!(s.trail(ts(1.0)), 0.0);
        s.on_trade(9.0, ts(2.0)); // 25% below the 12 high
        assert!((s.trail(ts(2.0)) - 25.0).abs() < 1e-9);
    }

    #[test]
    fn rise_is_percent_above_rolling_low() {
        let mut s = PriceWindowState::new(30.0);
        s.on_trade(10.0, ts(0.0));
        assert_eq!(s.rise(ts(0.0)), 0.0); // at the low
        s.on_trade(8.0, ts(1.0)); // new low, still at it
        assert_eq!(s.rise(ts(1.0)), 0.0);
        s.on_trade(10.0, ts(2.0)); // 25% above the 8 low
        assert!((s.rise(ts(2.0)) - 25.0).abs() < 1e-9);
    }

    #[test]
    fn rolling_high_expires_off_the_front() {
        let mut s = PriceWindowState::new(10.0);
        s.on_trade(20.0, ts(0.0)); // high at t=0
        s.on_trade(10.0, ts(5.0)); // 50% below the 20 high
        assert!((s.trail(ts(5.0)) - 50.0).abs() < 1e-9);
        // A trade at t=11 pushes the t=0 high out of the 10 s window → new high is 10,
        // current 10 → trail 0 (the old peak no longer counts).
        s.on_trade(10.0, ts(11.0));
        assert_eq!(s.trail(ts(11.0)), 0.0);
    }

    #[test]
    fn tick_evicts_to_empty_after_quiet_gap() {
        let mut s = PriceWindowState::new(10.0);
        s.on_trade(10.0, ts(0.0));
        assert_eq!(s.trail(ts(0.0)), 0.0);
        // Boundary: an entry exactly 10 s old is still in (cutoff is exclusive).
        s.evict(ts(10.0));
        assert_eq!(s.trail(ts(10.0)), 0.0);
        // One ms past the window edge → window empties → NaN (flow-dead token).
        s.evict(ts(10.001));
        assert!(s.trail(ts(10.001)).is_nan());
        assert!(s.rise(ts(10.001)).is_nan());
    }

    #[test]
    fn non_finite_price_ignored() {
        let mut s = PriceWindowState::new(30.0);
        s.on_trade(10.0, ts(0.0));
        s.on_trade(f64::NAN, ts(1.0));
        s.on_trade(f64::INFINITY, ts(2.0));
        assert_eq!(s.trail(ts(2.0)), 0.0);
        assert_eq!(s.rise(ts(2.0)), 0.0);
    }

    #[test]
    fn regressed_block_time_excludes_future_dated_entries() {
        let mut s = PriceWindowState::new(30.0);
        s.on_trade(10.0, ts(10.0));
        s.on_trade(8.0, ts(12.0));
        // At now=4 every stored print is future-dated — window is empty.
        s.evict(ts(4.0));
        assert!(s.trail(ts(4.0)).is_nan());
        // At now=12 both prints lie in the window.
        assert!((s.trail(ts(12.0)) - 20.0).abs() < 1e-9); // 8 is 20% below the 10 high
        assert_eq!(s.rise(ts(12.0)), 0.0); // 8 is the low, current is 8
    }

    #[test]
    fn slot_order_regressed_block_time_does_not_phantom_trail_spike() {
        let mut s = PriceWindowState::new(5.0);
        // Low slot seen first in slot order but carrying a later block_time.
        s.on_trade(0.000082, ts(54.0));
        // High stream with an earlier block_time seen second.
        s.on_trade(0.000121, ts(51.0));
        // At now=51 the t=54 print must not inflate trail against the t=51 print.
        assert!(s.trail(ts(51.0)) < 30.0);
    }

    /// The monotonic deques must agree with a naive O(n) scan over every prefix of
    /// a random-ish price series, both for the max (`trail`) and min (`rise`).
    #[test]
    fn deques_match_naive_scan_over_a_series() {
        let prices = [
            5.0, 7.0, 3.0, 9.0, 4.0, 4.0, 8.0, 2.0, 6.0, 10.0, 1.0, 7.5, 7.5, 3.3,
        ];
        // Wide window so nothing evicts — pure extrema correctness.
        let mut s = PriceWindowState::new(10_000.0);
        for (i, &p) in prices.iter().enumerate() {
            let now = ts(i as f64);
            s.on_trade(p, now);
            let hi = prices[..=i].iter().cloned().fold(f64::MIN, f64::max);
            let lo = prices[..=i].iter().cloned().fold(f64::MAX, f64::min);
            let expect_trail = (hi - p) / hi * 100.0;
            let expect_rise = (p - lo) / lo * 100.0;
            assert!((s.trail(now) - expect_trail).abs() < 1e-9, "trail@{i}");
            assert!((s.rise(now) - expect_rise).abs() < 1e-9, "rise@{i}");
        }
    }

    /// With a finite window the extrema must match a naive scan over only the
    /// in-window suffix — exercises front eviction against the naive baseline.
    #[test]
    fn windowed_extrema_match_naive_in_window_scan() {
        let prices = [5.0, 9.0, 3.0, 8.0, 2.0, 7.0, 4.0, 6.0, 1.0, 10.0];
        let window = 3.0; // seconds; one trade per second below
        let mut s = PriceWindowState::new(window);
        for (i, &p) in prices.iter().enumerate() {
            let now_secs = i as f64;
            let at = ts(now_secs);
            s.on_trade(p, at);
            // Naive baseline mirrors the eviction rule (`at < now - w` drops), so an
            // entry exactly `w` old stays — same closed lower bound as `flow_window`.
            let lo_bound = now_secs - window;
            let in_window: Vec<f64> = prices[..=i]
                .iter()
                .enumerate()
                .filter(|(j, _)| (*j as f64) >= lo_bound)
                .map(|(_, &v)| v)
                .collect();
            let hi = in_window.iter().cloned().fold(f64::MIN, f64::max);
            let lo = in_window.iter().cloned().fold(f64::MAX, f64::min);
            assert!((s.trail(at) - (hi - p) / hi * 100.0).abs() < 1e-9, "trail@{i}");
            assert!((s.rise(at) - (p - lo) / lo * 100.0).abs() < 1e-9, "rise@{i}");
        }
    }
}
