//! `m_flow_window` — trailing-window flow aggregates (dynamic metrics).
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
//!
//! **The read really is O(1)** — not an O(1) claim wrapped around a full-deque
//! rescan inside [`WindowState::value`], and not a window width re-derived per
//! element inside [`in_window`]. Either shape is a per-metric, per-rule, per-tick
//! cost on the live hot path and the single-rule simulate fold alike. Two
//! invariants make the O(1) read exact:
//!
//! * `buf` is kept **time-sorted** ([`WindowState::on_trade`] inserts in order; a
//!   regressed `block_time` — legal, since canonical order is slot → tx_index → leg
//!   — walks back from the tail, which is `O(1)` whenever times are monotone);
//! * `buy`/`sell` are running sums over **all of `buf`**, so a read starts from them
//!   and subtracts only the two ends that fall outside `(now − w, now]`: entries not
//!   yet evicted at the front, and future-dated entries at the back. Both loops are
//!   normally zero iterations, and neither can miss an entry because the deque is
//!   sorted.
//!
//! The window width is precomputed once per aggregator rather than per element.

use std::collections::HashMap;
use std::collections::VecDeque;

use chrono::Duration;

use super::{MetricId, Side, Ts};

/// Dedup key for a window size. Window sizes come from rule params (finite,
/// `> 0`), so millisecond-rounding gives a stable integer identity that two
/// rules requesting the same window collapse onto.
pub fn window_key(window_secs: f64) -> u64 {
    (window_secs * 1000.0).round().max(0.0) as u64
}

/// The trailing window's width as a `chrono::Duration`, from its `window_size_sec`.
/// Rounded through [`window_key`] so the width and the dedup identity can never
/// disagree. Precomputed once per aggregator — never per buffer element.
pub fn window_width(window_secs: f64) -> Duration {
    Duration::milliseconds(window_key(window_secs) as i64)
}

/// True when `at` lies in the trailing window `[now − w, now]` — same closed
/// lower bound as [`WindowState::evict`] (`at < cutoff` drops). Entries with
/// `at > now` are excluded (regressed `block_time` across slot order).
pub fn in_window(at: Ts, now: Ts, window_secs: f64) -> bool {
    let cutoff = now - window_width(window_secs);
    at >= cutoff && at <= now
}

/// Insert `(at, payload)` into a **time-sorted** deque, oldest at the front.
///
/// The common case is monotone time, which is a bare `push_back`. A regressed
/// `block_time` (canonical order is slot → tx_index → leg, so time may step back
/// within a mint) walks in from the tail — bounded by how far the regression
/// reaches, not by the buffer length. Sortedness is what lets a read correct the
/// running sums by inspecting only the two ends.
pub(super) fn push_sorted<T>(buf: &mut VecDeque<(Ts, T)>, at: Ts, payload: T) {
    match buf.back() {
        Some(&(last, _)) if last > at => {
            let idx = buf.iter().rposition(|&(ts, _)| ts <= at).map_or(0, |i| i + 1);
            buf.insert(idx, (at, payload));
        }
        _ => buf.push_back((at, payload)),
    }
}

/// One trailing-window aggregator for a single `window_size_sec`.
#[derive(Debug, Clone)]
pub struct WindowState {
    window_secs: f64,
    /// Precomputed `window_secs` as a duration — the read/evict cutoff arithmetic
    /// happens once per call instead of once per element.
    width: Duration,
    /// `(timestamp, signed SOL)` — buy positive, sell negative; oldest at front,
    /// kept **time-sorted** (see [`push_sorted`]).
    buf: VecDeque<(Ts, f64)>,
    /// Running sums over **all** of `buf` (kept in sync on push/evict), so a read is
    /// O(1) plus the out-of-window ends.
    buy: f64,
    sell: f64,
    /// Wallet hash per entry of `buf`, same order — the distinct count's payload.
    ///
    /// Parallel to `buf` rather than a third tuple field so the SOL-only reads keep
    /// their exact cache-linear layout: `unique_wallets` is the rare metric and must
    /// not make `gross_flow` pay for it.
    wallet_buf: VecDeque<(Ts, u64)>,
    /// Occurrence count per wallet over **all** of `wallet_buf`, so the distinct count
    /// is `len()` — maintained on push/evict, never recomputed by scanning.
    wallets: HashMap<u64, u32>,
}

impl WindowState {
    pub fn new(window_secs: f64) -> Self {
        Self {
            window_secs,
            width: window_width(window_secs),
            buf: VecDeque::new(),
            buy: 0.0,
            sell: 0.0,
            wallet_buf: VecDeque::new(),
            wallets: HashMap::new(),
        }
    }

