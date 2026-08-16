//! Fit broad, validate narrow (charter D1/D2).
//!
//! Exit logic is portable — the same exit improves 6 of 6 cohorts, losers included —
//! so the **ordering** comes from a pooled fit across the family. Level does not
//! transfer: on the reference family every candidate is negative on the fit set
//! (best −1.24%) while the winner pays +31% on the held-out target. Take the ranking
//! from the fit, take the number from the target, and never quote a fit level.
//!
//! ρ (Spearman between fit rank and held-out rank) is a **self-test, not a result**:
//! it grades the *procedure* on this family. Where it collapses, fit-broad does not
//! apply here and the board says so instead of ranking anyway.

use trading_core::strategies::kernel::weighted_return_pct;

/// Below this, fit-broad is not established on this family. Set at the midpoint
/// between "no relationship" and the ρ = 0.833 the reference family measures — a
/// board that ranks anyway under a collapsed ρ is reporting noise as an ordering.
pub const RHO_FLOOR: f64 = 0.5;

/// One candidate's realized result on one cohort. Both sums travel together so a
/// re-weighting anywhere upstream stays exact: percent is never carried alone.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CohortScore {
    pub pnl_sol: f64,
    /// Capital committed — `n_entered × buy_amount_sol`.
    pub entry_sol: f64,
}

impl CohortScore {
    pub fn ret_pct(&self) -> f64 {
        weighted_return_pct(self.pnl_sol, self.entry_sol)
    }
}

/// Pool a candidate across cohorts: `Σpnl_sol ÷ Σentry_sol`, the one PnL %
/// definition. **Never** a mean of per-cohort percents — that lets a 99-token cohort
/// outvote a 565-token one, and the reference family's cohorts differ by ~6×.
pub fn pooled_return_pct(scores: &[CohortScore]) -> f64 {
    let pnl: f64 = scores.iter().map(|s| s.pnl_sol).sum();
    let entry: f64 = scores.iter().map(|s| s.entry_sol).sum();
    weighted_return_pct(pnl, entry)
}

/// Fractional ranks (1-based, ties averaged), ascending. Ties must average or a
/// family where several candidates score identically would get an ordering invented
/// by input order.
fn ranks(v: &[f64]) -> Vec<f64> {
    let mut idx: Vec<usize> = (0..v.len()).collect();
    idx.sort_by(|&a, &b| v[a].partial_cmp(&v[b]).unwrap_or(std::cmp::Ordering::Equal));
    let mut out = vec![0.0; v.len()];
    let mut i = 0;
    while i < idx.len() {
        let mut j = i + 1;
        while j < idx.len() && v[idx[j]] == v[idx[i]] {
            j += 1;
        }
        // Mean of the 1-based positions this tie group spans.
        let rank = (i + j + 1) as f64 / 2.0;
        for &k in &idx[i..j] {
            out[k] = rank;
        }
        i = j;
    }
    out
}

/// Spearman rank correlation. `None` when there is nothing to correlate: fewer than
/// two points, or one side constant (every candidate tied ⇒ no ordering to agree
/// with, and a fabricated 0 would read as "the procedure failed").
pub fn spearman(a: &[f64], b: &[f64]) -> Option<f64> {
    if a.len() != b.len() || a.len() < 2 {
        return None;
    }
    let (ra, rb) = (ranks(a), ranks(b));
    let n = a.len() as f64;
    let (ma, mb) = (ra.iter().sum::<f64>() / n, rb.iter().sum::<f64>() / n);
    let mut num = 0.0;
    let (mut da, mut db) = (0.0, 0.0);
    for i in 0..a.len() {
        let (x, y) = (ra[i] - ma, rb[i] - mb);
        num += x * y;
        da += x * x;
        db += y * y;
    }
    (da > 0.0 && db > 0.0).then(|| num / (da * db).sqrt())
}

/// The broad fit and its self-test, over a candidate set.
#[derive(Clone, Debug, PartialEq)]
pub struct BroadFit {
    /// Candidate indices best-first by the pooled fit — **the** output of this stage.
    pub rank_fit: Vec<usize>,
    /// Pooled fit return per candidate, in candidate order. Rank-only: quoting one as
    /// a level is the mistake this whole split exists to prevent.
    pub ret_fit: Vec<f64>,
    /// Held-out target-cohort return per candidate, in candidate order. This is the
    /// number a report prints.
    pub ret_validate: Vec<f64>,
    /// Spearman(fit, held-out) across candidates. `None` when it cannot be computed
    /// (a family of one, or fewer than two candidates).
    pub rho: Option<f64>,
}

impl BroadFit {
    /// Whether fit-broad is established on this family (D2). `false` also covers
    /// "ρ could not be computed" — an unmeasured procedure is not a validated one.
    pub fn holds(&self) -> bool {
        self.rho.is_some_and(|r| r >= RHO_FLOOR)
    }

