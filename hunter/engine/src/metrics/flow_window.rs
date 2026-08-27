//! `m_flow_window` — trailing-window flow aggregates (dynamic metrics).
//!
//! Strict param `window_size_sec` (`w`): the trailing window is `[now − w, now]`.
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
//!   and subtracts only the two ends that fall outside `[now − w, now]`: entries not
//!   yet evicted at the front, and future-dated entries at the back. Both loops are
//!   normally zero iterations, and neither can miss an entry because the deque is
//!   sorted.
//!
//! The window width is precomputed once per aggregator rather than per element.

use std::collections::HashMap;
use std::collections::VecDeque;

use super::{MetricId, Side, WindowSpec};

/// True when `pos` lies inside `spec`'s window at `now_pos` - the same closed
/// bounds [`WindowState::evict`] and [`WindowState::value`] use, exposed for the
/// differential tests that check the O(1) reads against a brute-force scan.
pub fn in_window(spec: WindowSpec, pos: i64, now_pos: i64) -> bool {
    let (lo, hi) = spec.bounds(now_pos);
    pos >= lo && pos <= hi
}

/// Insert `(pos, payload)` into a **position-sorted** deque, oldest at the front.
///
/// The common case is a monotone cursor, which is a bare `push_back`. A regressed
/// `block_time` (canonical order is slot -> tx_index -> leg, so time may step back
/// within a mint) walks in from the tail - bounded by how far the regression
/// reaches, not by the buffer length. Sortedness is what lets a read correct the
/// running sums by inspecting only the two ends.
pub(super) fn push_sorted<T>(buf: &mut VecDeque<(i64, T)>, pos: i64, payload: T) {
    match buf.back() {
        Some(&(last, _)) if last > pos => {
            let idx = buf.iter().rposition(|&(p, _)| p <= pos).map_or(0, |i| i + 1);
            buf.insert(idx, (pos, payload));
        }
        _ => buf.push_back((pos, payload)),
    }
}

/// One trailing-window aggregator for a single [`WindowSpec`].
///
/// **Unit-agnostic by construction.** Every entry carries a `pos` already expressed
/// in this window's own unit - milliseconds for `WindowUnit::Sec`, the slot number
/// for `WindowUnit::Slot` - so the fold, the eviction and the read are ONE
/// implementation over an `i64` cursor rather than two parallel ones. That is what
/// keeps a slot window from duplicating a single metric.
#[derive(Debug, Clone)]
pub struct WindowState {
    spec: WindowSpec,
    /// `(pos, signed SOL)` - buy positive, sell negative; oldest at front, kept
    /// **position-sorted** (see [`push_sorted`]).
    buf: VecDeque<(i64, f64)>,
    /// Running sums over **all** of `buf` (kept in sync on push/evict), so a read is
    /// O(1) plus the out-of-window ends.
    buy: f64,
    sell: f64,
    /// Running count of buy entries in `buf`, corrected at the ends the same way.
    buy_n: u32,
    /// Wallet hash per entry of `buf`, same order - the distinct count's payload.
    ///
    /// Parallel to `buf` rather than a third tuple field so the SOL-only reads keep
    /// their exact cache-linear layout: `unique_wallets` is the rare metric and must
    /// not make `gross_flow` pay for it.
    wallet_buf: VecDeque<(i64, u64)>,
    /// Occurrence count per wallet over **all** of `wallet_buf`, so the distinct count
    /// is `len()` - maintained on push/evict, never recomputed by scanning.
    wallets: HashMap<u64, u32>,
}

impl WindowState {
    pub fn new(spec: WindowSpec) -> Self {
        Self {
            spec,
            buf: VecDeque::new(),
            buy: 0.0,
            sell: 0.0,
            buy_n: 0,
            wallet_buf: VecDeque::new(),
            wallets: HashMap::new(),
        }
    }

    /// The span this aggregator tracks.
    pub fn spec(&self) -> WindowSpec {
        self.spec
    }

