//! Per-`(token, entry)` prefix-extrema index for O(log n) exit resolution
//! (exit-index plan). Built once when the engine's entry cache goes stale; hull
//! Vecs are reused in place so peak RSS does not grow.
//!
//! Hull definitions (over `fill_row+1..n`):
//! - `hull_max[i]` = running max of **finite** prices (carried through NaN rows)
//! - `hull_min[i]` = running min of finite prices (same carry)
//!
//! Both are monotone, so for a predicate that is upward-closed in price (`pnl >= tp`)
//! the first satisfying row is `partition_point` over `hull_max` — and symmetrically
//! over `hull_min` for a downward-closed one. The predicate is supplied by the caller
//! and is the same `eval` the scalar walk applies, so the two cannot disagree about
//! inclusivity or non-finite handling; this module only supplies the *search*.
//!
//! The rebuild also records whether the row timestamps are non-decreasing over the
//! scan range — the precondition for answering an `m_position.held` bound by binary
//! search. Block time can regress a few seconds across slots, and a regression would
//! make `held` non-monotone and land such a search anywhere.

use hunter_engine::metrics::series::MetricSeries;

/// Recycled scratch + metadata for one entry's exit index.
#[derive(Debug, Clone, Default)]
pub struct ExitIndex {
    hull_max: Vec<f64>,
    hull_min: Vec<f64>,
    /// Absolute series row of the first dead flag after the fill (`None` if none).
    dead_row: Option<usize>,
    /// Last finite-price row in the whole series (Open mark-to-market; matches
    /// scalar `resolve_exit`'s `(0..n).rev().find(finite)`).
    last_finite_row: Option<usize>,
    /// First series row the hull covers (`fill_row + 1`).
    start: usize,
    /// Whether `series.at` is non-decreasing over `start..n` — the soundness
    /// precondition for a binary search on `held`.
    at_nondecreasing: bool,
    /// Whether [`Self::rebuild`] produced a usable index for the current entry.
    ready: bool,
}

impl ExitIndex {
    /// Clear without freeing capacity — next [`Self::rebuild`] reuses the Vecs.
    pub fn clear(&mut self) {
        self.hull_max.clear();
        self.hull_min.clear();
        self.dead_row = None;
        self.last_finite_row = None;
        self.start = 0;
        self.at_nondecreasing = false;
        self.ready = false;
    }

    /// Rebuild hulls for `fill_row+1..n` into recycled buffers.
    pub fn rebuild(&mut self, series: &MetricSeries, fill_row: usize) {
        self.hull_max.clear();
        self.hull_min.clear();
        self.dead_row = None;
        self.last_finite_row = None;
        self.at_nondecreasing = true;

        let n = series.n_rows();
        // Open path parity: last finite anywhere in the series.
        for k in (0..n).rev() {
            if series.price[k].is_finite() {
                self.last_finite_row = Some(k);
                break;
            }
        }

        let start = fill_row.saturating_add(1);
        self.start = start;
        if start >= n {
            self.ready = true;
            return;
        }

        let len = n - start;
        self.hull_max.reserve(len);
        self.hull_min.reserve(len);

        let mut running_max = f64::NEG_INFINITY;
        let mut running_min = f64::INFINITY;
        for j in start..n {
            let p = series.price[j];
            if p.is_finite() {
                if p > running_max {
                    running_max = p;
                }
                if p < running_min {
                    running_min = p;
                }
            }
            self.hull_max.push(running_max);
            self.hull_min.push(running_min);
            if self.dead_row.is_none() && series.dead[j] {
                self.dead_row = Some(j);
            }
            if j > start && series.at[j] < series.at[j - 1] {
                self.at_nondecreasing = false;
            }
        }
        self.ready = true;
    }

    #[inline]
    pub fn is_ready(&self) -> bool {
        self.ready
    }

    #[inline]
    pub fn dead_row(&self) -> Option<usize> {
        self.dead_row
    }

    #[inline]
    pub fn last_finite_row(&self) -> Option<usize> {
        self.last_finite_row
    }

    /// Whether row timestamps are non-decreasing over the indexed range — the
    /// precondition for answering a `held` bound by binary search.
    #[inline]
    pub fn at_nondecreasing(&self) -> bool {
        self.at_nondecreasing
    }

    /// Highest / lowest finite price the hull carries (`None` for an empty range).
    /// Callers use these to check that a price→metric transform stays finite (hence
    /// monotone) over the whole range before trusting a prefix query.
    #[inline]
    pub fn hull_max_last(&self) -> Option<f64> {
        self.hull_max.last().copied()
    }

    #[inline]
    pub fn hull_min_last(&self) -> Option<f64> {
        self.hull_min.last().copied()
    }

    /// First series row at which `pred` holds for some price up to that row, given
    /// `pred` is **upward-closed in price** (once true at a price it is true at every
    /// higher one). `partition_point(!pred(hull_max))` ⇒ the first index where the
    /// running max satisfies it — which is necessarily a row whose own price is that
    /// max, so it is exactly the row a scalar walk would stop at.
    pub fn first_max_row(&self, pred: impl Fn(f64) -> bool) -> Option<usize> {
        if !self.ready || self.hull_max.is_empty() {
            return None;
        }
        let i = self.hull_max.partition_point(|&m| !pred(m));
        (i < self.hull_max.len()).then_some(self.start + i)
    }

