//! `m_flow_window` — trailing-window flow aggregates (dynamic metrics).
//!
//! One of three window size params (`window_size_sec` / `_slots` / `_prints`) plus
//! an optional `window_lag` fixes the span; over the trades in it:
//! * `buy` / `sell` — SOL by side, and `gross_flow` / `net_flow` over the pair,
//! * `buy_count` / `sell_count` / `trade_count` — the same tape tallied,
//! * `buy_share` — the direction of the tape, independent of its size,
//! * `trade_share` / `sol_share` — the two-window reads over a nested
//!   `slice_size_*` (see [`flow_slice`](super::flow_slice)).
//!
//! **This deque carries SOL and nothing else.** Who traded is
//! [`m_crowd_window`](super::crowd_window)'s subject and its own buffer: carrying
//! wallet hashes here made every `gross_flow` rule pay a second deque push and a
//! hash-map entry per trade for a column it never reads.
//!
//! **Dynamic** = the value depends on per-rule strict params, so state is
//! **deduped by the whole span** ([`WindowKey`](super::WindowKey)): every rule
//! asking for the same window shares one buffer, and a 30-second and a 30-slot
//! window are correctly two. A ring buffer of `(pos, signed_sol)` with running
//! `buy`/`sell` sums keeps both fold and read O(1) amortized, no per-event
//! allocation.
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
//!   and subtracts only the two ends that fall outside the span: entries not
//!   yet evicted at the front, and future-dated entries at the back. Both loops are
//!   normally zero iterations, and neither can miss an entry because the deque is
//!   sorted.
//!
//! The window width is precomputed once per aggregator rather than per element.

use std::collections::VecDeque;

use super::{MetricId, Side, WindowSpec};

/// Whether a trade is admitted to a window at all — the ONE guard against a poisoned
/// feed value, shared with [`m_crowd_window`](super::crowd_window) so the two groups
/// cannot disagree about what a trade in a window is. A dropped trade is counted by
/// neither deque, which is what keeps `trades_per_wallet` a ratio over one tape.
#[inline]
pub(super) fn is_foldable(sol: f64) -> bool {
    sol.is_finite() && sol >= 0.0
}

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
}

impl WindowState {
    pub fn new(spec: WindowSpec) -> Self {
        Self {
            spec,
            buf: VecDeque::new(),
            buy: 0.0,
            sell: 0.0,
            buy_n: 0,
        }
    }

    /// The span this aggregator tracks.
    pub fn spec(&self) -> WindowSpec {
        self.spec
    }

    /// Fold one trade at `pos`, then drop anything that fell out of the window as of
    /// `now_pos`. A trade [`is_foldable`] refuses is ignored (guards a poisoned feed
    /// value), and refused identically by `m_crowd_window`'s deque.
    pub fn on_trade(&mut self, side: Side, sol: f64, pos: i64, now_pos: i64) {
        if !is_foldable(sol) {
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
            // Through `drop_entry`, never a second copy of it: `value` corrects the
            // window ends with that function, so an inline twin here is the same
            // decision written twice and free to drift from it. It did - the sign
            // test was fixed in one and not the other.
            drop_entry(signed, &mut self.buy, &mut self.sell, &mut self.buy_n);
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
            // Both ends are corrected on the SAME bounds as `trade_count`, so the two
            // counts and `buy_count` always add up — a sell is a trade that is not a buy.
            MetricId::SellCount => self.trade_count(now_pos) - buy_n as f64,
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
            MetricId::TradeCount => self.trade_count(now_pos),
            _ => f64::NAN,
        }
    }

    /// Trades in the window at `now_pos`. `buf` holds one entry per trade, so this is
    /// the same two-ended correction the SOL sums use, on a count instead. Shared by
    /// `trade_count`, `sell_count`, `m_crowd_window`'s `trades_per_wallet`, and
    /// [`flow_slice::trade_share`], so no two of them can disagree about what a trade
    /// in a window is.
    ///
    /// [`flow_slice::trade_share`]: super::flow_slice::trade_share
    pub(super) fn trade_count(&self, now_pos: i64) -> f64 {
        let (lo, hi) = self.spec.bounds(now_pos);
        let n = self.buf.len()
            - self.buf.iter().take_while(|&&(p, _)| p < lo).count()
            - self.buf.iter().rev().take_while(|&&(p, _)| p > hi).count();
        n as f64
    }

}

/// Remove one buffer entry's contribution from a read's running triple.
fn drop_entry(signed: f64, buy: &mut f64, sell: &mut f64, buy_n: &mut u32) {
    // Sign BIT, not `signed >= 0.0`: `on_trade` pushes a sell as `-sol`, so a zero-SOL
    // sell is `-0.0`, which compares `>= 0.0` and took the buy arm. The sums did not
    // notice (zero subtracts the same from either), `buy_n` did - it was decremented
    // for a trade that never incremented it, and `sell_count` is `trade_count - buy_n`,
    // so BOTH counts drifted for the rest of the window.
    if !signed.is_sign_negative() {
        *buy -= signed;
        *buy_n = buy_n.saturating_sub(1);
    } else {
        *sell += signed; // signed < 0 => subtracts from the sell sum
    }
}