    /// The window this aggregator tracks.
    pub fn window_secs(&self) -> f64 {
        self.window_secs
    }

    /// Fold one trade, then drop anything that fell out of the window at `at`.
    /// Non-finite or negative SOL is ignored (guards a poisoned feed value).
    pub fn on_trade(&mut self, side: Side, sol: f64, at: Ts, wallet: u64) {
        if !sol.is_finite() || sol < 0.0 {
            return;
        }
        match side {
            Side::Buy => {
                push_sorted(&mut self.buf, at, sol);
                self.buy += sol;
            }
            Side::Sell => {
                push_sorted(&mut self.buf, at, -sol);
                self.sell += sol;
            }
        }
        push_sorted(&mut self.wallet_buf, at, wallet);
        *self.wallets.entry(wallet).or_insert(0) += 1;
        self.evict(at);
    }

    /// Drop entries older than the window as of `now` — trailing window is
    /// `(now − w, now]`, so an entry exactly `w` old stays. Called on every
    /// trade and every tick so reads always see the window as of the last event.
    pub fn evict(&mut self, now: Ts) {
        let cutoff = now - self.width;
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
        while let Some(&(ts, wallet)) = self.wallet_buf.front() {
            if ts >= cutoff {
                break;
            }
            self.wallet_buf.pop_front();
            // The map holds occurrences, so a wallet leaves the distinct count only on
            // its LAST entry falling out — remove at zero, or `len()` counts ghosts.
            if let Some(n) = self.wallets.get_mut(&wallet) {
                *n -= 1;
                if *n == 0 {
                    self.wallets.remove(&wallet);
                }
            }
        }
    }

    /// Value of one `m_flow_window` metric over `(now − w, now]`.
    ///
    /// Starts from the running sums and subtracts only what lies outside the window
    /// at `now`: entries the last [`evict`](Self::evict) has not dropped yet (front)
    /// and future-dated entries a regressed `block_time` pushed in (back). Both loops
    /// terminate on the first in-window entry, which the sorted deque guarantees is
    /// also the last out-of-window one — so the read is O(1) whenever the caller has
    /// evicted at `now`, which the fold does on every trade and every tick.
    pub fn value(&self, id: MetricId, now: Ts) -> f64 {
        let (mut buy, mut sell) = (self.buy, self.sell);
        let mut drop = |signed: f64| {
            if signed >= 0.0 {
                buy -= signed;
            } else {
                sell += signed; // signed < 0 ⇒ subtracts from the sell sum
            }
        };
        let cutoff = now - self.width;
        for &(ts, signed) in self.buf.iter() {
            if ts >= cutoff {
                break;
            }
            drop(signed);
        }
        for &(ts, signed) in self.buf.iter().rev() {
            if ts <= now {
                break;
            }
            drop(signed);
        }
        match id {
            MetricId::Buy => buy,
            MetricId::Sell => sell,
            MetricId::GrossFlow => buy + sell,
            MetricId::NetFlow => buy - sell,
            MetricId::UniqueWallets => self.unique_wallets(now),
            _ => f64::NAN,
        }
    }

    /// Distinct wallets in `(now − w, now]`.
    ///
    /// Same contract as the SOL reads: start from state maintained on push/evict and
    /// correct only the two ends. A distinct count cannot subtract the way a sum can —
    /// a wallet leaves the count only when its **last** occurrence leaves the window —
    /// so the correction tallies the out-of-window occurrences per wallet and drops
    /// only the wallets whose whole tally is out. Both ends are normally empty, and
    /// then this is a `len()`.
    fn unique_wallets(&self, now: Ts) -> f64 {
        let cutoff = now - self.width;
        let front_out = self.wallet_buf.iter().take_while(|&&(ts, _)| ts < cutoff).count();
        let back_out = self.wallet_buf.iter().rev().take_while(|&&(ts, _)| ts > now).count();
        if front_out == 0 && back_out == 0 {
            return self.wallets.len() as f64;
        }
        // The two ends meet when nothing is in the window at all — without this they
        // would double-count the overlap and under-report what leaves.
        if front_out + back_out >= self.wallet_buf.len() {
            return 0.0;
        }
        let mut out: HashMap<u64, u32> = HashMap::new();
        for &(_, w) in self.wallet_buf.iter().take(front_out) {
            *out.entry(w).or_insert(0) += 1;
        }
        for &(_, w) in self.wallet_buf.iter().rev().take(back_out) {
            *out.entry(w).or_insert(0) += 1;
        }
        let gone = out
            .iter()
            .filter(|(w, n)| self.wallets.get(w).is_some_and(|live| live == *n))
            .count();
        (self.wallets.len() - gone) as f64
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
        w.on_trade(Side::Buy, 3.0, ts(0.0), 1);
        w.on_trade(Side::Sell, 1.0, ts(1.0), 1);
        w.on_trade(Side::Buy, 2.0, ts(2.0), 1);
        assert_eq!(w.value(MetricId::Buy, ts(2.0)), 5.0);
        assert_eq!(w.value(MetricId::Sell, ts(2.0)), 1.0);
        assert_eq!(w.value(MetricId::GrossFlow, ts(2.0)), 6.0);
        assert_eq!(w.value(MetricId::NetFlow, ts(2.0)), 4.0);
    }