    /// [`Self::first_max_row`]'s mirror for a **downward-closed** predicate, over the
    /// running min.
    pub fn first_min_row(&self, pred: impl Fn(f64) -> bool) -> Option<usize> {
        if !self.ready || self.hull_min.is_empty() {
            return None;
        }
        let i = self.hull_min.partition_point(|&m| !pred(m));
        (i < self.hull_min.len()).then_some(self.start + i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone, Utc};
    use hunter_engine::metrics::series::MetricSeries;
    use hunter_engine::metrics::{Side, TradeLite};

    /// Build a series then overwrite the public price/dead columns so hull tests
    /// can drive exact NaN / threshold cases without fighting deadness math.
    fn series_from(prices: &[f64], dead: &[bool]) -> MetricSeries {
        assert_eq!(prices.len(), dead.len());
        let t0 = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let mut s = MetricSeries::new(t0, vec![]);
        for (i, &p) in prices.iter().enumerate() {
            let price = if p.is_finite() { p } else { 1.0 };
            s.push_trade(TradeLite {
                side: Side::Buy,
                sol: 1.0,
                price,
                reserve_sol: 100.0,
                at: t0 + Duration::seconds(i as i64),
                ..Default::default()
            });
        }
        s.price = prices.to_vec();
        s.dead = dead.to_vec();
        s
    }

    #[test]
    fn hulls_monotone_and_carry_nan() {
        // fill_row = 0 ⇒ hull covers rows 1..n: [NaN, 3.0, 2.0]
        let s = series_from(&[1.0, f64::NAN, 3.0, 2.0], &[false; 4]);
        let mut idx = ExitIndex::default();
        idx.rebuild(&s, 0);
        assert_eq!(idx.hull_max, vec![f64::NEG_INFINITY, 3.0, 3.0]);
        assert_eq!(idx.hull_min, vec![f64::INFINITY, 3.0, 2.0]);
        assert!(idx.hull_max.windows(2).all(|w| w[0] <= w[1]));
        assert!(idx.hull_min.windows(2).all(|w| w[0] >= w[1]));
    }

    #[test]
    fn empty_tail_ready() {
        let s = series_from(&[1.0], &[false]);
        let mut idx = ExitIndex::default();
        idx.rebuild(&s, 0); // fill at last row → empty scan range
        assert!(idx.is_ready());
        assert!(idx.hull_max.is_empty());
        assert_eq!(idx.first_max_row(|m| m >= 2.0), None);
        assert_eq!(idx.dead_row(), None);
        assert_eq!(idx.hull_max_last(), None);
    }

    #[test]
    fn all_nan_series() {
        let s = series_from(&[f64::NAN, f64::NAN], &[false, true]);
        let mut idx = ExitIndex::default();
        idx.rebuild(&s, 0);
        assert_eq!(idx.dead_row(), Some(1));
        assert_eq!(idx.last_finite_row(), None);
        // The hull carries ±inf, which no finite-guarded predicate accepts.
        assert_eq!(idx.first_max_row(|m| m.is_finite() && m >= 1.0), None);
        assert_eq!(idx.first_min_row(|m| m.is_finite() && m <= 1.0), None);
    }

    #[test]
    fn threshold_inclusivity_matches_scalar() {
        // price hits exactly the threshold at row 2.
        let s = series_from(&[1.0, 1.0, 1.5, 2.0], &[false; 4]);
        let mut idx = ExitIndex::default();
        idx.rebuild(&s, 0);
        // `>= 1.5` → inclusive → row 2
        assert_eq!(idx.first_max_row(|m| m >= 1.5), Some(2));
        // `> 1.5` → exclusive → row 3 (2.0)
        assert_eq!(idx.first_max_row(|m| m > 1.5), Some(3));
        // `<= 1.0` → row 1 (first after the fill at 0)
        assert_eq!(idx.first_min_row(|m| m <= 1.0), Some(1));
    }

    #[test]
    fn hull_queries_agree_with_a_scalar_walk() {
        // The property the fast path rests on, checked directly against a per-row
        // walk for every threshold in a grid.
        let prices = [1.0, 1.4, f64::NAN, 0.9, 2.2, 0.4, 1.1, 3.0, 0.2];
        let s = series_from(&prices, &[false; 9]);
        let mut idx = ExitIndex::default();
        for fill_row in 0..prices.len() {
            idx.rebuild(&s, fill_row);
            for t in [0.2, 0.4, 0.9, 1.0, 1.1, 1.4, 2.2, 3.0, 5.0] {
                let up = |p: f64| p.is_finite() && p >= t;
                let down = |p: f64| p.is_finite() && p <= t;
                let want_up = (fill_row + 1..prices.len()).find(|&j| up(prices[j]));
                let want_down = (fill_row + 1..prices.len()).find(|&j| down(prices[j]));
                assert_eq!(idx.first_max_row(up), want_up, "fill={fill_row} t={t} up");
                assert_eq!(idx.first_min_row(down), want_down, "fill={fill_row} t={t} down");
            }
        }
    }

    #[test]
    fn at_monotonicity_is_recorded() {
        let mut s = series_from(&[1.0, 1.0, 1.0, 1.0], &[false; 4]);
        let mut idx = ExitIndex::default();
        idx.rebuild(&s, 0);
        assert!(idx.at_nondecreasing(), "1 s apart, strictly increasing");
        // Regress one row's block time inside the scan range.
        s.at[2] = s.at[1] - Duration::seconds(5);
        idx.rebuild(&s, 0);
        assert!(!idx.at_nondecreasing(), "a regressed block time must be caught");
        // A regression BEFORE the fill row is outside the range and must not matter.
        idx.rebuild(&s, 2);
        assert!(idx.at_nondecreasing());
    }

    #[test]
    fn rebuild_reuses_capacity() {
        let s = series_from(&[1.0, 2.0, 3.0, 4.0], &[false; 4]);
        let mut idx = ExitIndex::default();
        idx.rebuild(&s, 0);
        let cap = idx.hull_max.capacity();
        idx.rebuild(&s, 1);
        assert!(idx.hull_max.capacity() >= cap.min(3));
    }
}
