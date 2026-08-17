//! The oracle denominator and the capture ratio (charter D3).
//!
//! *"Of the tokens entered, N had a winning exit available and the rule took X% of
//! it; M never had one at all."* The first number grades the **exit**; the second
//! grades the **entry**, and it needs no exit rule to exist — which is how the entry
//! side becomes measurable at all.
//!
//! The oracle is a property of `(token, entry moment)`, never of the exit rule, so
//! [`CorpusToken::peak_after`] is built once per corpus at load and reused by every
//! candidate of every stage. Recomputing it per candidate is the single biggest
//! available mistake in this job.

use chrono::{DateTime, Utc};

use trading_core::strategies::kernel::round_trip_with_costs;

use crate::sweep::corpus::CorpusToken;
use crate::sweep::generic::strategy::Pricing;
use crate::sweep::strategy::TokenOutcome;

/// How much of the money that was actually available a rule took.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Capture {
    /// `100 × Σ realized_pnl ÷ Σ oracle_pnl`, over the entries where a profitable
    /// exit existed at all. Money-over-money, never a mean of per-token ratios —
    /// the same pooling rule as [PnL %](crate::sweep). `None` when no entry had
    /// upside (the denominator would be zero, and 0% would read as "took nothing").
    pub capture_pct: Option<f64>,
    /// Entries where the oracle round-trip pays > 0 — the capture denominator's
    /// population. Grades the exit.
    pub n_with_upside: u64,
    /// Entries where **no** profitable exit ever existed after the fill. Its own
    /// line, never folded into the ratio: this grades the *entry*, and an entry with
    /// no upside is not an exit's failure.
    pub n_no_upside: u64,
    /// Σ oracle pnl over the with-upside entries.
    pub oracle_pnl_sol: f64,
    /// Σ realized pnl over the **same** entries — so numerator and denominator cover
    /// one population and the ratio is meaningful.
    pub realized_pnl_sol: f64,
}

impl Capture {
    /// Entries scored — the two halves always partition the fired set.
    pub fn n_entered(&self) -> u64 {
        self.n_with_upside + self.n_no_upside
    }
}

/// Streaming [`Capture`] accumulator: one pass over outcomes already in hand, O(1)
/// state. The family loop folds a cohort into one of these while it walks.
#[derive(Clone, Copy, Debug, Default)]
pub struct CaptureAcc {
    n_with_upside: u64,
    n_no_upside: u64,
    oracle_pnl_sol: f64,
    realized_pnl_sol: f64,
}

impl CaptureAcc {
    /// Fold one (token, outcome) pair. Non-fired outcomes are ignored — capture is a
    /// question about entries, not about the population.
    pub fn record(&mut self, token: &CorpusToken, outcome: &TokenOutcome, pricing: &Pricing) {
        if !outcome.fired {
            return;
        }
        match oracle_pnl_sol(token, outcome, pricing) {
            Some(oracle) if oracle > 0.0 => {
                self.n_with_upside += 1;
                self.oracle_pnl_sol += oracle;
                self.realized_pnl_sol += outcome.pnl_sol as f64;
            }
            // Includes "the corpus carries no oracle curve" — an un-priceable entry
            // is never counted as upside the rule failed to take.
            _ => self.n_no_upside += 1,
        }
    }

    pub fn finish(self) -> Capture {
        Capture {
            capture_pct: (self.oracle_pnl_sol > 0.0)
                .then(|| 100.0 * self.realized_pnl_sol / self.oracle_pnl_sol),
            n_with_upside: self.n_with_upside,
            n_no_upside: self.n_no_upside,
            oracle_pnl_sol: self.oracle_pnl_sol,
            realized_pnl_sol: self.realized_pnl_sol,
        }
    }
}

/// [`CaptureAcc`] over a whole cohort — `tokens` and `outcomes` in lockstep.
pub fn capture(
    tokens: &[CorpusToken],
    outcomes: &[TokenOutcome],
    pricing: &Pricing,
) -> Capture {
    let mut acc = CaptureAcc::default();
    for (t, o) in tokens.iter().zip(outcomes) {
        acc.record(t, o, pricing);
    }
    acc.finish()
}

