//! `m_crowd_window` — trailing-window WALLET counts (dynamic metrics).
//!
//! * `unique_wallets` — distinct wallets that traded in the window;
//! * `trades_per_wallet` — `trade_count / unique_wallets` over that same window.
//!
//! **Its own buffer, not `m_flow_window`'s.** The two groups share the window size
//! params and read the same tape, but they do not share state, for the same reason
//! `m_price_window` does not: a group's buffer is an obligation, and this one's is
//! the WALLET column. Carrying the wallet hashes on the flow deque made every
//! `gross_flow` rule pay a second `VecDeque` push plus a hash-map entry on every
//! trade of every window, for a column it never reads — the cost the parallel-vec
//! layout was supposed to avoid, paid in the fold instead of the read.
//!
//! Splitting it also removes the only way the pair could have disagreed. A crowd
//! window registered mid-life starts empty, exactly like any newly registered
//! window; upgrading a flow window in place would instead have left `unique_wallets`
//! reading a partial buffer while `trade_count` on the same deque read a complete
//! one, and `trades_per_wallet` is the ratio of the two.
//!
//! `trade_count` here is this buffer's own — one entry per foldable trade, the same
//! [`is_foldable`](super::flow_window::is_foldable) admission `m_flow_window` uses —
//! so `m_crowd_window(w).trades_per_wallet` and
//! `m_flow_window(w).trade_count / m_crowd_window(w).unique_wallets` are the same
//! number by construction rather than by agreement.

use std::collections::VecDeque;

use smallvec::SmallVec;

use crate::hash::HashedMap;

use super::flow_window::{is_foldable, push_sorted};
use super::{MetricId, WindowSpec};

/// How many out-of-window entries a read corrects without touching the heap.
///
/// Both ends are normally EMPTY (eviction clears the front; only a lagged window has
/// a back at all), so this is a scratch that the common case never fills. It exists
/// because the correction used to allocate a `HashMap` per read — and a lagged
/// window's back end is never empty, so `window_lag: 1` allocated on every read of
/// every tick, which is precisely the per-event allocation the hot path forbids.
/// A linear scan over a handful of entries also beats hashing them.
const ENDS_INLINE: usize = 16;

/// One trailing-window wallet aggregator for a single [`WindowSpec`].
///
/// Unit-agnostic by construction, same as [`WindowState`](super::flow_window::WindowState):
/// every entry carries a `pos` already expressed in this window's own unit, so one
/// implementation serves seconds, slots and prints.
#[derive(Debug, Clone)]
pub struct CrowdWindowState {
    spec: WindowSpec,
    /// `(pos, wallet hash)`, oldest at front, kept position-sorted — one entry per
    /// folded trade, so `buf.len()` is this window's trade count.
    buf: VecDeque<(i64, u64)>,
    /// Occurrence count per wallet over **all** of `buf`, so the distinct count is
    /// `len()` — maintained on push/evict, never recomputed by scanning.
    wallets: HashedMap<u32>,
}

impl CrowdWindowState {
    pub fn new(spec: WindowSpec) -> Self {
        Self { spec, buf: VecDeque::new(), wallets: HashedMap::default() }
    }

    /// The span this aggregator tracks.
    pub fn spec(&self) -> WindowSpec {
        self.spec
    }

    /// Fold one trade at `pos`, then drop anything that fell out of the window as of
    /// `now_pos`. `sol` is not summed here — it is read only for the admission guard,
    /// which must be the SAME one `m_flow_window` applies or the two groups would
    /// disagree about what a trade in a window is.
    pub fn on_trade(&mut self, sol: f64, wallet: u64, pos: i64, now_pos: i64) {
        if !is_foldable(sol) {
            return;
        }
        push_sorted(&mut self.buf, pos, wallet);
        *self.wallets.entry(wallet).or_insert(0) += 1;
        self.evict(now_pos);
    }

