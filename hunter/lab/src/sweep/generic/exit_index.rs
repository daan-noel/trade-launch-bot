//! Per-`(token, entry)` prefix-extrema index for O(log n) pure-TP/SL exit
//! resolution (exit-index plan). Built once when the engine's entry cache goes
//! stale; hull Vecs are reused in place so peak RSS does not grow.
//!
//! Hull definitions (over `fill_row+1..n`):
//! - `hull_max[i]` = running max of **finite** prices (carried through NaN rows)
//! - `hull_min[i]` = running min of finite prices (same carry)
//! Both are monotone ⇒ `partition_point` finds the first crossing exactly.

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
        self.ready = false;
    }

    /// Rebuild hulls for `fill_row+1..n` into recycled buffers.
    pub fn rebuild(&mut self, series: &MetricSeries, fill_row: usize) {
        self.hull_max.clear();
        self.hull_min.clear();
        self.dead_row = None;
        self.last_finite_row = None;

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

    /// First series row with a finite price `≥ tp_price`, or `None`.
    /// `partition_point(hull_max < tp)` ⇒ first index where `hull_max >= tp`.
    pub fn first_tp_row(&self, tp_price: f64) -> Option<usize> {
        if !self.ready || self.hull_max.is_empty() {
            return None;
        }
        let i = self.hull_max.partition_point(|&m| m < tp_price);
        if i < self.hull_max.len() {
            Some(self.start + i)
        } else {
            None
        }
    }

    /// First series row with a finite price `≤ sl_price`, or `None`.
    /// `partition_point(hull_min > sl)` ⇒ first index where `hull_min <= sl`.
    pub fn first_sl_row(&self, sl_price: f64) -> Option<usize> {
        if !self.ready || self.hull_min.is_empty() {
            return None;
        }
        let i = self.hull_min.partition_point(|&m| m > sl_price);
        if i < self.hull_min.len() {
            Some(self.start + i)
        } else {
            None
        }
    }
}

/// Pick the earliest exit among Dead / SL / TP. On equal rows, priority is
/// **Dead > StopLoss > TakeProfit** (scalar within-row check order).
pub fn pick_exit_row(
    dead: Option<usize>,
    sl: Option<usize>,
    tp: Option<usize>,
) -> Option<(usize, ExitKind)> {
    let mut best: Option<(usize, u8, ExitKind)> = None;
    let mut consider = |row: Option<usize>, prio: u8, kind: ExitKind| {
        if let Some(r) = row {
            match best {
                None => best = Some((r, prio, kind)),
                Some((br, bp, _)) if r < br || (r == br && prio < bp) => {
                    best = Some((r, prio, kind));
                }
                _ => {}
            }
        }
    };
    consider(dead, 0, ExitKind::Dead);
    consider(sl, 1, ExitKind::StopLoss);
    consider(tp, 2, ExitKind::TakeProfit);
    best.map(|(r, _, k)| (r, k))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitKind {
    Dead,
    StopLoss,
    TakeProfit,
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
        assert_eq!(idx.first_tp_row(2.0), None);
        assert_eq!(idx.dead_row(), None);
    }

    #[test]
    fn all_nan_series() {
        let s = series_from(&[f64::NAN, f64::NAN], &[false, true]);
        let mut idx = ExitIndex::default();
        idx.rebuild(&s, 0);
        assert_eq!(idx.dead_row(), Some(1));
        assert_eq!(idx.last_finite_row(), None);
        assert_eq!(idx.first_tp_row(1.0), None);
        assert_eq!(idx.first_sl_row(1.0), None);
    }

    #[test]
    fn tp_sl_inclusivity_matches_scalar() {
        // price hits exactly the threshold at row 2.
        let s = series_from(&[1.0, 1.0, 1.5, 2.0], &[false; 4]);
        let mut idx = ExitIndex::default();
        idx.rebuild(&s, 0);
        // TP at 1.5 → ≥ inclusive → row 2
        assert_eq!(idx.first_tp_row(1.5), Some(2));
        // SL at 1.0 → ≤ inclusive → row 1 (first after fill at 0)
        assert_eq!(idx.first_sl_row(1.0), Some(1));
    }

    #[test]
    fn pick_tie_break_dead_over_sl_over_tp() {
        assert_eq!(
            pick_exit_row(Some(5), Some(5), Some(5)),
            Some((5, ExitKind::Dead))
        );
        assert_eq!(
            pick_exit_row(None, Some(5), Some(5)),
            Some((5, ExitKind::StopLoss))
        );
        assert_eq!(
            pick_exit_row(Some(10), Some(5), Some(7)),
            Some((5, ExitKind::StopLoss))
        );
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
