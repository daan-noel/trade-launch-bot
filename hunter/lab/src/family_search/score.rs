//! Fit broad, validate narrow (charter D1/D2), then select on **both sides**.
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
//!
//! **The two sides are graded by their own jobs (charter D11).** Entry decides
//! *safety* and exit decides *profit*, so the ranking alone cannot pick a draft: a
//! candidate must also clear a **win-rate** bar and a **return** bar, both read on the
//! held-out target. The win-rate bar is the ungated control's own win rate — an entry
//! gate that does not enter more safely than buying everything is not filtering
//! anything, and unlike an absolute figure that bar is a property of the cohort and
//! exists before any rule does (D6).

use trading_core::strategies::kernel::weighted_return_pct;

/// Below this, fit-broad is not established on this family. Set at the midpoint
/// between "no relationship" and the ρ = 0.833 the reference family measures — a
/// board that ranks anyway under a collapsed ρ is reporting noise as an ordering.
pub const RHO_FLOOR: f64 = 0.5;

/// One candidate's realized result on one cohort.
///
/// Every quantity travels as its own **sum** so re-weighting upstream stays exact:
/// a percent is never carried alone and a rate is never averaged. Pooling six cohorts
/// that differ by ~6× in size is the case that punishes any other choice.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CohortScore {
    pub pnl_sol: f64,
    /// Capital committed — `n_entered × buy_amount_sol`.
    pub entry_sol: f64,
    /// Positions that closed — the win rate's denominator.
    pub n_closed: u64,
    /// Of those, the ones that closed up.
    pub n_wins: u64,
}

impl CohortScore {
    pub fn ret_pct(&self) -> f64 {
        weighted_return_pct(self.pnl_sol, self.entry_sol)
    }

    /// Wins over closes, as a percent. `None` when nothing closed — a rule that never
    /// traded has no win rate, and 0% would read as "it lost every time".
    pub fn win_rate_pct(&self) -> Option<f64> {
        (self.n_closed > 0).then(|| 100.0 * self.n_wins as f64 / self.n_closed as f64)
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

/// Pool a win rate the same way money pools: `Σwins ÷ Σcloses`. A mean of per-cohort
/// rates would let a 12-trade cohort weigh as much as a 700-trade one.
pub fn pooled_win_rate_pct(scores: &[CohortScore]) -> Option<f64> {
    let closed: u64 = scores.iter().map(|s| s.n_closed).sum();
    let wins: u64 = scores.iter().map(|s| s.n_wins).sum();
    (closed > 0).then(|| 100.0 * wins as f64 / closed as f64)
}

/// Lower bound of the 95% Wilson interval on a win rate, in percent. `None` when
/// nothing closed — no interval exists around a rate that is not a measurement.
///
/// This is the honesty layer under the win-rate bar: on a cohort with 150 closes a
/// 3pp lift over the ungated control sits well inside binomial noise, and a board
/// that presents point estimates as "cleared the bar" reports noise as safety. The
/// bound is a **diagnostic**, never a selection input — tightening selection on the
/// held-out cohort would leak it.
pub fn wilson_low_pct(wins: u64, n_closed: u64) -> Option<f64> {
    if n_closed == 0 {
        return None;
    }
    let z = 1.96f64;
    let n = n_closed as f64;
    let p = wins as f64 / n;
    let z2 = z * z;
    let center = p + z2 / (2.0 * n);
    let half = z * ((p * (1.0 - p) + z2 / (4.0 * n)) / n).sqrt();
    Some(100.0 * ((center - half) / (1.0 + z2 / n)).max(0.0))
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
    /// Held-out win rate per candidate, in candidate order. The **safety** half.
    pub win_validate: Vec<Option<f64>>,
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

    /// The fit stage's pick, **ignoring the two-sided bars** — the ordering's head.
    /// [`select`] is what a board reports; this stays for the archive's row 1.
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
    let win_validate: Vec<Option<f64>> = validate.iter().map(|s| s.win_rate_pct()).collect();

    let key = if single_cohort { &ret_validate } else { &ret_fit };
    let mut rank_fit: Vec<usize> = (0..fit.len()).collect();
    rank_fit.sort_by(|&a, &b| {
        key[b].partial_cmp(&key[a]).unwrap_or(std::cmp::Ordering::Equal).then(a.cmp(&b))
    });

    // With no fit set there is no second opinion to correlate against — reporting a
    // ρ of 1.0 from `ret_validate` against itself would be a self-test that cannot fail.
    let rho = (!single_cohort).then(|| spearman(&ret_fit, &ret_validate)).flatten();
    BroadFit { rank_fit, ret_fit, ret_validate, win_validate, rho }
}

// ─────────────────────────── two-sided selection (charter D11) ────────────────

/// The bars a candidate must clear on the **held-out target** to be the draft.
///
/// Both are read narrow, never on the fit set: every candidate can be negative on the
/// pooled fit while the winner pays +31% on the target, so a profit bar applied to the
/// fit level would refuse the whole library.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SelectionBars {
    /// Win rate the ungated control achieves on the target, if it closed anything.
    /// The entry side has to beat *buying everything* or it is not filtering.
    pub control_win_pct: Option<f64>,
    /// An absolute floor the operator sets on top. `0.0` leaves the control as the
    /// only bar.
    pub floor_win_pct: f64,
    /// Closes a candidate needs before its win rate is believed. A 3-of-4 rule is not
    /// a 75% win rate.
    pub min_closed: u64,
}

impl SelectionBars {
    /// The effective win-rate bar: the stricter of the operator's floor and the
    /// cohort's own ungated rate.
    pub fn win_bar_pct(&self) -> f64 {
        self.control_win_pct.unwrap_or(0.0).max(self.floor_win_pct)
    }
}

/// Why a candidate is, or is not, the draft.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejected {
    /// Fewer closes than the bar — the win rate is not yet a measurement.
    TooFewCloses,
    /// The entry does not enter more safely than buying everything.
    WinRate,
    /// The exit does not make money on the held-out cohort.
    Return,
}