/// Index of the first trade printed **strictly after** `at` — the earliest row an
/// exit could fill against. Per-mint corpus order is block-time monotone, so this is
/// a binary search, not a scan.
fn first_row_after(token: &CorpusToken, at: DateTime<Utc>) -> usize {
    token.trades.partition_point(|t| t.block_time <= at)
}

/// The SOL-side pool depth this entry's impact is charged against — the entry row's
/// own reserve, the shape [`round_trip_with_costs`] defines.
fn entry_depth(token: &CorpusToken, at: DateTime<Utc>) -> Option<f64> {
    first_row_after(token, at)
        .checked_sub(1)
        .and_then(|i| token.trades.get(i))
        .and_then(|t| t.real_reserve_sol)
        .filter(|r| r.is_finite() && *r > 0.0)
}

/// Every priceable entry's **net** oracle round trip, as a percent of notional, and
/// the entry depths that priced them.
///
/// This is the cohort's upside distribution: what the *best* exit available after
/// each fill would have paid, after the same round trip a realized exit pays. Feed it
/// the **ungated control's** outcomes and it is rule-free — a property of the cohort,
/// which is what makes it usable before a generator has produced anything (D8).
///
/// Losers are kept. A distribution filtered to the winners has a positive median by
/// construction and can never refuse anything.
pub struct OracleMoves {
    /// Net oracle round trip per priceable entry, in percent of notional.
    pub pct: Vec<f64>,
    /// Entry-row pool depth per priceable entry, in the same order.
    pub depth_sol: Vec<f64>,
    /// Entries whose oracle round trip pays > 0 — context for the median, never a
    /// filter on it.
    pub n_with_upside: u64,
}

/// Fold a cohort's outcomes into its [`OracleMoves`]. `token_idx[i]` is the corpus
/// index `outcomes[i]` belongs to.
pub fn oracle_moves(
    tokens: &[CorpusToken],
    outcomes: &[TokenOutcome],
    token_idx: &[usize],
    pricing: &Pricing,
) -> OracleMoves {
    let mut out =
        OracleMoves { pct: Vec::new(), depth_sol: Vec::new(), n_with_upside: 0 };
    if pricing.buy_amount_sol <= 0.0 {
        return out;
    }
    for (o, &ti) in outcomes.iter().zip(token_idx) {
        if !o.fired {
            continue;
        }
        let Some(token) = tokens.get(ti) else { continue };
        let Some(sol) = oracle_pnl_sol(token, o, pricing) else { continue };
        if sol > 0.0 {
            out.n_with_upside += 1;
        }
        out.pct.push(100.0 * sol / pricing.buy_amount_sol);
        if let Some(d) = o.entry_time.and_then(|at| entry_depth(token, at)) {
            out.depth_sol.push(d);
        }
    }
    out
}

/// The middle value of a sample. `None` on an empty one; an even count averages the
/// two middles.
pub fn median(values: &[f64]) -> Option<f64> {
    let mut v: Vec<f64> = values.iter().copied().filter(|x| x.is_finite()).collect();
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = v.len();
    Some(if n % 2 == 1 { v[n / 2] } else { 0.5 * (v[n / 2 - 1] + v[n / 2]) })
}

/// What **one round trip costs** at this buy size and pool depth, as a percent of
/// notional — the move a rule must clear before it earns anything (D8).
///
/// Priced as a flat trade (entry price == exit price) through the run's own
/// [`CostModel`](trading_core::strategies::kernel::CostModel), so it moves with the
/// buy size, the fee tuning and the pool depth instead of being a constant somebody
/// measured once on one family. The result is independent of the price level: gross
/// return is `exit ÷ entry`, so a flat trade leaves nothing but cost.
pub fn execution_band_pct(pricing: &Pricing, depth_sol: Option<f64>) -> f64 {
    if pricing.buy_amount_sol <= 0.0 {
        return 0.0;
    }
    let (pnl_sol, _) =
        round_trip_with_costs(1.0, 1.0, pricing.buy_amount_sol, depth_sol, &pricing.cost);
    -100.0 * pnl_sol / pricing.buy_amount_sol
}