    /// The fit stage's pick: best pooled fit. The **level** to report for it is
    /// `ret_validate[winner]`, never `ret_fit[winner]`.
    pub fn winner(&self) -> Option<usize> {
        self.rank_fit.first().copied()
    }
}

/// Rank candidates on the pooled fit cohorts and read their held-out level.
///
/// `fit[c]` is candidate `c`'s score on each **fit** cohort (the family minus the
/// target); `validate[c]` is the same candidate on the target cohort alone. A family
/// of one leaves `fit[c]` empty — the run degrades to single-cohort, ρ is `None`, and
/// the ranking falls back to the target's own return rather than to nothing.
pub fn broad_fit(fit: &[Vec<CohortScore>], validate: &[CohortScore]) -> BroadFit {
    assert_eq!(fit.len(), validate.len(), "fit and validate must cover the same candidates");
    let single_cohort = fit.iter().all(|f| f.is_empty());
    let ret_fit: Vec<f64> = fit.iter().map(|f| pooled_return_pct(f)).collect();
    let ret_validate: Vec<f64> = validate.iter().map(|s| s.ret_pct()).collect();

    let key = if single_cohort { &ret_validate } else { &ret_fit };
    let mut rank_fit: Vec<usize> = (0..fit.len()).collect();
    rank_fit.sort_by(|&a, &b| {
        key[b].partial_cmp(&key[a]).unwrap_or(std::cmp::Ordering::Equal).then(a.cmp(&b))
    });

    // With no fit set there is no second opinion to correlate against — reporting a
    // ρ of 1.0 from `ret_validate` against itself would be a self-test that cannot fail.
    let rho = (!single_cohort).then(|| spearman(&ret_fit, &ret_validate)).flatten();
    BroadFit { rank_fit, ret_fit, ret_validate, rho }
}

/// One term's contribution to the finalist, measured **narrow** — on the target
/// cohort, with that term dropped.
#[derive(Clone, Debug, PartialEq)]
pub struct TermContribution {
    pub label: String,
    pub ret_full_pct: f64,
    pub ret_without_pct: f64,
}

impl TermContribution {
    /// Points the term is worth on the target cohort. Zero ⇒ the drop changed
    /// nothing at all.
    pub fn delta_pct(&self) -> f64 {
        self.ret_full_pct - self.ret_without_pct
    }

    /// Whether dropping the term left the cohort byte-identical — the shape a broad
    /// fit is blind to, since a term worth nothing on five cohorts can be worth ten
    /// points on the sixth.
    pub fn inert(&self) -> bool {
        self.ret_full_pct == self.ret_without_pct
    }
}