    #[test]
    fn old_trades_fall_out_of_the_window() {
        let mut w = WindowState::new(10.0);
        w.on_trade(Side::Buy, 5.0, ts(0.0), 1);
        // Boundary: an entry exactly 10 s old is still in (cutoff is exclusive).
        w.evict(ts(10.0));
        assert_eq!(w.value(MetricId::Buy, ts(10.0)), 5.0);
        // One millisecond past the window edge → it drops.
        w.evict(ts(10.001));
        assert_eq!(w.value(MetricId::Buy, ts(10.001)), 0.0);
        assert_eq!(w.value(MetricId::GrossFlow, ts(10.001)), 0.0);
    }

    #[test]
    fn eviction_keeps_running_sums_correct() {
        let mut w = WindowState::new(5.0);
        w.on_trade(Side::Buy, 1.0, ts(0.0), 1);
        w.on_trade(Side::Sell, 2.0, ts(3.0), 1);
        // A new trade at t=6 pushes t=0 out of the 5 s window.
        w.on_trade(Side::Buy, 4.0, ts(6.0), 1);
        assert_eq!(w.value(MetricId::Buy, ts(6.0)), 4.0); // t=0 buy gone
        assert_eq!(w.value(MetricId::Sell, ts(6.0)), 2.0); // t=3 sell still in
        assert_eq!(w.value(MetricId::NetFlow, ts(6.0)), 2.0);
    }

    #[test]
    fn non_finite_or_negative_sol_ignored() {
        let mut w = WindowState::new(10.0);
        w.on_trade(Side::Buy, f64::NAN, ts(0.0), 1);
        w.on_trade(Side::Buy, -1.0, ts(0.0), 1);
        w.on_trade(Side::Buy, 2.0, ts(0.0), 1);
        assert_eq!(w.value(MetricId::Buy, ts(0.0)), 2.0);
    }

    #[test]
    fn future_dated_entries_excluded_at_regressed_now() {
        let mut w = WindowState::new(30.0);
        w.on_trade(Side::Buy, 5.0, ts(54.0), 1);
        w.on_trade(Side::Buy, 1.0, ts(51.0), 1);
        // At now=51 the t=54 print is future-dated — must not count.
        assert_eq!(w.value(MetricId::Buy, ts(51.0)), 1.0);
        assert_eq!(w.value(MetricId::Buy, ts(54.0)), 6.0);
    }

    /// The O(1) read starts from the running sums and corrects only the two ends,
    /// which is exact **only** because the deque stays time-sorted. Lock the
    /// invariant directly: a regressed `block_time` must land in place, not at the
    /// tail. (Canonical trade order is slot → tx_index → leg, so time can step back.)
    #[test]
    fn out_of_order_inserts_keep_the_buffer_sorted() {
        let mut w = WindowState::new(60.0);
        for secs in [10.0, 40.0, 20.0, 5.0, 30.0, 20.0] {
            w.on_trade(Side::Buy, 1.0, ts(secs), 1);
        }
        let times: Vec<_> = w.buf.iter().map(|&(t, _)| t).collect();
        let mut sorted = times.clone();
        sorted.sort();
        assert_eq!(times, sorted, "buffer must stay time-sorted");
    }

    #[test]
    fn unique_wallets_counts_people_not_trades() {
        let mut w = WindowState::new(10.0);
        w.on_trade(Side::Buy, 1.0, ts(0.0), 7);
        w.on_trade(Side::Buy, 1.0, ts(1.0), 7); // same wallet again
        w.on_trade(Side::Sell, 1.0, ts(2.0), 8);
        assert_eq!(w.value(MetricId::UniqueWallets, ts(2.0)), 2.0);
        // Churn is invisible to gross_flow's shape but not here: three trades, two
        // wallets. That separation is the whole reason for the metric.
        assert_eq!(w.value(MetricId::GrossFlow, ts(2.0)), 3.0);
    }

