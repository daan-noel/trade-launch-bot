//! Storage-retention filter for grouped-sweep combo rows — the shared core used
//! both **write-time** (trim what a fresh run persists) and at **compaction**
//! (prune the already-written 26 M-row backlog). Both call the one pure function
//! [`retained_combo_ids`] so existing and future data are selected identically.
//!
//! ## Why this exists
//!
//! `<strategy>_grouped_sweep_results` is `runs × groups × combos`-shaped; a single
//! run's combo count is 186k–331k, so the table reaches tens of GB. Migration
//! `0007` shrinks bytes-per-row (dedup `params`, narrow columns) but not the
//! multiplicative **row count**. This filter cuts the row count by keeping only
//! the analytically interesting combos per group.
//!
//! ## What it keeps
//!
//! Not pure top-K by `score`: a combo that is best on `win_rate` but mid on
//! `score` would be discarded, losing the frontier on that metric. Instead, per
//! group, it keeps the top/bottom **extremes of every ranking metric** (the 11
//! `pnl`-group columns the UI ranks on), so each metric's frontier survives.
//!
//! Two guards make that bounded — proven necessary by the data: in the largest
//! group (331,776 combos) `win_rate` has only 156 distinct values and its worst
//! value alone is shared by 110,592 combos (exactly the `score IS NULL`,
//! `n_closed < 2` junk). "Keep all combos sharing a top/bottom value" is therefore
//! unbounded, so we (a) exclude under-sampled/undefined combos from every metric's
//! value set and (b) cap how many representatives a single tied value may keep.

use std::collections::{HashMap, HashSet};

use crate::sweep::aggregate::ComboMetrics;

/// Tunable retention bounds. The [`Default`] is the locked production config; it's
/// the single place to widen/narrow what survives.
#[derive(Clone, Copy, Debug)]
pub struct RetentionCfg {
    /// Distinct top values to keep per metric (the metric's best end).
    pub top_n: usize,
    /// Distinct bottom values to keep per metric (the metric's worst end).
    pub bottom_n: usize,
    /// Max combos kept per selected tied value — the bound that stops a flat
    /// distribution (e.g. 110k combos sharing one `win_rate`) from blowing up.
    pub cap_per_value: usize,
    /// A combo only contributes to a metric's value set if it has at least this
    /// many closed trades — matches `score`'s validity rule (`n_closed >= 2`) and
    /// drops the under-sampled junk from every metric's extremes.
    pub min_closed: u64,
}

impl Default for RetentionCfg {
    fn default() -> Self {
        Self { top_n: 3, bottom_n: 3, cap_per_value: 10, min_closed: 2 }
    }
}

/// The 11 ranking metrics whose extremes we preserve — the `pnl` column group the
/// frontend sorts on. `score`/`profit_factor` are `Option`; the rest are always
/// finite once eligibility (`min_closed`, finite) is enforced. Returns `None` to
/// exclude the combo from this metric's value set (undefined or non-finite).
fn metric_value(m: &ComboMetrics, idx: usize) -> Option<f64> {
    let v = match idx {
        0 => m.score?,
        1 => m.win_rate,
        2 => m.total_pnl_sol,
        3 => m.expectancy_sol,
        4 => m.profit_factor?,
        5 => m.median_pnl_pct,
        6 => m.mean_pnl_pct,
        7 => m.p90_pnl_pct,
        8 => m.best_pnl_pct,
        9 => m.worst_pnl_pct,
        10 => m.std_pnl_pct,
        _ => unreachable!("metric index out of range"),
    };
    v.is_finite().then_some(v)
}

/// Number of ranking metrics — the loop bound for [`metric_value`].
const N_METRICS: usize = 11;