#[cfg(test)]
mod tests {

    /// A zero-SOL sell is a SELL, across eviction too.
    ///
    /// `on_trade` pushes a sell as `-sol`, so a zero-SOL one is stored as `-0.0` —
    /// and `-0.0 >= 0.0` is TRUE, which put `drop_entry` on the buy arm. It
    /// decremented `buy_n` for a trade that never incremented it, so `buy_count` read
    /// one low and `sell_count` (`trade_count - buy_n`) one high for the rest of the
    /// window. The SOL sums never showed it, because zero subtracts the same from
    /// either side — only the counts carry the sign.
    #[test]
    fn a_zero_sol_sell_does_not_decrement_the_buy_count_when_it_leaves() {
        let mut w = WindowState::new(WindowSpec::secs(10.0));
        w.on_trade(Side::Buy, 1.0, p(0.0), p(0.0));
        w.on_trade(Side::Sell, 0.0, p(1.0), p(1.0));
        assert_eq!(w.value(MetricId::BuyCount, p(1.0)), 1.0);
        assert_eq!(w.value(MetricId::SellCount, p(1.0)), 1.0);

        // Push both out of the window; the eviction is where the sign is read again.
        w.on_trade(Side::Buy, 1.0, p(30.0), p(30.0));
        assert_eq!(w.value(MetricId::BuyCount, p(30.0)), 1.0);
        assert_eq!(w.value(MetricId::SellCount, p(30.0)), 0.0);
        assert_eq!(
            w.value(MetricId::BuyCount, p(30.0)) + w.value(MetricId::SellCount, p(30.0)),
            w.value(MetricId::TradeCount, p(30.0)),
        );
    }

    /// `buy_count + sell_count == trade_count`, on every window, always.
    ///
    /// The point of registering `sell_count` at all: a condition cannot subtract, so
    /// `trade_count - buy_count` has no spelling. That makes the identity a contract
    /// rather than an observation — if the two counts ever stopped adding up, a rule
    /// authored as "at most two sells" would quietly bound something else.
    #[test]
    fn buys_and_sells_add_up_to_the_trade_count() {
        let mut w = WindowState::new(WindowSpec::secs(10.0));
        let script = [
            (Side::Buy, 1.0, 0.0),
            (Side::Sell, 2.0, 1.0),
            (Side::Buy, 3.0, 2.0),
            (Side::Sell, 4.0, 3.0),
            (Side::Sell, 5.0, 4.0),
        ];
        for &(side, sol, at) in script.iter() {
            w.on_trade(side, sol, p(at), p(at));
        }
        let now = p(4.0);
        assert_eq!(w.value(MetricId::BuyCount, now), 2.0);
        assert_eq!(w.value(MetricId::SellCount, now), 3.0);
        assert_eq!(
            w.value(MetricId::BuyCount, now) + w.value(MetricId::SellCount, now),
            w.value(MetricId::TradeCount, now),
        );

        // ...and it still holds once the window has evicted part of the tape, which is
        // where a separately-maintained counter would drift from the count.
        let later = p(13.0);
        w.evict(later);
        assert_eq!(w.value(MetricId::SellCount, later), 2.0, "only the 3.0 and 4.0 sells remain");
        assert_eq!(
            w.value(MetricId::BuyCount, later) + w.value(MetricId::SellCount, later),
            w.value(MetricId::TradeCount, later),
        );
    }

    /// An empty window has no trades of either side, so both counts are `0` — NOT the
    /// `NaN` the RATIO metrics use. A count of nothing is a real zero; a share of
    /// nothing is undefined.
    #[test]
    fn an_empty_window_counts_zero_sells_rather_than_nan() {
        let w = WindowState::new(WindowSpec::secs(10.0));
        assert_eq!(w.value(MetricId::SellCount, p(0.0)), 0.0);
        assert_eq!(w.value(MetricId::BuyCount, p(0.0)), 0.0);
    }
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
        w.on_trade(Side::Buy, 7.0, p(0.0), p(0.0));
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
        w.on_trade(Side::Buy, 2.0, 100, 100);
        assert_eq!(w.value(MetricId::Buy, 100), 2.0, "in the current slot");
        assert_eq!(w.value(MetricId::BuyCount, 100), 1.0);
        assert_eq!(w.value(MetricId::Buy, 101), 0.0, "gone once the slot rolls");