    /// A wallet leaves the count on its LAST occurrence, not its first — the failure
    /// mode a naive `remove()` on eviction would produce.
    #[test]
    fn a_wallet_leaves_the_count_only_when_its_last_trade_does() {
        let mut w = WindowState::new(10.0);
        w.on_trade(Side::Buy, 1.0, ts(0.0), 7);
        w.on_trade(Side::Buy, 1.0, ts(8.0), 7);
        w.evict(ts(10.5)); // the t=0 entry drops, the t=8 one stays
        assert_eq!(w.value(MetricId::UniqueWallets, ts(10.5)), 1.0);
        w.evict(ts(18.5)); // now the last one goes too
        assert_eq!(w.value(MetricId::UniqueWallets, ts(18.5)), 0.0);
    }

    /// The distinct count must survive the same adversarial read instants the SOL
    /// reads do — un-evicted fronts, future-dated backs, and a `now` with nothing in
    /// the window at all (where the two correction ends overlap).
    #[test]
    fn unique_wallets_read_equals_a_brute_force_scan() {
        let script: &[(Side, f64, f64, u64)] = &[
            (Side::Buy, 3.0, 0.0, 1),
            (Side::Sell, 1.0, 4.0, 2),
            (Side::Buy, 2.0, 9.0, 1), // wallet 1 again
            (Side::Buy, 5.0, 7.0, 3), // regressed
            (Side::Sell, 4.0, 12.0, 2),
            (Side::Buy, 1.5, 11.0, 4), // regressed
            (Side::Sell, 0.5, 25.0, 1),
        ];
        for window in [1.0_f64, 5.0, 10.0, 60.0] {
            let mut w = WindowState::new(window);
            for &(side, sol, at, wallet) in script {
                w.on_trade(side, sol, ts(at), wallet);
                for probe in [-30.0, -3.0, 0.0, 0.5, 3.0, 12.0] {
                    let now = ts(at + probe);
                    let mut seen: Vec<u64> = w
                        .wallet_buf
                        .iter()
                        .filter(|&&(t, _)| in_window(t, now, window))
                        .map(|&(_, wallet)| wallet)
                        .collect();
                    seen.sort_unstable();
                    seen.dedup();
                    assert_eq!(
                        w.value(MetricId::UniqueWallets, now),
                        seen.len() as f64,
                        "w={window} at={at} probe={probe}",
                    );
                }
            }
        }
    }

    /// The running-sum read must agree with a brute-force `in_window` scan for
    /// **every** read instant, including out-of-order arrivals and instants the
    /// caller never evicted at (`TokenCreated` / `FirstSlotSettled` evaluate at a
    /// time no `evict` ran on). This is the guard on replacing the old full scan.
    #[test]
    fn running_sum_read_equals_a_brute_force_scan() {
        let script: &[(Side, f64, f64)] = &[
            (Side::Buy, 3.0, 0.0),
            (Side::Sell, 1.0, 4.0),
            (Side::Buy, 2.0, 9.0),
            (Side::Buy, 5.0, 7.0), // regressed
            (Side::Sell, 4.0, 12.0),
            (Side::Buy, 1.5, 11.0), // regressed
            (Side::Sell, 0.5, 25.0),
        ];
        for window in [1.0_f64, 5.0, 10.0, 60.0] {
            let mut w = WindowState::new(window);
            for &(side, sol, at) in script {
                w.on_trade(side, sol, ts(at), 1);
                // Read at a spread of instants — before, at, and after the fold —
                // so both correction loops are exercised.
                for probe in [-3.0, 0.0, 0.5, 3.0, 12.0] {
                    let now = ts(at + probe);
                    let (mut buy, mut sell) = (0.0, 0.0);
                    for &(t, signed) in &w.buf {
                        if !in_window(t, now, window) {
                            continue;
                        }
                        if signed >= 0.0 {
                            buy += signed;
                        } else {
                            sell -= signed;
                        }
                    }
                    let got = |id| w.value(id, now);
                    let eq = |a: f64, b: f64| (a - b).abs() < 1e-9;
                    assert!(eq(got(MetricId::Buy), buy), "buy w={window} at={at} p={probe}");
                    assert!(eq(got(MetricId::Sell), sell), "sell w={window} at={at} p={probe}");
                    assert!(eq(got(MetricId::GrossFlow), buy + sell), "gross w={window}");
                    assert!(eq(got(MetricId::NetFlow), buy - sell), "net w={window}");
                }
            }
        }
    }
}