/// Compute the set of `combo_id`s to retain for one group's `metrics`.
///
/// Per group:
/// 1. **Eligibility** — a combo feeds a metric's value set only if
///    `n_closed >= cfg.min_closed` and that metric is finite/`Some`.
/// 2. For each of the 11 metrics, take the top `cfg.top_n` and bottom
///    `cfg.bottom_n` *distinct* values among eligible combos.
/// 3. For each selected value, keep at most `cfg.cap_per_value` combos holding it,
///    tie-broken by `score` desc then `combo_id` asc (deterministic).
/// 4. **Always** include `best_combo_id` so the group summary stays joinable.
///
/// Worst case `11 × (top+bottom) × cap` ids/group; the cross-metric union dedups
/// further. Pure and order-independent — the input slice order doesn't change the
/// result.
pub fn retained_combo_ids(
    metrics: &[ComboMetrics],
    best_combo_id: u32,
    cfg: &RetentionCfg,
) -> HashSet<u32> {
    let mut keep = HashSet::new();
    keep.insert(best_combo_id);

    // Eligibility (`n_closed >= min_closed`) does NOT depend on the metric, and
    // neither does the cap tie-break order (`score` desc, `combo_id` asc). Both used
    // to be recomputed inside the per-metric loop — 11 rebuilds of an `n_combos`-sized
    // vec and 11 byte-identical sorts of it, per group, inside the fold's rayon
    // workers. Hoisting them halves the sorts (22 → 11) and drops 10 of the 11
    // allocations; what remains per metric is only the genuinely metric-dependent
    // value sort. `None`/non-finite score sorts last (`NEG_INFINITY`).
    let mut eligible: Vec<&ComboMetrics> = metrics
        .iter()
        .filter(|m| m.n_closed >= cfg.min_closed)
        .collect();
    if eligible.is_empty() {
        return keep;
    }
    let score_of = |m: &ComboMetrics| m.score.filter(|s| s.is_finite()).unwrap_or(f64::NEG_INFINITY);
    eligible.sort_by(|a, b| score_of(b).total_cmp(&score_of(a)).then(a.combo_id.cmp(&b.combo_id)));

    // Buffers reused across all 11 metrics rather than reallocated per metric. `vals`
    // inherits the shared tie-break order above (it is a filtered subsequence of
    // `eligible`), which is exactly the order the cap walk needs.
    let mut vals: Vec<(u32, f64)> = Vec::with_capacity(eligible.len());
    let mut values: Vec<f64> = Vec::with_capacity(eligible.len());
    let mut selected: HashSet<u64> = HashSet::new();
    let mut taken: HashMap<u64, usize> = HashMap::new();

    for idx in 0..N_METRICS {
        vals.clear();
        vals.extend(
            eligible
                .iter()
                .filter_map(|m| metric_value(m, idx).map(|v| (m.combo_id, v))),
        );
        if vals.is_empty() {
            continue;
        }

        // Distinct values, ascending. `total_cmp` keeps NaN out (already filtered)
        // and gives a deterministic order for ties on the float bit pattern.
        values.clear();
        values.extend(vals.iter().map(|(_, v)| *v));
        values.sort_by(|a, b| a.total_cmp(b));
        values.dedup();

        // Bottom `bottom_n` and top `top_n` distinct values; a small group whose
        // distinct-count is below top+bottom just keeps every value (union dedups).
        selected.clear();
        for &v in values.iter().take(cfg.bottom_n) {
            selected.insert(v.to_bits());
        }
        for &v in values.iter().rev().take(cfg.top_n) {
            selected.insert(v.to_bits());
        }

        // Keep up to `cap_per_value` combos per selected value, in the shared
        // score-desc/id-asc order so the survivors are deterministic.
        taken.clear();
        for (id, v) in &vals {
            let bits = v.to_bits();
            if !selected.contains(&bits) {
                continue;
            }
            let n = taken.entry(bits).or_insert(0);
            if *n < cfg.cap_per_value {
                keep.insert(*id);
                *n += 1;
            }
        }
    }

    keep
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a `ComboMetrics` with only the fields retention reads set; the rest
    /// are zeroed (retention never inspects them).
    fn cm(combo_id: u32, n_closed: u64, score: Option<f64>, win_rate: f64) -> ComboMetrics {
        ComboMetrics {
            combo_id,
            n_fired: n_closed,
            n_open: 0,
            n_closed,
            win_rate,
            total_pnl_sol: 0.0,
            open_pnl_sol: 0.0,
            mean_pnl_pct: 0.0,
            median_pnl_pct: 0.0,
            p90_pnl_pct: 0.0,
            best_pnl_pct: 0.0,
            worst_pnl_pct: 0.0,
            std_pnl_pct: 0.0,
            profit_factor: None,
            mtm_pnl_pct: 0.0,
            score,
            expectancy_sol: 0.0,
            avg_holding_secs: 0.0,
            median_holding_secs: 0.0,
            n_exit_take_profit: 0,
            n_exit_stop_loss: 0,
            n_exit_trailing: 0,
            n_exit_stall: 0,
            n_exit_time: 0,
            n_exit_liquidity: 0,
            n_exit_next_kill: 0,
            n_exit_dead: 0,
            n_exit_metrics: 0,
            n_exit_open: 0,
        }
    }

    /// Build a combo whose every ranking metric equals `v` (distinct `v` per
    /// combo) — so all 11 metrics rank combos identically and the top/bottom
    /// selection is unambiguous regardless of which metric drives it.
    fn cm_uniform(combo_id: u32, n_closed: u64, v: f64) -> ComboMetrics {
        let mut m = cm(combo_id, n_closed, Some(v), v);
        m.total_pnl_sol = v;
        m.expectancy_sol = v;
        m.profit_factor = Some(v);
        m.median_pnl_pct = v;
        m.mean_pnl_pct = v;
        m.p90_pnl_pct = v;
        m.best_pnl_pct = v;
        m.worst_pnl_pct = v;
        m.std_pnl_pct = v;
        m
    }

    /// The docstring promises the result is **order-independent** — the input slice
    /// order must not change which combos are retained.
    ///
    /// This is the guard for hoisting the eligibility filter and the score tie-break
    /// sort out of the per-metric loop. That refactor replaced 11 per-metric sorts of
    /// a per-metric subset with ONE sort of the shared eligible set, so if the shared
    /// ordering were not a total order (e.g. ties left to input position), retention
    /// would silently start depending on how the fold happened to emit combos — and
    /// two runs over the same data would persist different rows.
    #[test]
    fn retention_is_order_independent() {
        // Mixed: distinct values, a heavily tied value, and ineligible combos. The
        // tie sits at the MAXIMUM so it lands in the top-3 distinct values and the
        // `cap_per_value` walk actually runs — a tie at a mid value would be selected
        // by no metric and would leave the tie-break untested.
        let mut metrics: Vec<ComboMetrics> = (0..40)
            .map(|i| cm_uniform(i, 5, f64::from(i)))
            .chain((40..70).map(|i| cm_uniform(i, 5, 100.0))) // 30 combos tied at the max
            .chain((70..80).map(|i| cm(i, 1, None, 0.0))) // under-sampled, ineligible
            .collect();
        let cfg = RetentionCfg::default();
        let baseline = retained_combo_ids(&metrics, 0, &cfg);

        // Several deterministic permutations, including full reversal.
        metrics.reverse();
        assert_eq!(retained_combo_ids(&metrics, 0, &cfg), baseline, "reversed input");
        metrics.rotate_left(37);
        assert_eq!(retained_combo_ids(&metrics, 0, &cfg), baseline, "rotated input");
        metrics.sort_by_key(|m| m.combo_id % 7); // interleaves tied and untied
        assert_eq!(retained_combo_ids(&metrics, 0, &cfg), baseline, "interleaved input");

        // Sanity: the tied value really did hit the cap, so the tie-break was
        // exercised rather than every tied combo being trivially kept.
        let tied_kept = (40..70).filter(|i| baseline.contains(i)).count();
        assert_eq!(tied_kept, cfg.cap_per_value, "tie cap should bound the 30 tied combos");
    }

    #[test]
    fn best_combo_always_kept() {
        // best_combo_id is under-sampled (n_closed < 2) so contributes to no
        // metric value set, yet must still survive for the summary JOIN.
        let metrics = vec![cm(0, 1, None, 0.0), cm(1, 5, Some(1.0), 0.5)];
        let keep = retained_combo_ids(&metrics, 0, &RetentionCfg::default());
        assert!(keep.contains(&0));
    }

    #[test]
    fn under_sampled_excluded_from_extremes() {
        // Only combo 2 clears min_closed; the two n_closed<2 combos (even with
        // extreme win_rate) must not be kept on a metric's strength. best=2.
        let metrics = vec![
            cm(0, 1, None, 1.0), // best win_rate but under-sampled
            cm(1, 1, None, 0.0), // worst win_rate but under-sampled
            cm(2, 9, Some(0.5), 0.5),
        ];
        let keep = retained_combo_ids(&metrics, 2, &RetentionCfg::default());
        assert!(!keep.contains(&0));
        assert!(!keep.contains(&1));
        assert!(keep.contains(&2));
    }

    #[test]
    fn tie_cap_bounds_flat_distribution() {
        // 100 eligible combos all sharing one win_rate value (and all None score):
        // the worst+best value collapses to one tied value, capped at cap_per_value.
        let cfg = RetentionCfg::default();
        let metrics: Vec<ComboMetrics> =
            (0..100).map(|i| cm(i, 5, None, 0.5)).collect();
        let keep = retained_combo_ids(&metrics, 0, &cfg);
        // win_rate is the only finite metric here (score None, others all 0.0 →
        // 0.0 is also a single tied value across total_pnl/mean/etc). Each metric
        // contributes <= cap_per_value combos; the union plus best stays small and
        // far below 100.
        assert!(keep.len() <= cfg.cap_per_value + 1, "kept {}", keep.len());
        assert!(keep.contains(&0)); // best
    }

    #[test]
    fn top_and_bottom_values_survive() {
        // 10 eligible combos, all 11 metrics monotone in combo_id (combo i → value
        // i): the bottom 3 and top 3 distinct values are kept on every metric, the
        // middle is dropped. cap doesn't bite (1 combo per distinct value).
        let cfg = RetentionCfg::default();
        let metrics: Vec<ComboMetrics> =
            (0..10).map(|i| cm_uniform(i, 5, i as f64)).collect();
        let keep = retained_combo_ids(&metrics, 0, &cfg);
        for id in [0u32, 1, 2, 7, 8, 9] {
            assert!(keep.contains(&id), "missing {id}");
        }
        for id in [3u32, 4, 5, 6] {
            assert!(!keep.contains(&id), "unexpectedly kept {id}");
        }
    }
}