/// Re-score the finalist on the **target cohort** with each term dropped in turn.
/// The caller supplies the re-scores; this is the comparison, ranked by the points
/// each term is worth so the load-bearing one reads first.
pub fn narrow_recheck(full: CohortScore, dropped: &[(String, CohortScore)]) -> Vec<TermContribution> {
    let ret_full_pct = full.ret_pct();
    let mut out: Vec<TermContribution> = dropped
        .iter()
        .map(|(label, s)| TermContribution {
            label: label.clone(),
            ret_full_pct,
            ret_without_pct: s.ret_pct(),
        })
        .collect();
    out.sort_by(|a, b| {
        b.delta_pct().partial_cmp(&a.delta_pct()).unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(pnl: f64, entry: f64) -> CohortScore {
        CohortScore { pnl_sol: pnl, entry_sol: entry }
    }

    #[test]
    fn pooling_is_money_over_capital_and_order_free() {
        // A 565-token cohort at −1% and a 99-token cohort at +20%. Buy 0.01.
        let big = s(-0.0565, 5.65);
        let small = s(0.198, 0.99);
        let pooled = pooled_return_pct(&[big, small]);

        // Σpnl / Σentry, exactly.
        let want = 100.0 * (big.pnl_sol + small.pnl_sol) / (big.entry_sol + small.entry_sol);
        assert!((pooled - want).abs() < 1e-12);

        // Swapping the cohorts' order does not move it.
        assert_eq!(pooled, pooled_return_pct(&[small, big]));

        // A mean-of-percents implementation reads ~+9.5% — a rank-flipping difference
        // from the +2.1% the money actually pays.
        let mean_of_pcts = (big.ret_pct() + small.ret_pct()) / 2.0;
        assert!(
            (pooled - mean_of_pcts).abs() > 5.0,
            "pooled {pooled} vs mean-of-percents {mean_of_pcts}"
        );
        assert!(pooled > 0.0 && pooled < 5.0, "{pooled}");
        // An empty pool has no capital and therefore no return to invent.
        assert_eq!(pooled_return_pct(&[]), 0.0);
    }

    #[test]
    fn spearman_measures_rank_agreement_and_tolerates_ties() {
        // Perfectly ordered, wildly different levels — rank transfers, level does not.
        let fit = [-1.24, -3.0, -8.5, -12.0];
        let holdout = [31.0, 12.0, 4.0, -9.0];
        assert!((spearman(&fit, &holdout).unwrap() - 1.0).abs() < 1e-12);
        // Reversed ⇒ −1.
        let flipped: Vec<f64> = holdout.iter().rev().copied().collect();
        assert!((spearman(&fit, &flipped).unwrap() + 1.0).abs() < 1e-12);
        // Ties average rather than taking their input order as an ordering.
        assert_eq!(spearman(&[1.0, 1.0, 2.0], &[5.0, 5.0, 9.0]), Some(1.0));
        // No ordering to agree with ⇒ no correlation, never a fabricated 0.
        assert_eq!(spearman(&[1.0, 1.0, 1.0], &[1.0, 2.0, 3.0]), None);
        assert_eq!(spearman(&[1.0], &[1.0]), None);
        assert_eq!(spearman(&[1.0, 2.0], &[1.0]), None);
    }

    #[test]
    fn the_fit_ranks_and_the_target_supplies_the_level() {
        // Three candidates. Every one is NEGATIVE on the fit set; the best of them
        // pays +31% on the held-out target. That is the expected shape, not a bug.
        let fit = vec![
            vec![s(-0.0124, 1.0), s(-0.02, 1.0)], // candidate 0: best fit
            vec![s(-0.05, 1.0), s(-0.06, 1.0)],
            vec![s(-0.12, 1.0), s(-0.10, 1.0)],
        ];
        let validate = vec![s(0.31, 1.0), s(0.12, 1.0), s(-0.09, 1.0)];
        let bf = broad_fit(&fit, &validate);

        assert_eq!(bf.rank_fit, vec![0, 1, 2]);
        assert!(bf.ret_fit.iter().all(|&r| r < 0.0), "the fit level is negative throughout");
        assert!((bf.rho.unwrap() - 1.0).abs() < 1e-12);
        assert!(bf.holds());
        // The winner's reportable number comes from the held-out cohort.
        let w = bf.winner().unwrap();
        assert!((bf.ret_validate[w] - 31.0).abs() < 1e-9);
    }

    #[test]
    fn a_collapsed_rho_stops_the_board_from_ranking_anyway() {
        // Fit order and holdout order disagree completely.
        let fit = vec![vec![s(0.3, 1.0)], vec![s(0.2, 1.0)], vec![s(0.1, 1.0)]];
        let validate = vec![s(-0.1, 1.0), s(0.5, 1.0), s(0.2, 1.0)];
        let bf = broad_fit(&fit, &validate);
        assert!(bf.rho.unwrap() < RHO_FLOOR);
        assert!(!bf.holds(), "fit-broad does not apply on this family");
    }

    #[test]
    fn a_family_of_one_reports_no_rho_and_ranks_on_the_target() {
        // No fit cohorts at all — the run degrades to single-cohort.
        let fit = vec![Vec::new(), Vec::new()];
        let validate = vec![s(0.05, 1.0), s(0.40, 1.0)];
        let bf = broad_fit(&fit, &validate);
        assert_eq!(bf.rho, None, "there is no second opinion to self-test against");
        assert!(!bf.holds());
        // Ranking still happens — off the only cohort there is.
        assert_eq!(bf.rank_fit, vec![1, 0]);
        assert_eq!(bf.winner(), Some(1));
    }

    #[test]
    fn the_narrow_recheck_finds_a_term_a_broad_fit_is_blind_to() {
        // The finalist pays +31% on the target. Dropping `nonvol_buy >= 1.6 @2s`
        // costs 10 points there while leaving two other cohorts byte-identical —
        // only this stage sees it.
        let full = s(0.31, 1.0);
        let dropped = vec![
            ("stall >= 30".to_string(), s(0.29, 1.0)),
            ("nonvol_buy(2s) >= 1.6".to_string(), s(0.21, 1.0)),
            ("gross_flow(10s) < 15".to_string(), s(0.31, 1.0)),
        ];
        let rows = narrow_recheck(full, &dropped);

        // Ranked by what each term is worth, so the load-bearing one reads first.
        assert_eq!(rows[0].label, "nonvol_buy(2s) >= 1.6");
        assert!((rows[0].delta_pct() - 10.0).abs() < 1e-9);
        assert!((rows[1].delta_pct() - 2.0).abs() < 1e-9);
        // A term whose removal changes nothing is visibly inert, not a small number.
        assert!(rows[2].inert());
        assert_eq!(rows[2].delta_pct(), 0.0);
    }
}