    /// Drop entries that fell off the low end of the window as of `now_pos`.
    ///
    /// Only the LOW end evicts; a lagged window's excluded head is still inside the
    /// buffer and the read corrects for it — the same contract as the flow deque.
    pub fn evict(&mut self, now_pos: i64) {
        let (lo, _) = self.spec.bounds(now_pos);
        while let Some(&(pos, wallet)) = self.buf.front() {
            if pos >= lo {
                break;
            }
            self.buf.pop_front();
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

    /// Value of one `m_crowd_window` metric over the window at `now_pos`.
    pub fn value(&self, id: MetricId, now_pos: i64) -> f64 {
        match id {
            MetricId::UniqueWallets => self.unique_wallets(now_pos),
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

    /// How many out-of-window entries sit at each end at `now_pos`. Both loops stop
    /// on the first in-window entry, which sortedness guarantees is also the last
    /// out-of-window one.
    fn ends(&self, now_pos: i64) -> (usize, usize) {
        let (lo, hi) = self.spec.bounds(now_pos);
        (
            self.buf.iter().take_while(|&&(p, _)| p < lo).count(),
            self.buf.iter().rev().take_while(|&&(p, _)| p > hi).count(),
        )
    }

    /// Trades in the window at `now_pos` — `buf` holds one entry per trade, so this is
    /// the same two-ended correction the SOL sums use, on a count.
    fn trade_count(&self, now_pos: i64) -> f64 {
        let (front_out, back_out) = self.ends(now_pos);
        (self.buf.len().saturating_sub(front_out + back_out)) as f64
    }

    /// Distinct wallets in the window at `now_pos`.
    ///
    /// Same contract as the flow reads: start from state maintained on push/evict and
    /// correct only the two ends. A distinct count cannot subtract the way a sum can —
    /// a wallet leaves the count only when its **last** occurrence leaves the window —
    /// so the correction tallies the out-of-window occurrences per wallet and drops
    /// only the wallets whose whole tally is out.
    ///
    /// The tally lives in an inline [`SmallVec`], not a `HashMap`: both ends are
    /// normally empty and never more than a burst, so this allocates nothing on the
    /// path a lagged window takes on every single read.
    fn unique_wallets(&self, now_pos: i64) -> f64 {
        let (front_out, back_out) = self.ends(now_pos);
        if front_out == 0 && back_out == 0 {
            return self.wallets.len() as f64;
        }
        // The two ends meet when nothing is in the window at all - without this they
        // would double-count the overlap and under-report what leaves.
        if front_out + back_out >= self.buf.len() {
            return 0.0;
        }
        let mut out: SmallVec<[(u64, u32); ENDS_INLINE]> = SmallVec::new();
        let mut tally = |w: u64| match out.iter_mut().find(|(k, _)| *k == w) {
            Some((_, n)) => *n += 1,
            None => out.push((w, 1)),
        };
        for &(_, w) in self.buf.iter().take(front_out) {
            tally(w);
        }
        for &(_, w) in self.buf.iter().rev().take(back_out) {
            tally(w);
        }
        let gone = out
            .iter()
            .filter(|(w, n)| self.wallets.get(w).is_some_and(|live| live == n))
            .count();
        (self.wallets.len() - gone) as f64
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

    #[test]
    fn counts_distinct_wallets_and_churn() {
        let mut w = CrowdWindowState::new(WindowSpec::secs(10.0));
        for (wallet, at) in [(1, 0.0), (1, 1.0), (2, 2.0), (3, 3.0)] {
            w.on_trade(1.0, wallet, p(at), p(at));
        }
        assert_eq!(w.value(MetricId::UniqueWallets, p(3.0)), 3.0);
        assert_eq!(w.value(MetricId::TradesPerWallet, p(3.0)), 4.0 / 3.0);
    }

    /// A wallet leaves the distinct count only when its LAST entry falls out.
    #[test]
    fn a_wallet_leaves_only_on_its_last_occurrence() {
        let mut w = CrowdWindowState::new(WindowSpec::secs(10.0));
        w.on_trade(1.0, 7, p(0.0), p(0.0));
        w.on_trade(1.0, 7, p(9.0), p(9.0));
        w.on_trade(1.0, 8, p(9.0), p(9.0));
        // At t=11 the first entry is out, but wallet 7 still has one inside.
        w.evict(p(11.0));
        assert_eq!(w.value(MetricId::UniqueWallets, p(11.0)), 2.0);
        // At t=20 both of 7's entries and 8's are gone.
        w.evict(p(20.0));
        assert_eq!(w.value(MetricId::UniqueWallets, p(20.0)), 0.0);
        assert!(w.value(MetricId::TradesPerWallet, p(20.0)).is_nan());
    }

    /// A LAGGED window excludes a head, and the read — not eviction — is what
    /// corrects for it. This is the path that used to allocate a `HashMap` per read.
    #[test]
    fn a_lagged_window_excludes_the_head_it_still_holds() {
        let mut w = CrowdWindowState::new(WindowSpec::slots(3.0, 1.0));
        // Slots 1..=4, a distinct wallet each.
        for slot in 1..=4u64 {
            w.on_trade(1.0, slot, slot as i64, slot as i64);
        }
        // At slot 4 with lag 1 the window is slots [1, 3]: wallet 4 is excluded.
        assert_eq!(w.value(MetricId::UniqueWallets, 4), 3.0);
        assert_eq!(w.value(MetricId::TradesPerWallet, 4), 1.0);
    }

    /// The admission guard is `m_flow_window`'s, so a trade the flow deque refuses is
    /// not a trade here either — otherwise `trades_per_wallet` and `trade_count`
    /// would be ratios over two different tapes.
    #[test]
    fn a_poisoned_sol_value_is_not_a_trade_on_either_deque() {
        let mut w = CrowdWindowState::new(WindowSpec::secs(10.0));
        w.on_trade(f64::NAN, 1, p(0.0), p(0.0));
        w.on_trade(-1.0, 2, p(1.0), p(1.0));
        w.on_trade(1.0, 3, p(2.0), p(2.0));
        assert_eq!(w.value(MetricId::UniqueWallets, p(2.0)), 1.0);
        assert_eq!(w.value(MetricId::TradesPerWallet, p(2.0)), 1.0);
    }

    /// The inline scratch must not change the answer when it spills to the heap.
    #[test]
    fn the_end_correction_is_exact_past_the_inline_capacity() {
        let mut w = CrowdWindowState::new(WindowSpec::slots(2.0, 1.0));
        // Slot 1: two wallets. Slot 2: ENDS_INLINE + 4 distinct wallets, all excluded
        // by the lag, so the correction's scratch spills.
        w.on_trade(1.0, 1, 1, 1);
        w.on_trade(1.0, 2, 1, 1);
        for i in 0..(ENDS_INLINE as u64 + 4) {
            w.on_trade(1.0, 100 + i, 2, 2);
        }
        // At slot 2 with lag 1 the window is slots [1, 1]: only the first two count.
        assert_eq!(w.value(MetricId::UniqueWallets, 2), 2.0);
        assert_eq!(w.value(MetricId::TradesPerWallet, 2), 1.0);
    }

    /// The ratio the flow metrics cannot express: one wallet re-entering and a crowd
    /// arriving are the same `trade_count` and the same `gross_flow`, and differ here.
    #[test]
    fn trades_per_wallet_separates_a_crowd_from_one_wallet_churning() {
        let mut crowd = CrowdWindowState::new(WindowSpec::secs(10.0));
        let mut churn = CrowdWindowState::new(WindowSpec::secs(10.0));
        for i in 0..6u64 {
            crowd.on_trade(1.0, i, p(i as f64), p(i as f64)); // six people, one trade each
            churn.on_trade(1.0, 1, p(i as f64), p(i as f64)); // one person, six trades
        }
        let now = p(6.0);
        assert_eq!(crowd.value(MetricId::TradesPerWallet, now), 1.0);
        assert_eq!(churn.value(MetricId::TradesPerWallet, now), 6.0);
    }

    /// The trap the `NaN` exists for: a dead tape must not satisfy `<= 2`.
    #[test]
    fn trades_per_wallet_is_nan_on_an_empty_window_not_zero() {
        let mut w = CrowdWindowState::new(WindowSpec::secs(5.0));
        w.on_trade(1.0, 1, p(0.0), p(0.0));
        // Read far enough ahead that the trade has aged out of the window.
        let empty = p(60.0);
        assert_eq!(w.value(MetricId::UniqueWallets, empty), 0.0);
        assert!(
            w.value(MetricId::TradesPerWallet, empty).is_nan(),
            "0.0 here would let `trades_per_wallet <= 2` pass on a dead tape",
        );
    }

    /// The two-ended correction must agree with a brute-force scan at **every** read
    /// instant, including out-of-order arrivals and instants nothing evicted at
    /// (`TokenCreated` / `FirstSlotSettled` evaluate at a time no `evict` ran on).
    /// This is the guard on reading from maintained state instead of rescanning.
    #[test]
    fn the_distinct_count_equals_a_brute_force_scan() {
        let script: &[(f64, f64, u64)] = &[
            (3.0, 0.0, 1),
            (1.0, 4.0, 2),
            (2.0, 9.0, 1), // wallet 1 again
            (5.0, 7.0, 3), // regressed
            (4.0, 12.0, 2),
            (1.5, 11.0, 4), // regressed
            (0.5, 25.0, 1),
        ];
        for window in [1.0_f64, 5.0, 10.0, 60.0] {
            let spec = WindowSpec::secs(window);
            let mut w = CrowdWindowState::new(spec);
            for &(sol, at, wallet) in script {
                w.on_trade(sol, wallet, p(at), p(at));
                for probe in [-30.0, -3.0, 0.0, 0.5, 3.0, 12.0] {
                    let now = p(at + probe);
                    let mut seen: Vec<u64> = w
                        .buf
                        .iter()
                        .filter(|&&(t, _)| super::super::flow_window::in_window(spec, t, now))
                        .map(|&(_, wallet)| wallet)
                        .collect();
                    seen.sort_unstable();
                    seen.dedup();
                    assert_eq!(
                        w.value(MetricId::UniqueWallets, now),
                        seen.len() as f64,
                        "w={window} at={at} probe={probe}",
                    );
                    assert_eq!(
                        w.trade_count(now),
                        w.buf
                            .iter()
                            .filter(|&&(t, _)| super::super::flow_window::in_window(spec, t, now))
                            .count() as f64,
                        "trade_count w={window} at={at} probe={probe}",
                    );
                }
            }
        }
    }

    #[test]
    fn an_empty_window_has_no_crowd_to_report() {
        let w = CrowdWindowState::new(WindowSpec::secs(10.0));
        assert_eq!(w.value(MetricId::UniqueWallets, p(0.0)), 0.0);
        assert!(w.value(MetricId::TradesPerWallet, p(0.0)).is_nan());
    }
}