/// The best price still printed after `at` — what an exit could have got.
///
/// `None` when the corpus was loaded without [`Selection::with_oracle`], when no
/// print follows the entry at all, or when nothing after it carries a usable price.
///
/// [`Selection::with_oracle`]: crate::sweep::corpus::Selection::with_oracle
pub fn oracle_exit_price(token: &CorpusToken, at: DateTime<Utc>) -> Option<f64> {
    let peak = token.peak_after.as_ref()?;
    let p = *peak.get(first_row_after(token, at))?;
    (p.is_finite() && p > 0.0).then_some(p as f64)
}

/// The oracle round-trip for one realized entry, in SOL.
///
/// Charged the **same** round trip as the realized exit — [`round_trip_with_costs`]
/// at the run's own [`CostModel`](trading_core::strategies::kernel::CostModel) and
/// notional. A denominator that pays no fees, no fixed per-leg cost, and no price
/// impact is not a comparison; it is a strictly larger number that makes every rule
/// look worse by a constant nobody can name.
///
/// The impact depth is the entry row's own reserve on both legs — the shape
/// [`round_trip_with_costs`] defines and the one the realized open-mark uses. Pricing
/// the *peak* row's depth would need the argmax index, which is a second 4 B column
/// for a second-order correction to a bound.
pub fn oracle_pnl_sol(
    token: &CorpusToken,
    outcome: &TokenOutcome,
    pricing: &Pricing,
) -> Option<f64> {
    let entry_price = outcome.entry_price?;
    let entry_at = outcome.entry_time?;
    let exit_price = oracle_exit_price(token, entry_at)?;
    let depth = entry_depth(token, entry_at);
    let (pnl_sol, _) = round_trip_with_costs(
        entry_price,
        exit_price,
        pricing.buy_amount_sol,
        depth,
        &pricing.cost,
    );
    Some(pnl_sol)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::family_search::fixtures::{outcome_at, pricing, token_from_prices};
    use crate::sweep::projection::suffix_peak;

    #[test]
    fn suffix_peak_equals_the_naive_backward_max() {
        let t = token_from_prices(&[1.0, 5.0, 2.0, 9.0, 3.0]);
        let got = suffix_peak(&t.trades);
        // Naive: for every i, the max over trades[i..].
        for (i, &got_i) in got.iter().enumerate() {
            let want = t.trades[i..]
                .iter()
                .filter_map(|tr| {
                    use crate::models::trade::TradeRow;
                    tr.chart_spot_price()
                })
                .fold(f64::NEG_INFINITY, f64::max);
            assert!(
                (got_i as f64 - want).abs() < 1e-9 * want.abs().max(1.0),
                "row {i}: {got_i} vs {want}"
            );
        }
        // An empty history has no peak to report, not a fabricated zero.
        assert!(suffix_peak(&[]).is_empty());
    }

    #[test]
    fn oracle_reads_only_prints_after_the_entry() {
        // Price peaks at row 1 (BEFORE the entry) and again, lower, at row 3.
        let t = token_from_prices(&[1.0, 9.0, 1.0, 4.0, 1.0]).with_oracle();
        let entry_at = t.trades[2].block_time;
        // The 9.0 is already gone; the best still available is the 4.0.
        assert_eq!(oracle_exit_price(&t, entry_at), Some(4.0));
        // Entering on the last print leaves nothing to exit into.
        let last = t.trades[4].block_time;
        assert_eq!(oracle_exit_price(&t, last), None);
    }

    #[test]
    fn oracle_pays_the_same_round_trip_as_the_realized_exit() {
        let t = token_from_prices(&[1.0, 2.0, 4.0]).with_oracle();
        let p = pricing();
        let entry_at = t.trades[0].block_time;
        let o = outcome_at(entry_at, 1.0, 0.0);
        let oracle = oracle_pnl_sol(&t, &o, &p).expect("priced");

        // The identical call the realized close makes, at the oracle's exit price.
        let depth = t.trades[0].real_reserve_sol;
        let (want, _) = round_trip_with_costs(1.0, 4.0, p.buy_amount_sol, depth, &p.cost);
        assert_eq!(oracle, want);
        // Costs are actually charged: a 4x gross is NOT 3x the notional net.
        assert!(oracle < 3.0 * p.buy_amount_sol, "the round trip must cost something");
    }

    #[test]
    fn a_token_that_only_falls_lands_in_no_upside() {
        // Monotone decline after the entry: no profitable exit ever existed.
        let falling = token_from_prices(&[10.0, 9.0, 8.0, 7.0]).with_oracle();
        // A clean winner, for contrast.
        let rising = token_from_prices(&[1.0, 2.0, 4.0]).with_oracle();
        let p = pricing();

        let outs = [
            outcome_at(falling.trades[0].block_time, 10.0, -0.004),
            outcome_at(rising.trades[0].block_time, 1.0, 0.02),
        ];
        let c = capture(&[falling.clone(), rising.clone()], &outs, &p);

        assert_eq!(c.n_no_upside, 1, "the faller has no upside to capture");
        assert_eq!(c.n_with_upside, 1);
        assert_eq!(c.n_entered(), 2);
        // The faller is absent from BOTH sides of the ratio, not just the numerator.
        let want_oracle = oracle_pnl_sol(&rising, &outs[1], &p).unwrap();
        assert_eq!(c.oracle_pnl_sol, want_oracle);
        let realized = 0.02f32 as f64;
        assert_eq!(c.realized_pnl_sol, realized);
        assert_eq!(c.capture_pct, Some(100.0 * realized / want_oracle));
    }

    #[test]
    fn capture_pools_by_money_not_by_mean_of_ratios() {
        // Two entries: a big one that captures 10%, a small one that captures 90%.
        // A mean of ratios reads 50%; pooling by money reads the big one's answer.
        let big = token_from_prices(&[1.0, 100.0]).with_oracle();
        let small = token_from_prices(&[1.0, 1.5]).with_oracle();
        let p = pricing();
        let mut outs = [
            outcome_at(big.trades[0].block_time, 1.0, 0.0),
            outcome_at(small.trades[0].block_time, 1.0, 0.0),
        ];
        let big_oracle = oracle_pnl_sol(&big, &outs[0], &p).unwrap();
        let small_oracle = oracle_pnl_sol(&small, &outs[1], &p).unwrap();
        assert!(big_oracle > 0.0 && small_oracle > 0.0);
        outs[0].pnl_sol = (0.10 * big_oracle) as f32;
        outs[1].pnl_sol = (0.90 * small_oracle) as f32;

        let c = capture(&[big, small], &outs, &p);
        let pooled = c.capture_pct.expect("both have upside");
        let mean_of_ratios = (10.0 + 90.0) / 2.0;
        assert!(
            (pooled - mean_of_ratios).abs() > 20.0,
            "a mean-of-percents implementation would read ~{mean_of_ratios}, got {pooled}"
        );
        assert!(pooled < 20.0, "the big entry dominates the pool: {pooled}");
    }

    #[test]
    fn a_corpus_without_the_oracle_column_reports_no_upside_not_a_ratio() {
        // `with_oracle` off ⇒ nothing is priceable ⇒ every entry is "no upside",
        // never a capture percentage computed off a missing denominator.
        let t = token_from_prices(&[1.0, 4.0]);
        let outs = [outcome_at(t.trades[0].block_time, 1.0, 0.5)];
        let c = capture(&[t], &outs, &pricing());
        assert_eq!(c.n_with_upside, 0);
        assert_eq!(c.n_no_upside, 1);
        assert_eq!(c.capture_pct, None);
    }
}