impl Rejected {
    pub fn label(self) -> &'static str {
        match self {
            Self::TooFewCloses => "too few closes",
            Self::WinRate => "win rate below the ungated control",
            Self::Return => "negative on the held-out cohort",
        }
    }
}

/// What the two-sided selection decided.
#[derive(Clone, Debug, PartialEq)]
pub struct Selection {
    /// The draft: the highest-ranked candidate clearing **both** bars. `None` when
    /// nothing does.
    pub chosen: Option<usize>,
    /// The ranking's own head, whether or not it cleared. Always reported, so a board
    /// can say "the best-ranked rule was refused, and here is why".
    pub top_ranked: Option<usize>,
    /// Why `top_ranked` is not `chosen`. Empty when they agree.
    pub top_rejected: Vec<Rejected>,
    /// Candidates the bars turned away, in rank order.
    pub n_rejected: usize,
    pub bars: SelectionBars,
}

/// Walk the fit ordering and take the first candidate that clears both bars.
///
/// Rank comes from the pooled family (validated at ρ = 0.833 on the reference family);
/// the bars come from the held-out cohort. Selecting on rank alone reports whichever
/// shape the search happened to favour; selecting on the bars alone throws away the
/// only cross-cohort evidence there is.
pub fn select(bf: &BroadFit, validate: &[CohortScore], bars: SelectionBars) -> Selection {
    let judge = |ci: usize| -> Vec<Rejected> {
        let s = validate.get(ci).copied().unwrap_or_default();
        let mut out = Vec::new();
        if s.n_closed < bars.min_closed {
            out.push(Rejected::TooFewCloses);
        }
        match s.win_rate_pct() {
            Some(w) if w >= bars.win_bar_pct() => {}
            Some(_) => out.push(Rejected::WinRate),
            None => out.push(Rejected::TooFewCloses),
        }
        // Spelled out because the obvious `<= 0.0` is **false** for NaN and would let
        // an unmeasurable return through the profit bar. A rule has to be positively
        // positive.
        let ret = bf.ret_validate.get(ci).copied().unwrap_or(0.0);
        if !ret.is_finite() || ret <= 0.0 {
            out.push(Rejected::Return);
        }
        out.dedup();
        out
    };

    let mut chosen = None;
    let mut n_rejected = 0;
    for &ci in &bf.rank_fit {
        if judge(ci).is_empty() {
            chosen = Some(ci);
            break;
        }
        n_rejected += 1;
    }
    let top_ranked = bf.winner();
    Selection {
        chosen,
        top_ranked,
        top_rejected: top_ranked.map(judge).unwrap_or_default(),
        n_rejected,
        bars,
    }
}

/// One term's contribution to the finalist, measured **narrow** — on the target
/// cohort, with that term dropped.
#[derive(Clone, Debug, PartialEq)]
pub struct TermContribution {
    pub label: String,
    /// Which side of the rule the term sits on — the two are graded by different
    /// jobs, so a board must never rank them in one list.
    pub is_entry: bool,
    pub ret_full_pct: f64,
    pub ret_without_pct: f64,
    pub win_full_pct: Option<f64>,
    pub win_without_pct: Option<f64>,
}