    /// Fold one trade at `pos`, then drop anything that fell out of the window as of
    /// `now_pos`. Non-finite or negative SOL is ignored (guards a poisoned feed value).
    pub fn on_trade(&mut self, side: Side, sol: f64, pos: i64, now_pos: i64, wallet: u64) {
        if !sol.is_finite() || sol < 0.0 {
            return;
        }
        match side {
            Side::Buy => {
                push_sorted(&mut self.buf, pos, sol);
                self.buy += sol;
                self.buy_n += 1;
            }
            Side::Sell => {
                push_sorted(&mut self.buf, pos, -sol);
                self.sell += sol;
            }
        }
        push_sorted(&mut self.wallet_buf, pos, wallet);
        *self.wallets.entry(wallet).or_insert(0) += 1;
        self.evict(now_pos);
    }

    /// Drop entries that fell off the low end of the window as of `now_pos`. Called on
    /// every trade and every tick so reads always see the window as of the last event.
    ///
    /// Only the LOW end evicts. A lagged window excludes a head as well, but that head
    /// is still inside the buffer of every shorter window sharing the same cursor and
    /// will enter this one as the cursor advances, so the read corrects it instead.
    pub fn evict(&mut self, now_pos: i64) {
        let (lo, _) = self.spec.bounds(now_pos);
        while let Some(&(pos, signed)) = self.buf.front() {
            if pos >= lo {
                break;
            }
            self.buf.pop_front();
            if signed >= 0.0 {
                self.buy -= signed;
                self.buy_n = self.buy_n.saturating_sub(1);
            } else {
                self.sell += signed; // signed < 0 => subtracts from the sell sum
            }
        }
        while let Some(&(pos, wallet)) = self.wallet_buf.front() {
            if pos >= lo {
                break;
            }
            self.wallet_buf.pop_front();
            // The map holds occurrences, so a wallet leaves the distinct count only on
            // its LAST entry falling out - remove at zero, or `len()` counts ghosts.
            if let Some(n) = self.wallets.get_mut(&wallet) {
                *n -= 1;
                if *n == 0 {
                    self.wallets.remove(&wallet);
                }
            }
        }
    }

    /// Value of one `m_flow_window` metric over the window at `now_pos`.
    ///
    /// Starts from the running sums and subtracts only what lies outside `[lo, hi]`:
    /// entries the last [`evict`](Self::evict) has not dropped yet (front), and
    /// entries past `hi` at the back - which is where a **lagged** window's excluded
    /// head lives, as well as anything a regressed `block_time` pushed in. Both loops
    /// terminate on the first in-window entry, which the sorted deque guarantees is
    /// also the last out-of-window one.
    pub fn value(&self, id: MetricId, now_pos: i64) -> f64 {
        let (lo, hi) = self.spec.bounds(now_pos);
        let (mut buy, mut sell, mut buy_n) = (self.buy, self.sell, self.buy_n);
        for &(pos, signed) in self.buf.iter() {
            if pos >= lo {
                break;
            }
            drop_entry(signed, &mut buy, &mut sell, &mut buy_n);
        }
        for &(pos, signed) in self.buf.iter().rev() {
            if pos <= hi {
                break;
            }
            drop_entry(signed, &mut buy, &mut sell, &mut buy_n);
        }
        match id {
            MetricId::Buy => buy,
            MetricId::Sell => sell,
            MetricId::GrossFlow => buy + sell,
            MetricId::NetFlow => buy - sell,
            MetricId::BuyCount => buy_n as f64,
            // Direction of the window's flow, independent of its size. Undefined on an
            // empty window: with no SOL either way there is no share to report, and a
            // `0.0` would read as "all sells" to a `buy_share <= X` condition.
            MetricId::BuyShare => {
                let gross = buy + sell;
                if gross > 0.0 {
                    buy / gross * 100.0
                } else {
                    f64::NAN
                }
            }
            MetricId::UniqueWallets => self.unique_wallets(now_pos),
            MetricId::TradeCount => self.trade_count(now_pos),
            // How hard each wallet in the window is working the tape. `<= 2` is a
            // crowd; a large value is one wallet re-entering, which `trade_count` and
            // `gross_flow` cannot tell apart. A COUNT ratio, never an identity, so
            // wallet rotation does not defeat it.
            //
            // `NaN` on an empty window rather than `0.0`: no wallets means no churn to
            // report, and a `0.0` would let `trades_per_wallet <= 2` pass on a dead
            // tape - the exact reading the gate exists to exclude.
            MetricId::TradesPerWallet => {
                let wallets = self.unique_wallets(now_pos);
                if wallets > 0.0 {
                    self.trade_count(now_pos) / wallets
                } else {
                    f64::NAN
                }
            }
            _ => f64::NAN,
        }
    }

