//! A trailing-window DISTINCT count over `u64` keys: the mechanism under
//! `m_crowd_window` (keys = wallets) and `m_build_window` (keys = build recipes).
//!
//! Not a group. Two groups count distinct values of two different columns, and each
//! keeps its own buffer because each column is its own load obligation; what they
//! share is only how a distinct count stays O(1) on a trailing window, which lives
//! here once.
//!
//! Unit-agnostic by construction, same as
//! [`WindowState`](super::flow_window::WindowState): every entry carries a `pos`
//! already expressed in the window's own unit, so one implementation serves
//! seconds, slots and prints.

use std::collections::VecDeque;

use smallvec::SmallVec;

use crate::hash::HashedMap;

use super::flow_window::push_sorted;
use super::WindowSpec;

/// How many out-of-window entries a read corrects without touching the heap.
///
/// Both ends are normally EMPTY (eviction clears the front; only a lagged window has
/// a back at all), so this is a scratch that the common case never fills. It exists
/// because the correction used to allocate a `HashMap` per read — and a lagged
/// window's back end is never empty, so `window_lag: 1` allocated on every read of
/// every tick, which is precisely the per-event allocation the hot path forbids.
/// A linear scan over a handful of entries also beats hashing them.
pub(super) const ENDS_INLINE: usize = 16;

/// One trailing-window distinct counter for a single [`WindowSpec`].
#[derive(Debug, Clone)]
pub struct DistinctWindow {
    spec: WindowSpec,
    /// `(pos, key)`, oldest at front, kept position-sorted — one entry per pushed
    /// event, so `buf.len()` is this window's event count.
    pub(super) buf: VecDeque<(i64, u64)>,
    /// Occurrence count per key over **all** of `buf`, so the distinct count is
    /// `len()` — maintained on push/evict, never recomputed by scanning.
    counts: HashedMap<u32>,
}

impl DistinctWindow {
    pub fn new(spec: WindowSpec) -> Self {
        Self { spec, buf: VecDeque::new(), counts: HashedMap::default() }
    }

    /// The span this counter tracks.
    pub fn spec(&self) -> WindowSpec {
        self.spec
    }

    /// Push one key at `pos`, then drop anything that fell out of the window as of
    /// `now_pos`. Admission is the caller's: each group decides what counts as an
    /// event in its window.
    pub fn push(&mut self, key: u64, pos: i64, now_pos: i64) {
        push_sorted(&mut self.buf, pos, key);
        *self.counts.entry(key).or_insert(0) += 1;
        self.evict(now_pos);
    }

    /// Drop entries that fell off the low end of the window as of `now_pos`.
    ///
    /// Only the LOW end evicts; a lagged window's excluded head is still inside the
    /// buffer and the read corrects for it — the same contract as the flow deque.
    pub fn evict(&mut self, now_pos: i64) {
        let (lo, _) = self.spec.bounds(now_pos);
        while let Some(&(pos, key)) = self.buf.front() {
            if pos >= lo {
                break;
            }
            self.buf.pop_front();
            // The map holds occurrences, so a key leaves the distinct count only on
            // its LAST entry falling out - remove at zero, or `len()` counts ghosts.
            if let Some(n) = self.counts.get_mut(&key) {
                *n -= 1;
                if *n == 0 {
                    self.counts.remove(&key);
                }
            }
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

    /// Events in the window at `now_pos` — `buf` holds one entry per event, so this
    /// is the same two-ended correction the SOL sums use, on a count.
    pub fn count(&self, now_pos: i64) -> f64 {
        let (front_out, back_out) = self.ends(now_pos);
        (self.buf.len().saturating_sub(front_out + back_out)) as f64
    }

    /// Distinct keys in the window at `now_pos`.
    ///
    /// Same contract as the flow reads: start from state maintained on push/evict and
    /// correct only the two ends. A distinct count cannot subtract the way a sum can —
    /// a key leaves the count only when its **last** occurrence leaves the window —
    /// so the correction tallies the out-of-window occurrences per key and drops only
    /// the keys whose whole tally is out.
    ///
    /// The tally lives in an inline [`SmallVec`], not a `HashMap`: both ends are
    /// normally empty and never more than a burst, so this allocates nothing on the
    /// path a lagged window takes on every single read.
    pub fn distinct(&self, now_pos: i64) -> f64 {
        let (front_out, back_out) = self.ends(now_pos);
        if front_out == 0 && back_out == 0 {
            return self.counts.len() as f64;
        }
        // The two ends meet when nothing is in the window at all - without this they
        // would double-count the overlap and under-report what leaves.
        if front_out + back_out >= self.buf.len() {
            return 0.0;
        }
        let mut out: SmallVec<[(u64, u32); ENDS_INLINE]> = SmallVec::new();
        let mut tally = |k: u64| match out.iter_mut().find(|(x, _)| *x == k) {
            Some((_, n)) => *n += 1,
            None => out.push((k, 1)),
        };
        for &(_, k) in self.buf.iter().take(front_out) {
            tally(k);
        }
        for &(_, k) in self.buf.iter().rev().take(back_out) {
            tally(k);
        }
        let gone = out
            .iter()
            .filter(|(k, n)| self.counts.get(k).is_some_and(|live| live == n))
            .count();
        (self.counts.len() - gone) as f64
    }
}