impl TermContribution {
    /// Points of return the term is worth on the target cohort.
    pub fn delta_pct(&self) -> f64 {
        self.ret_full_pct - self.ret_without_pct
    }

    /// Points of **win rate** the term is worth — the entry side's own currency.
    pub fn win_delta_pp(&self) -> Option<f64> {
        Some(self.win_full_pct? - self.win_without_pct?)
    }

    /// Whether dropping the term left the cohort byte-identical — the shape a broad
    /// fit is blind to, since a term worth nothing on five cohorts can be worth ten
    /// points on the sixth.
    pub fn inert(&self) -> bool {
        self.ret_full_pct == self.ret_without_pct
            && self.win_full_pct == self.win_without_pct
    }

    /// Does this term earn its place? An entry term earns it by making the rule
    /// **safer**, an exit term by making it **richer** — grading both on return alone
    /// is what deletes every entry condition.
    pub fn earns_its_place(&self) -> bool {
        if self.inert() {
            return false;
        }
        if self.is_entry {
            self.win_delta_pp().is_some_and(|d| d > 0.0) || self.delta_pct() > 0.0
        } else {
            self.delta_pct() > 0.0
        }
    }
}

/// Re-score the finalist on the **target cohort** with each term dropped in turn.
/// The caller supplies the re-scores; this is the comparison, ranked by the points
/// each term is worth so the load-bearing one reads first.
pub fn narrow_recheck(
    full: CohortScore,
    dropped: &[(String, bool, CohortScore)],
) -> Vec<TermContribution> {
    let mut out: Vec<TermContribution> = dropped
        .iter()
        .map(|(label, is_entry, s)| TermContribution {
            label: label.clone(),
            is_entry: *is_entry,
            ret_full_pct: full.ret_pct(),
            ret_without_pct: s.ret_pct(),
            win_full_pct: full.win_rate_pct(),
            win_without_pct: s.win_rate_pct(),
        })
        .collect();
    // Entry terms first (safety), then exit (profit) — each ranked in its own
    // currency, because sorting both by return buries every working entry gate.
    out.sort_by(|a, b| {
        b.is_entry
            .cmp(&a.is_entry)
            .then_with(|| {
                if a.is_entry {
                    b.win_delta_pp()
                        .unwrap_or(0.0)
                        .partial_cmp(&a.win_delta_pp().unwrap_or(0.0))
                        .unwrap_or(std::cmp::Ordering::Equal)
                } else {
                    b.delta_pct().partial_cmp(&a.delta_pct()).unwrap_or(std::cmp::Ordering::Equal)
                }
            })
            .then_with(|| a.label.cmp(&b.label))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(pnl: f64, entry: f64) -> CohortScore {
        CohortScore { pnl_sol: pnl, entry_sol: entry, n_closed: 100, n_wins: 50 }
    }

    fn sw(pnl: f64, entry: f64, closed: u64, wins: u64) -> CohortScore {
        CohortScore { pnl_sol: pnl, entry_sol: entry, n_closed: closed, n_wins: wins }
    }

    #[test]
    fn pooling_is_money_over_capital_and_order_free() {
        // A 565-token cohort at −1% and a 99-token cohort at +20%. Buy 0.01.
        let big = s(-0.0565, 5.65);
        let small = s(0.198, 0.99);
        let pooled = pooled_return_pct(&[big, small]);

        let want = 100.0 * (big.pnl_sol + small.pnl_sol) / (big.entry_sol + small.entry_sol);
        assert!((pooled - want).abs() < 1e-12);
        assert_eq!(pooled, pooled_return_pct(&[small, big]));

        let mean_of_pcts = (big.ret_pct() + small.ret_pct()) / 2.0;
        assert!(
            (pooled - mean_of_pcts).abs() > 5.0,
            "pooled {pooled} vs mean-of-percents {mean_of_pcts}"
        );
        assert!(pooled > 0.0 && pooled < 5.0, "{pooled}");
        assert_eq!(pooled_return_pct(&[]), 0.0);
    }

    /// A win rate pools like money does: by its own denominator, never as a mean.
    #[test]
    fn a_win_rate_pools_by_closes_not_by_cohort() {
        // 700 closes at 30% and 12 closes at 100%.
        let big = sw(0.0, 7.0, 700, 210);
        let small = sw(0.0, 0.12, 12, 12);
        let pooled = pooled_win_rate_pct(&[big, small]).unwrap();
        assert!((pooled - 100.0 * 222.0 / 712.0).abs() < 1e-9, "{pooled}");
        // A mean of rates reads 65% — the 12-trade cohort weighing as much as 700.
        let mean = (big.win_rate_pct().unwrap() + small.win_rate_pct().unwrap()) / 2.0;
        assert!(mean > 60.0 && pooled < 35.0, "pooled {pooled} vs mean {mean}");
        // Nothing closed ⇒ no rate at all, never a 0 that reads as "lost every time".
        assert_eq!(sw(0.0, 0.0, 0, 0).win_rate_pct(), None);
        assert_eq!(pooled_win_rate_pct(&[]), None);
    }

    /// The bound narrows with n and vanishes without closes — a 62% rate on 8 closes
    /// and on 800 are different claims, and the bar comparison has to know it.
    #[test]
    fn the_wilson_bound_narrows_with_sample_size_and_refuses_an_empty_one() {
        assert_eq!(wilson_low_pct(5, 0), None, "no closes, no interval");
        let thin = wilson_low_pct(5, 8).unwrap(); // 62.5% of 8
        let thick = wilson_low_pct(500, 800).unwrap(); // 62.5% of 800
        assert!(thin < thick, "thin {thin} vs thick {thick}");
        assert!(thick > 59.0 && thick < 62.5, "{thick}");
        assert!(thin < 35.0, "8 closes cannot pin a rate: {thin}");
        // Degenerate rates stay inside [0, 100].
        assert_eq!(wilson_low_pct(0, 20).unwrap(), 0.0);
        assert!(wilson_low_pct(20, 20).unwrap() < 100.0);
    }

    #[test]
    fn spearman_measures_rank_agreement_and_tolerates_ties() {
        let fit = [-1.24, -3.0, -8.5, -12.0];
        let holdout = [31.0, 12.0, 4.0, -9.0];
        assert!((spearman(&fit, &holdout).unwrap() - 1.0).abs() < 1e-12);
        let flipped: Vec<f64> = holdout.iter().rev().copied().collect();
        assert!((spearman(&fit, &flipped).unwrap() + 1.0).abs() < 1e-12);
        assert_eq!(spearman(&[1.0, 1.0, 2.0], &[5.0, 5.0, 9.0]), Some(1.0));
        assert_eq!(spearman(&[1.0, 1.0, 1.0], &[1.0, 2.0, 3.0]), None);
        assert_eq!(spearman(&[1.0], &[1.0]), None);
        assert_eq!(spearman(&[1.0, 2.0], &[1.0]), None);
    }

    #[test]
    fn the_fit_ranks_and_the_target_supplies_the_level() {
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
        let w = bf.winner().unwrap();
        assert!((bf.ret_validate[w] - 31.0).abs() < 1e-9);
    }

    #[test]
    fn a_collapsed_rho_stops_the_board_from_ranking_anyway() {
        let fit = vec![vec![s(0.3, 1.0)], vec![s(0.2, 1.0)], vec![s(0.1, 1.0)]];
        let validate = vec![s(-0.1, 1.0), s(0.5, 1.0), s(0.2, 1.0)];
        let bf = broad_fit(&fit, &validate);
        assert!(bf.rho.unwrap() < RHO_FLOOR);
        assert!(!bf.holds(), "fit-broad does not apply on this family");
    }

    #[test]
    fn a_family_of_one_reports_no_rho_and_ranks_on_the_target() {
        let fit = vec![Vec::new(), Vec::new()];
        let validate = vec![s(0.05, 1.0), s(0.40, 1.0)];
        let bf = broad_fit(&fit, &validate);
        assert_eq!(bf.rho, None, "there is no second opinion to self-test against");
        assert!(!bf.holds());
        assert_eq!(bf.rank_fit, vec![1, 0]);
        assert_eq!(bf.winner(), Some(1));
    }

    /// Entry is safety and exit is profit, so the draft has to clear BOTH — and the
    /// safety bar is the cohort's own ungated rate, not a number someone picked.
    #[test]
    fn selection_takes_the_first_ranked_candidate_clearing_both_bars() {
        // Rank order 0,1,2. Candidate 0 is rich but no safer than buying everything;
        // candidate 1 is safer AND positive; candidate 2 is safe but loses.
        let fit = vec![
            vec![sw(0.30, 1.0, 100, 40)],
            vec![sw(0.20, 1.0, 100, 62)],
            vec![sw(0.10, 1.0, 100, 80)],
        ];
        let validate = vec![
            sw(0.31, 1.0, 100, 40), // 40% — at the control's rate, no safer
            sw(0.18, 1.0, 100, 62), // 62% and +18%
            sw(-0.05, 1.0, 100, 80),
        ];
        let bf = broad_fit(&fit, &validate);
        assert_eq!(bf.rank_fit, vec![0, 1, 2]);

        let bars =
            SelectionBars { control_win_pct: Some(45.0), floor_win_pct: 0.0, min_closed: 8 };
        let sel = select(&bf, &validate, bars);

        assert_eq!(sel.top_ranked, Some(0));
        assert_eq!(sel.top_rejected, vec![Rejected::WinRate], "richest, but not safer");
        assert_eq!(sel.chosen, Some(1), "the draft is the first to clear both");
        assert_eq!(sel.n_rejected, 1);
        assert!((sel.bars.win_bar_pct() - 45.0).abs() < 1e-9);
    }

    #[test]
    fn a_thin_win_rate_is_not_a_measurement_and_an_absolute_floor_can_raise_the_bar() {
        // 3 of 4 is not a 75% win rate.
        let validate = vec![sw(0.9, 0.04, 4, 3)];
        let bf = broad_fit(&[vec![]], &validate);
        let bars = SelectionBars { control_win_pct: Some(40.0), floor_win_pct: 0.0, min_closed: 8 };
        let sel = select(&bf, &validate, bars);
        assert_eq!(sel.chosen, None);
        assert_eq!(sel.top_rejected, vec![Rejected::TooFewCloses]);

        // An operator floor above the control's rate is the stricter of the two.
        let strict =
            SelectionBars { control_win_pct: Some(40.0), floor_win_pct: 55.0, min_closed: 1 };
        assert!((strict.win_bar_pct() - 55.0).abs() < 1e-9);
        assert_eq!(select(&bf, &validate, strict).chosen, Some(0), "75% clears a 55% floor");
    }

    #[test]
    fn nothing_clearing_the_bars_is_reported_as_no_draft_with_a_reason() {
        let validate = vec![sw(-0.2, 1.0, 100, 20), sw(-0.1, 1.0, 100, 25)];
        let bf = broad_fit(&[vec![], vec![]], &validate);
        let bars = SelectionBars { control_win_pct: Some(50.0), floor_win_pct: 0.0, min_closed: 8 };
        let sel = select(&bf, &validate, bars);
        assert_eq!(sel.chosen, None);
        assert_eq!(sel.n_rejected, 2);
        assert!(sel.top_rejected.contains(&Rejected::WinRate));
        assert!(sel.top_rejected.contains(&Rejected::Return));
    }

    /// An entry term earns its place by raising the WIN RATE. Grading it on return
    /// alone is the recorded reason a search deletes every entry condition.
    #[test]
    fn the_narrow_recheck_grades_each_side_in_its_own_currency() {
        let full = sw(0.31, 1.0, 100, 62);
        let dropped = vec![
            // Exit alarm: worth 10 points of return.
            ("nonvol_buy(2s) >= 1.6".to_string(), false, sw(0.21, 1.0, 120, 60)),
            // Entry idea: costs a little return but buys 14 points of win rate —
            // exactly the trade the operator is making, and it must read as a keeper.
            ("liquidity > 30".to_string(), true, sw(0.46, 1.4, 140, 67)),
            // Dead weight on both counts.
            ("gross_flow(10s) < 15".to_string(), false, sw(0.31, 1.0, 100, 62)),
        ];
        let rows = narrow_recheck(full, &dropped);

        // Entry rows lead, ranked by win rate.
        assert!(rows[0].is_entry && rows[0].label == "liquidity > 30");
        let win_gain = rows[0].win_delta_pp().unwrap();
        assert!((win_gain - (62.0 - 100.0 * 67.0 / 140.0)).abs() < 1e-9, "{win_gain}");
        assert!(win_gain > 13.0 && rows[0].delta_pct() < 0.0);
        assert!(rows[0].earns_its_place(), "safety is the entry side's currency");

        // Exit rows follow, ranked by money.
        assert!(!rows[1].is_entry && rows[1].label == "nonvol_buy(2s) >= 1.6");
        assert!((rows[1].delta_pct() - 10.0).abs() < 1e-9);
        assert!(rows[1].earns_its_place());

        // Inert on both counts ⇒ visibly dead weight, not a small number.
        assert!(rows[2].inert() && !rows[2].earns_its_place());
        assert_eq!(rows[2].delta_pct(), 0.0);
    }
}