    /// Trades in the window at `now_pos`. `buf` holds one entry per trade, so this is
    /// the same two-ended correction the SOL sums use, on a count instead. Shared by
    /// `trade_count`, `trades_per_wallet`, and - across the group boundary -
    /// [`flow_burst::trade_share`], so no two of them can disagree about what a
    /// trade in a window is.
    ///
    /// [`flow_burst::trade_share`]: super::flow_burst::trade_share
    pub(super) fn trade_count(&self, now_pos: i64) -> f64 {
        let (lo, hi) = self.spec.bounds(now_pos);
        let n = self.buf.len()
            - self.buf.iter().take_while(|&&(p, _)| p < lo).count()
            - self.buf.iter().rev().take_while(|&&(p, _)| p > hi).count();
        n as f64
    }

    /// Distinct wallets in the window at `now_pos`.
    ///
    /// Same contract as the SOL reads: start from state maintained on push/evict and
    /// correct only the two ends. A distinct count cannot subtract the way a sum can -
    /// a wallet leaves the count only when its **last** occurrence leaves the window -
    /// so the correction tallies the out-of-window occurrences per wallet and drops
    /// only the wallets whose whole tally is out. Both ends are normally empty, and
    /// then this is a `len()`.
    fn unique_wallets(&self, now_pos: i64) -> f64 {
        let (lo, hi) = self.spec.bounds(now_pos);
        let front_out = self.wallet_buf.iter().take_while(|&&(p, _)| p < lo).count();
        let back_out = self.wallet_buf.iter().rev().take_while(|&&(p, _)| p > hi).count();
        if front_out == 0 && back_out == 0 {
            return self.wallets.len() as f64;
        }
        // The two ends meet when nothing is in the window at all - without this they
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

/// Remove one buffer entry's contribution from a read's running triple.
fn drop_entry(signed: f64, buy: &mut f64, sell: &mut f64, buy_n: &mut u32) {
    if signed >= 0.0 {
        *buy -= signed;
        *buy_n = buy_n.saturating_sub(1);
    } else {
        *sell += signed; // signed < 0 => subtracts from the sell sum
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::Ts;
    use chrono::{Duration, TimeZone, Utc};

    fn ts(secs: f64) -> Ts {
        Utc.timestamp_opt(1_700_000_000, 0).unwrap()
            + Duration::milliseconds((secs * 1000.0) as i64)
    }

    /// The same instant as [`ts`], on a window's own millisecond cursor.
    fn p(secs: f64) -> i64 {
        ts(secs).timestamp_millis()
    }

    /// The window is CLOSED at both ends: an entry exactly `w` old is still in it.
    /// Pinned because the off-by-one at this edge is invisible in aggregate and
    /// silently shifts every threshold fitted against an external re-implementation.
    #[test]
    fn the_window_is_closed_at_both_ends() {
        let w10 = WindowSpec::secs(10.0);
        assert!(in_window(w10, p(0.0), p(10.0)), "an entry exactly w old stays");
        assert!(in_window(w10, p(10.0), p(10.0)), "an entry exactly at now is in");
        assert!(!in_window(w10, p(-0.001), p(10.0)), "a hair older is out");
        assert!(!in_window(w10, p(10.001), p(10.0)), "the future is out");
        // And the sums agree with the predicate at that exact edge.
        let mut w = WindowState::new(WindowSpec::secs(10.0));
        w.on_trade(Side::Buy, 7.0, p(0.0), p(0.0), 1);
        assert_eq!(w.value(MetricId::Buy, p(10.0)), 7.0, "still summed at exactly w");
        assert_eq!(w.value(MetricId::Buy, p(10.001)), 0.0, "dropped a hair later");
    }

    /// Two rules asking for the same span share one buffer; a span that differs in
    /// SIZE, UNIT or LAG is a different buffer. The last two are the ones a
    /// seconds-only key could not tell apart, and they read different tape.
    #[test]
    fn the_span_identity_separates_size_unit_and_lag() {
        assert_eq!(WindowSpec::secs(10.0).key(), WindowSpec::secs(10.0).key());
        assert_ne!(WindowSpec::secs(10.0).key(), WindowSpec::secs(5.0).key());
        assert_ne!(
            WindowSpec::secs(10.0).key(),
            WindowSpec::slots(10.0, 0.0).key(),
            "10 seconds is not 10 slots"
        );
        assert_ne!(
            WindowSpec::slots(30.0, 0.0).key(),
            WindowSpec::slots(30.0, 1.0).key(),
            "a lagged window reads a different span"
        );
    }

    /// A one-slot window at no lag is the CURRENT slot alone, and a lagged window
    /// cannot see it. This pair is the whole causality guarantee: a gate on the tape
    /// before a burst must not be able to read the burst.
    #[test]
    fn a_slot_window_is_discrete_and_a_lag_excludes_the_current_slot() {
        let burst = WindowSpec::slots(1.0, 0.0);
        assert_eq!(burst.bounds(100), (100, 100));
        let quiet = WindowSpec::slots(30.0, 1.0);
        assert_eq!(quiet.bounds(100), (70, 99), "30 slots, none of them slot 100");

        let mut w = WindowState::new(burst);
        w.on_trade(Side::Buy, 2.0, 100, 100, 1);
        assert_eq!(w.value(MetricId::Buy, 100), 2.0, "in the current slot");
        assert_eq!(w.value(MetricId::BuyCount, 100), 1.0);
        assert_eq!(w.value(MetricId::Buy, 101), 0.0, "gone once the slot rolls");

        let mut q = WindowState::new(quiet);
        q.on_trade(Side::Buy, 2.0, 100, 100, 1);
        assert_eq!(q.value(MetricId::Buy, 100), 0.0, "the current slot is excluded");
        assert_eq!(q.value(MetricId::Buy, 101), 2.0, "and enters once it is behind");
    }

    /// `buy_count` counts BUYS; `trade_count` counts both sides. On a one-slot
    /// window that difference is the whole gate.
    #[test]
    fn buy_count_ignores_sells_where_trade_count_does_not() {
        let mut w = WindowState::new(WindowSpec::slots(1.0, 0.0));
        w.on_trade(Side::Buy, 1.0, 7, 7, 1);
        w.on_trade(Side::Sell, 1.0, 7, 7, 2);
        w.on_trade(Side::Buy, 1.0, 7, 7, 3);
        assert_eq!(w.value(MetricId::BuyCount, 7), 2.0);
        assert_eq!(w.value(MetricId::TradeCount, 7), 3.0);
    }

    #[test]
    fn flows_sum_over_the_window() {
        let mut w = WindowState::new(WindowSpec::secs(10.0));
        w.on_trade(Side::Buy, 3.0, p(0.0), p(0.0), 1);
        w.on_trade(Side::Sell, 1.0, p(1.0), p(1.0), 1);
        w.on_trade(Side::Buy, 2.0, p(2.0), p(2.0), 1);
        assert_eq!(w.value(MetricId::Buy, p(2.0)), 5.0);
        assert_eq!(w.value(MetricId::Sell, p(2.0)), 1.0);
        assert_eq!(w.value(MetricId::GrossFlow, p(2.0)), 6.0);
        assert_eq!(w.value(MetricId::NetFlow, p(2.0)), 4.0);
    }

    #[test]
    fn old_trades_fall_out_of_the_window() {
        let mut w = WindowState::new(WindowSpec::secs(10.0));
        w.on_trade(Side::Buy, 5.0, p(0.0), p(0.0), 1);
        // Boundary: an entry exactly 10 s old is still in (cutoff is exclusive).
        w.evict(p(10.0));
        assert_eq!(w.value(MetricId::Buy, p(10.0)), 5.0);
        // One millisecond past the window edge → it drops.
        w.evict(p(10.001));
        assert_eq!(w.value(MetricId::Buy, p(10.001)), 0.0);
        assert_eq!(w.value(MetricId::GrossFlow, p(10.001)), 0.0);
    }

    #[test]
    fn eviction_keeps_running_sums_correct() {
        let mut w = WindowState::new(WindowSpec::secs(5.0));
        w.on_trade(Side::Buy, 1.0, p(0.0), p(0.0), 1);
        w.on_trade(Side::Sell, 2.0, p(3.0), p(3.0), 1);
        // A new trade at t=6 pushes t=0 out of the 5 s window.
        w.on_trade(Side::Buy, 4.0, p(6.0), p(6.0), 1);
        assert_eq!(w.value(MetricId::Buy, p(6.0)), 4.0); // t=0 buy gone
        assert_eq!(w.value(MetricId::Sell, p(6.0)), 2.0); // t=3 sell still in
        assert_eq!(w.value(MetricId::NetFlow, p(6.0)), 2.0);
    }

    #[test]
    fn non_finite_or_negative_sol_ignored() {
        let mut w = WindowState::new(WindowSpec::secs(10.0));
        w.on_trade(Side::Buy, f64::NAN, p(0.0), p(0.0), 1);
        w.on_trade(Side::Buy, -1.0, p(0.0), p(0.0), 1);
        w.on_trade(Side::Buy, 2.0, p(0.0), p(0.0), 1);
        assert_eq!(w.value(MetricId::Buy, p(0.0)), 2.0);
    }

    #[test]
    fn future_dated_entries_excluded_at_regressed_now() {
        let mut w = WindowState::new(WindowSpec::secs(30.0));
        w.on_trade(Side::Buy, 5.0, p(54.0), p(54.0), 1);
        w.on_trade(Side::Buy, 1.0, p(51.0), p(51.0), 1);
        // At now=51 the t=54 print is future-dated — must not count.
        assert_eq!(w.value(MetricId::Buy, p(51.0)), 1.0);
        assert_eq!(w.value(MetricId::Buy, p(54.0)), 6.0);
    }

    /// The O(1) read starts from the running sums and corrects only the two ends,
    /// which is exact **only** because the deque stays time-sorted. Lock the
    /// invariant directly: a regressed `block_time` must land in place, not at the
    /// tail. (Canonical trade order is slot → tx_index → leg, so time can step back.)
    #[test]
    fn out_of_order_inserts_keep_the_buffer_sorted() {
        let mut w = WindowState::new(WindowSpec::secs(60.0));
        for secs in [10.0, 40.0, 20.0, 5.0, 30.0, 20.0] {
            w.on_trade(Side::Buy, 1.0, p(secs), p(secs), 1);
        }
        let times: Vec<_> = w.buf.iter().map(|&(t, _)| t).collect();
        let mut sorted = times.clone();
        sorted.sort();
        assert_eq!(times, sorted, "buffer must stay time-sorted");
    }

    #[test]
    fn unique_wallets_counts_people_not_trades() {
        let mut w = WindowState::new(WindowSpec::secs(10.0));
        w.on_trade(Side::Buy, 1.0, p(0.0), p(0.0), 7);
        w.on_trade(Side::Buy, 1.0, p(1.0), p(1.0), 7); // same wallet again
        w.on_trade(Side::Sell, 1.0, p(2.0), p(2.0), 8);
        assert_eq!(w.value(MetricId::UniqueWallets, p(2.0)), 2.0);
        // Churn is invisible to gross_flow's shape but not here: three trades, two
        // wallets. That separation is the whole reason for the metric.
        assert_eq!(w.value(MetricId::GrossFlow, p(2.0)), 3.0);
    }

    /// A wallet leaves the count on its LAST occurrence, not its first — the failure
    /// mode a naive `remove()` on eviction would produce.
    #[test]
    fn a_wallet_leaves_the_count_only_when_its_last_trade_does() {
        let mut w = WindowState::new(WindowSpec::secs(10.0));
        w.on_trade(Side::Buy, 1.0, p(0.0), p(0.0), 7);
        w.on_trade(Side::Buy, 1.0, p(8.0), p(8.0), 7);
        w.evict(p(10.5)); // the t=0 entry drops, the t=8 one stays
        assert_eq!(w.value(MetricId::UniqueWallets, p(10.5)), 1.0);
        w.evict(p(18.5)); // now the last one goes too
        assert_eq!(w.value(MetricId::UniqueWallets, p(18.5)), 0.0);
    }

    /// `trades_per_wallet` is the ratio the other two cannot express: one wallet
    /// re-entering and a crowd arriving look identical in `trade_count` and in
    /// `gross_flow`, and differ here.
    #[test]
    fn trades_per_wallet_separates_a_crowd_from_one_wallet_churning() {
        // Same trade count, same SOL, same window — only the wallet spread differs.
        let mut crowd = WindowState::new(WindowSpec::secs(10.0));
        let mut churn = WindowState::new(WindowSpec::secs(10.0));
        for i in 0..6u64 {
            crowd.on_trade(Side::Buy, 1.0, p(i as f64), p(i as f64), i); // six people, one trade each
            churn.on_trade(Side::Buy, 1.0, p(i as f64), p(i as f64), 1); // one person, six trades
        }
        let now = p(6.0);
        assert_eq!(crowd.value(MetricId::TradeCount, now), churn.value(MetricId::TradeCount, now));
        assert_eq!(crowd.value(MetricId::GrossFlow, now), churn.value(MetricId::GrossFlow, now));
        assert_eq!(crowd.value(MetricId::TradesPerWallet, now), 1.0);
        assert_eq!(churn.value(MetricId::TradesPerWallet, now), 6.0);
    }

    /// The trap the `NaN` exists for: a dead tape must not satisfy `<= 2`.
    #[test]
    fn trades_per_wallet_is_nan_on_an_empty_window_not_zero() {
        let mut w = WindowState::new(WindowSpec::secs(5.0));
        w.on_trade(Side::Buy, 1.0, p(0.0), p(0.0), 1);
        // Read far enough ahead that the trade has aged out of the window.
        let empty = p(60.0);
        assert_eq!(w.value(MetricId::UniqueWallets, empty), 0.0);
        assert_eq!(w.value(MetricId::TradeCount, empty), 0.0);
        assert!(
            w.value(MetricId::TradesPerWallet, empty).is_nan(),
            "0.0 here would let `trades_per_wallet <= 2` pass on a dead tape"
        );
    }

    /// `trade_count` counts TRADES where `unique_wallets` counts PEOPLE, and it must
    /// survive the same adversarial read instants as every other window read.
    #[test]
    fn trade_count_counts_trades_not_wallets() {
        let script: &[(Side, f64, f64, u64)] = &[
            (Side::Buy, 3.0, 0.0, 1),
            (Side::Sell, 1.0, 4.0, 2),
            (Side::Buy, 2.0, 9.0, 1), // wallet 1 again - a second TRADE, same person
            (Side::Buy, 5.0, 7.0, 3), // regressed
            (Side::Sell, 4.0, 12.0, 2),
            (Side::Buy, 1.5, 11.0, 4), // regressed
            (Side::Sell, 0.5, 25.0, 1),
        ];
        for window in [1.0_f64, 5.0, 10.0, 60.0] {
            let mut w = WindowState::new(WindowSpec::secs(window));
            for &(side, sol, at, wallet) in script {
                w.on_trade(side, sol, p(at), p(at), wallet);
                for probe in [-30.0, -3.0, 0.0, 0.5, 3.0, 12.0] {
                    let now = p(at + probe);
                    let brute = w
                        .buf
                        .iter()
                        .filter(|&&(t, _)| in_window(WindowSpec::secs(window), t, now))
                        .count() as f64;
                    assert_eq!(
                        w.value(MetricId::TradeCount, now),
                        brute,
                        "w={window} at={at} probe={probe}",
                    );
                }
            }
        }

        // One wallet churning is 1 wallet and 3 trades - the whole reason this metric
        // is not a rename of `unique_wallets`.
        let mut w = WindowState::new(WindowSpec::secs(60.0));
        for at in [0.0, 1.0, 2.0] {
            w.on_trade(Side::Buy, 1.0, p(at), p(at), 7);
        }
        assert_eq!(w.value(MetricId::UniqueWallets, p(2.0)), 1.0);
        assert_eq!(w.value(MetricId::TradeCount, p(2.0)), 3.0);
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
            let mut w = WindowState::new(WindowSpec::secs(window));
            for &(side, sol, at, wallet) in script {
                w.on_trade(side, sol, p(at), p(at), wallet);
                for probe in [-30.0, -3.0, 0.0, 0.5, 3.0, 12.0] {
                    let now = p(at + probe);
                    let mut seen: Vec<u64> = w
                        .wallet_buf
                        .iter()
                        .filter(|&&(t, _)| in_window(WindowSpec::secs(window), t, now))
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
            let mut w = WindowState::new(WindowSpec::secs(window));
            for &(side, sol, at) in script {
                w.on_trade(side, sol, p(at), p(at), 1);
                // Read at a spread of instants — before, at, and after the fold —
                // so both correction loops are exercised.
                for probe in [-3.0, 0.0, 0.5, 3.0, 12.0] {
                    let now = p(at + probe);
                    let (mut buy, mut sell) = (0.0, 0.0);
                    for &(t, signed) in &w.buf {
                        if !in_window(WindowSpec::secs(window), t, now) {
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
                    let share = got(MetricId::BuyShare);
                    if buy + sell > 0.0 {
                        assert!(
                            eq(share, buy / (buy + sell) * 100.0),
                            "buy_share w={window} at={at} p={probe}"
                        );
                    } else {
                        assert!(share.is_nan(), "buy_share is NaN on an empty window");
                    }
                }
            }
        }
    }

    /// `buy_share` reads the DIRECTION of the window, independent of its size, and is
    /// undefined when nothing traded.
    ///
    /// The empty case must be `NaN`, not `0.0`: a `0.0` reads as "every SOL was a sell"
    /// to a `buy_share <= X` condition, so an untraded window would satisfy a
    /// sell-pressure gate it has no evidence for.
    #[test]
    fn buy_share_is_direction_not_size_and_nan_when_empty() {
        let mut w = WindowState::new(WindowSpec::secs(60.0));
        assert!(w.value(MetricId::BuyShare, p(0.0)).is_nan(), "empty window is undefined");

        // 6 SOL of turnover, 5 of it buys.
        w.on_trade(Side::Buy, 5.0, p(1.0), p(1.0), 1);
        w.on_trade(Side::Sell, 1.0, p(2.0), p(2.0), 2);
        let small = w.value(MetricId::BuyShare, p(3.0));
        assert!((small - 500.0 / 6.0).abs() < 1e-9, "got {small}");

        // Same 5:1 direction at 100x the size reads identically - which `net_flow`
        // cannot do (+4 SOL against +400 SOL).
        let mut big = WindowState::new(WindowSpec::secs(60.0));
        big.on_trade(Side::Buy, 500.0, p(1.0), p(1.0), 1);
        big.on_trade(Side::Sell, 100.0, p(2.0), p(2.0), 2);
        assert!((big.value(MetricId::BuyShare, p(3.0)) - small).abs() < 1e-9);
        assert!(
            (big.value(MetricId::NetFlow, p(3.0)) - w.value(MetricId::NetFlow, p(3.0))).abs()
                > 1.0,
            "net_flow conflates direction with size; buy_share is the point"
        );

        // All buys, no sells - a full 100%, not a divide-by-zero.
        let mut one = WindowState::new(WindowSpec::secs(60.0));
        one.on_trade(Side::Buy, 2.0, p(1.0), p(1.0), 1);
        assert!((one.value(MetricId::BuyShare, p(2.0)) - 100.0).abs() < 1e-9);
    }
}