        let mut q = WindowState::new(quiet);
        q.on_trade(Side::Buy, 2.0, 100, 100);
        assert_eq!(q.value(MetricId::Buy, 100), 0.0, "the current slot is excluded");
        assert_eq!(q.value(MetricId::Buy, 101), 2.0, "and enters once it is behind");
    }

    /// `buy_count` counts BUYS; `trade_count` counts both sides. On a one-slot
    /// window that difference is the whole gate.
    #[test]
    fn buy_count_ignores_sells_where_trade_count_does_not() {
        let mut w = WindowState::new(WindowSpec::slots(1.0, 0.0));
        w.on_trade(Side::Buy, 1.0, 7, 7);
        w.on_trade(Side::Sell, 1.0, 7, 7);
        w.on_trade(Side::Buy, 1.0, 7, 7);
        assert_eq!(w.value(MetricId::BuyCount, 7), 2.0);
        assert_eq!(w.value(MetricId::TradeCount, 7), 3.0);
    }

    #[test]
    fn flows_sum_over_the_window() {
        let mut w = WindowState::new(WindowSpec::secs(10.0));
        w.on_trade(Side::Buy, 3.0, p(0.0), p(0.0));
        w.on_trade(Side::Sell, 1.0, p(1.0), p(1.0));
        w.on_trade(Side::Buy, 2.0, p(2.0), p(2.0));
        assert_eq!(w.value(MetricId::Buy, p(2.0)), 5.0);
        assert_eq!(w.value(MetricId::Sell, p(2.0)), 1.0);
        assert_eq!(w.value(MetricId::GrossFlow, p(2.0)), 6.0);
        assert_eq!(w.value(MetricId::NetFlow, p(2.0)), 4.0);
    }

    #[test]
    fn old_trades_fall_out_of_the_window() {
        let mut w = WindowState::new(WindowSpec::secs(10.0));
        w.on_trade(Side::Buy, 5.0, p(0.0), p(0.0));
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
        w.on_trade(Side::Buy, 1.0, p(0.0), p(0.0));
        w.on_trade(Side::Sell, 2.0, p(3.0), p(3.0));
        // A new trade at t=6 pushes t=0 out of the 5 s window.
        w.on_trade(Side::Buy, 4.0, p(6.0), p(6.0));
        assert_eq!(w.value(MetricId::Buy, p(6.0)), 4.0); // t=0 buy gone
        assert_eq!(w.value(MetricId::Sell, p(6.0)), 2.0); // t=3 sell still in
        assert_eq!(w.value(MetricId::NetFlow, p(6.0)), 2.0);
    }

    #[test]
    fn non_finite_or_negative_sol_ignored() {
        let mut w = WindowState::new(WindowSpec::secs(10.0));
        w.on_trade(Side::Buy, f64::NAN, p(0.0), p(0.0));
        w.on_trade(Side::Buy, -1.0, p(0.0), p(0.0));
        w.on_trade(Side::Buy, 2.0, p(0.0), p(0.0));
        assert_eq!(w.value(MetricId::Buy, p(0.0)), 2.0);
    }

    #[test]
    fn future_dated_entries_excluded_at_regressed_now() {
        let mut w = WindowState::new(WindowSpec::secs(30.0));
        w.on_trade(Side::Buy, 5.0, p(54.0), p(54.0));
        w.on_trade(Side::Buy, 1.0, p(51.0), p(51.0));
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
            w.on_trade(Side::Buy, 1.0, p(secs), p(secs));
        }
        let times: Vec<_> = w.buf.iter().map(|&(t, _)| t).collect();
        let mut sorted = times.clone();
        sorted.sort();
        assert_eq!(times, sorted, "buffer must stay time-sorted");
    }

    /// `trade_count` counts TRADES — one entry per fold, whatever the side — and it
    /// must survive the same adversarial read instants as every other window read.
    /// `m_crowd_window` divides by this count, so a drift here moves that group too.
    #[test]
    fn trade_count_counts_every_fold_across_adversarial_reads() {
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
                w.on_trade(side, sol, p(at), p(at));
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
                w.on_trade(side, sol, p(at), p(at));
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
        w.on_trade(Side::Buy, 5.0, p(1.0), p(1.0));
        w.on_trade(Side::Sell, 1.0, p(2.0), p(2.0));
        let small = w.value(MetricId::BuyShare, p(3.0));
        assert!((small - 500.0 / 6.0).abs() < 1e-9, "got {small}");

        // Same 5:1 direction at 100x the size reads identically - which `net_flow`
        // cannot do (+4 SOL against +400 SOL).
        let mut big = WindowState::new(WindowSpec::secs(60.0));
        big.on_trade(Side::Buy, 500.0, p(1.0), p(1.0));
        big.on_trade(Side::Sell, 100.0, p(2.0), p(2.0));
        assert!((big.value(MetricId::BuyShare, p(3.0)) - small).abs() < 1e-9);
        assert!(
            (big.value(MetricId::NetFlow, p(3.0)) - w.value(MetricId::NetFlow, p(3.0))).abs()
                > 1.0,
            "net_flow conflates direction with size; buy_share is the point"
        );

        // All buys, no sells - a full 100%, not a divide-by-zero.
        let mut one = WindowState::new(WindowSpec::secs(60.0));
        one.on_trade(Side::Buy, 2.0, p(1.0), p(1.0));
        assert!((one.value(MetricId::BuyShare, p(2.0)) - 100.0).abs() < 1e-9);
    }
}
