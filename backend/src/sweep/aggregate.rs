//! Aggregate per-(combo, token) [`TokenOutcome`]s into one ranked metrics row
//! per combo — the param-pair table the UI shows. The sweep produces combos ×
//! tokens outcomes; this collapses them to `combos` rows (bounded — hundreds to
//! low thousands), small enough to hold in RAM, persist whole, and serve whole.
//!
//! Metrics cover the criteria a param search ranks on: profitability
//! (`total_pnl_sol`, `expectancy_sol`, `profit_factor`), success (`win_rate`),
//! central return (`mean`/`median_pnl_pct`), tails (`best`/`worst`/`p90`),
//! holding time, and the exit-reason mix.

use crate::sweep::strategy::{ExitCode, TokenOutcome};

/// Confidence multiplier for the robust score's one-sided lower bound (~95%, the
/// standard-normal z). A tunable knob: raise it to penalise uncertainty harder,
/// lower it toward the raw realized mean. See [`robust_score`].
const SCORE_Z: f64 = 1.64;

/// Streaming accumulator for one combo across every token it fired on. PnL stats
/// mark open positions to their last price (so a still-running token still
/// counts); holding-time stats and the robust [`score`](ComboMetrics::score) use
/// closed positions only.
///
/// **Bounded memory (O(1) per combo).** Median/p90 come from a fixed-size
/// [`QuantileSketch`] rather than a per-token `Vec`, so a combo's footprint no
/// longer grows with the number of tokens it fires on. At `HARD_MAX_COMBOS` ×
/// a few-thousand-token group the old `Vec<f32>`/`Vec<i32>` reached multi-GB held
/// in the fold thread; the sketch caps each combo at ~0.6 KB regardless of corpus
/// size — the cap that actually lets big sweeps run. The whole sweep allocates
/// one contiguous `vec![ComboAgg; n_combos]`, so this per-combo size is multiplied
/// by the (large) combo count × pool threads: keeping `ComboAgg` small is what
/// stops a high-`max_combos` run from OOM-aborting the process. Best/worst stay
/// **exact** (running min/max); mean/holding-mean stay exact (running sums); only
/// the two interior quantiles are approximate (~15% relative error, see
/// [`QuantileSketch`]).
#[derive(Clone)]
pub struct ComboAgg {
    fired: u64,
    open: u64,
    wins: u64,
    pnl_sol_sum: f64,
    gross_win_sol: f64,
    gross_loss_sol: f64,
    /// Σ pnl% over every fired token (mark-to-market for open) — exact mean.
    pnl_pct_sum: f64,
    /// Exact min/max pnl% over every fired token — `best`/`worst` stay precise.
    pnl_min: f32,
    pnl_max: f32,
    /// pnl% distribution of every fired token (mark-to-market for open) — for the
    /// approximate median/p90.
    pnl_sketch: QuantileSketch,
    /// Σ and Σ² of *closed* pnl% — the realized mean/stddev the score ranks on
    /// (open positions excluded: their pnl% is unrealized mark-to-market).
    closed_pct_sum: f64,
    closed_pct_sum_sq: f64,
    /// Σ holding seconds over closed positions — exact holding mean.
    holding_sum: i64,
    /// holding-seconds distribution of closed positions — for the median.
    holding_sketch: QuantileSketch,
    /// counts indexed by [`exit_index`].
    exit_counts: [u32; 8],
}

impl Default for ComboAgg {
    fn default() -> Self {
        Self {
            fired: 0,
            open: 0,
            wins: 0,
            pnl_sol_sum: 0.0,
            gross_win_sol: 0.0,
            gross_loss_sol: 0.0,
            pnl_pct_sum: 0.0,
            // Sentinels so the first recorded value wins min/max; an unfired combo
            // reports best/worst = 0 (handled in `finalize`).
            pnl_min: f32::INFINITY,
            pnl_max: f32::NEG_INFINITY,
            pnl_sketch: QuantileSketch::default(),
            closed_pct_sum: 0.0,
            closed_pct_sum_sq: 0.0,
            holding_sum: 0,
            holding_sketch: QuantileSketch::default(),
            exit_counts: [0; 8],
        }
    }
}

impl ComboAgg {
    /// Fold one token's outcome under this combo. No-entry rows are ignored.
    pub fn record(&mut self, o: &TokenOutcome) {
        if !o.fired {
            return;
        }
        self.fired += 1;
        self.pnl_sol_sum += o.pnl_sol as f64;
        self.pnl_pct_sum += o.pnl_percent as f64;
        self.pnl_min = self.pnl_min.min(o.pnl_percent);
        self.pnl_max = self.pnl_max.max(o.pnl_percent);
        self.pnl_sketch.record(o.pnl_percent as f64);
        if o.pnl_sol > 0.0 {
            self.wins += 1;
            self.gross_win_sol += o.pnl_sol as f64;
        } else if o.pnl_sol < 0.0 {
            self.gross_loss_sol += -(o.pnl_sol as f64);
        }
        if o.exit == ExitCode::Open {
            self.open += 1;
        } else {
            self.holding_sum += o.holding_secs;
            self.holding_sketch.record(o.holding_secs as f64);
            let p = o.pnl_percent as f64;
            self.closed_pct_sum += p;
            self.closed_pct_sum_sq += p * p;
        }
        self.exit_counts[exit_index(o.exit)] += 1;
    }

    /// Collapse to the final ranked row.
    pub fn finalize(self, combo_id: u32) -> ComboMetrics {
        let n = self.fired as f64;
        let (median_pnl_pct, p90_pnl_pct, best_pnl_pct, worst_pnl_pct) = if self.fired == 0 {
            (0.0, 0.0, 0.0, 0.0)
        } else {
            (
                self.pnl_sketch.quantile(0.5),
                self.pnl_sketch.quantile(0.9),
                self.pnl_max as f64,
                self.pnl_min as f64,
            )
        };
        let mean_pnl_pct = if self.fired == 0 { 0.0 } else { self.pnl_pct_sum / n };
        let n_closed = self.fired - self.open;
        let (avg_holding_secs, median_holding_secs) = if n_closed == 0 {
            (0.0, 0.0)
        } else {
            (
                self.holding_sum as f64 / n_closed as f64,
                self.holding_sketch.quantile(0.5),
            )
        };
        let profit_factor = if self.gross_loss_sol > 0.0 {
            Some(self.gross_win_sol / self.gross_loss_sol)
        } else {
            None // no losing trades → undefined (shown as ∞)
        };
        let (std_pnl_pct, score) =
            robust_score(n_closed, self.closed_pct_sum, self.closed_pct_sum_sq);
        ComboMetrics {
            combo_id,
            n_fired: self.fired,
            n_open: self.open,
            n_closed,
            win_rate: if self.fired == 0 { 0.0 } else { self.wins as f64 / n },
            total_pnl_sol: self.pnl_sol_sum,
            mean_pnl_pct,
            median_pnl_pct,
            p90_pnl_pct,
            best_pnl_pct,
            worst_pnl_pct,
            std_pnl_pct,
            profit_factor,
            score,
            expectancy_sol: if self.fired == 0 { 0.0 } else { self.pnl_sol_sum / n },
            avg_holding_secs,
            median_holding_secs,
            n_exit_take_profit: self.exit_counts[0],
            n_exit_stop_loss: self.exit_counts[1],
            n_exit_trailing: self.exit_counts[2],
            n_exit_stall: self.exit_counts[3],
            n_exit_time: self.exit_counts[4],
            n_exit_liquidity: self.exit_counts[5],
            n_exit_cohort: self.exit_counts[6],
            n_exit_open: self.exit_counts[7],
        }
    }
}

/// One ranked param-pair row: the combo's aggregated outcome across all tokens.
#[derive(Clone, Debug)]
pub struct ComboMetrics {
    pub combo_id: u32,
    pub n_fired: u64,
    pub n_open: u64,
    pub n_closed: u64,
    pub win_rate: f64,
    pub total_pnl_sol: f64,
    pub mean_pnl_pct: f64,
    pub median_pnl_pct: f64,
    pub p90_pnl_pct: f64,
    pub best_pnl_pct: f64,
    pub worst_pnl_pct: f64,
    /// Stddev of realized (closed) per-trade pnl% — the dispersion/risk term the
    /// `score` penalises. `0` when fewer than 2 closed trades.
    pub std_pnl_pct: f64,
    /// gross wins ÷ gross losses; `None` = no losing trades (infinite).
    pub profit_factor: Option<f64>,
    /// Robust rank: `μ − Z·σ/√n` over closed trades — the table's default sort.
    /// `None` when n_closed < 2 (no evidence of a repeatable edge → sorts last).
    pub score: Option<f64>,
    pub expectancy_sol: f64,
    pub avg_holding_secs: f64,
    pub median_holding_secs: f64,
    /// Per-exit-reason trade counts (how each combo's closed trades terminated),
    /// not param values. `n_` marks them as counts like `n_fired`/`n_closed`.
    pub n_exit_take_profit: u32,
    pub n_exit_stop_loss: u32,
    pub n_exit_trailing: u32,
    pub n_exit_stall: u32,
    pub n_exit_time: u32,
    pub n_exit_liquidity: u32,
    pub n_exit_cohort: u32,
    pub n_exit_open: u32,
}

/// One-sided lower-confidence bound on realized per-trade return:
/// `score = μ − Z·σ/√n` over *closed* positions — the sweep's ranking objective.
/// The `μ` term rewards a high mean return, `σ` penalises dispersion (a combo
/// that won big on a couple tokens and bled on the rest), and `√n` shrinks small
/// samples toward zero, so an edge proven over hundreds of tokens out-ranks a
/// lucky handful. Returns `(stddev, Some(score))`, or `(0, None)` when n < 2 —
/// one closed trade is no evidence of a repeatable edge, and `None` sorts last.
fn robust_score(n_closed: u64, sum: f64, sum_sq: f64) -> (f64, Option<f64>) {
    if n_closed < 2 {
        return (0.0, None);
    }
    let n = n_closed as f64;
    let mean = sum / n;
    // Sample variance (n−1 denominator); clamp tiny negatives from float
    // cancellation when every closed return is (near-)identical.
    let var = ((sum_sq - n * mean * mean) / (n - 1.0)).max(0.0);
    let std = var.sqrt();
    (std, Some(mean - SCORE_Z * std / n.sqrt()))
}

fn exit_index(e: ExitCode) -> usize {
    match e {
        ExitCode::TakeProfit => 0,
        ExitCode::StopLoss => 1,
        ExitCode::TrailingStop => 2,
        ExitCode::Stall => 3,
        ExitCode::TimeStop => 4,
        ExitCode::LiquidityExit => 5,
        ExitCode::CohortExit => 6,
        // Open + the two non-fired terminals (never recorded) bucket here.
        ExitCode::Open | ExitCode::NoEntry => 7,
    }
}

// --------------------------------------------------------------------------
// Quantile sketch
// --------------------------------------------------------------------------

/// Buckets per sign. With [`SKETCH_INV_LN_GAMMA`] this covers roughly `[1e-3, 3e5]`
/// in magnitude — wide enough for both pnl% (−100%..hundreds-of-thousands %) and
/// holding seconds (0..days). Values outside the range clamp into the end
/// buckets: their *rank* still counts correctly, only the returned value saturates.
///
/// Kept deliberately small (64, not 256): each bucket is a counter in the
/// per-combo [`ComboAgg`], and the whole sweep holds `n_combos` of those in one
/// contiguous `Vec`. 64 buckets per sign (vs. 256) cuts each sketch ~4×, the main
/// lever keeping a high-`max_combos` run from exhausting memory. The cost is a
/// coarser quantile (see the relative-error note on [`SKETCH_INV_LN_GAMMA`]).
const SKETCH_N: usize = 64;
/// Index offset so sub-unit magnitudes (e.g. 0.1% pnl, 1-second holds) land in
/// low buckets rather than clamping at 0. Chosen with [`SKETCH_INV_LN_GAMMA`] so
/// bucket 0 ≈ magnitude `1e-3` and the top bucket ≈ `3e5` — the same range the
/// old 256-bucket layout covered, just spread over 64 wider buckets.
const SKETCH_BIAS: f64 = 22.65;
/// `1 / ln(GAMMA)` for `GAMMA ≈ 1.357`. Each bucket is ~36% wider than the
/// previous, giving ~15% relative error `(GAMMA−1)/(GAMMA+1)` on a returned
/// quantile — coarser than the old 256-bucket sketch (~3.9%) in exchange for a 4×
/// smaller `ComboAgg`. Best/worst/mean/total stay exact; only median/p90 widen,
/// and the sweep ranks combos on `score`, not on these. Precomputed (no `ln` of a
/// constant in the hot fold path).
const SKETCH_INV_LN_GAMMA: f64 = 3.27885;

/// Fixed-memory, **order-independent** quantile sketch (DDSketch-style log
/// buckets). Replaces the old per-combo sample `Vec`s so each [`ComboAgg`] is
/// O(1) regardless of how many tokens it fires on (see [`ComboAgg`] for why that
/// matters at scale). Each value maps to a log-scaled bucket; the returned
/// quantile is the bucket's geometric midpoint (~3.9% relative error). Counts are
/// integers, so the estimate is **identical regardless of fold order** — grouped
/// and parallel folds reproduce the same median/p90, unlike order-sensitive
/// P²/t-digest. A two-sided layout (plus an exact zero count) handles the signed
/// pnl% domain; holding seconds simply never touch the negative buckets.
///
/// Counts are `u16` (saturating), not `u32`: a bucket only ever counts tokens that
/// fired into it (≤ the corpus size, a few thousand under the default caps), so the
/// 4-billion headroom of a `u32` was pure waste — halving the counter halves the
/// sketch again. A bucket that somehow exceeds 65 535 fires saturates rather than
/// wrapping (a wrap would corrupt the rank); at that point the quantile is already
/// only a coarse estimate, so the saturation is a negligible further approximation.
#[derive(Clone)]
struct QuantileSketch {
    /// Counts for v < 0, indexed by the bucket of `|v|`.
    neg: [u16; SKETCH_N],
    /// Counts for v > 0.
    pos: [u16; SKETCH_N],
    /// Count of exactly-zero values.
    zero: u16,
}

impl Default for QuantileSketch {
    fn default() -> Self {
        Self { neg: [0; SKETCH_N], pos: [0; SKETCH_N], zero: 0 }
    }
}

/// Bucket index for a strictly-positive magnitude, clamped into `[0, SKETCH_N)`.
fn sketch_bucket(mag: f64) -> usize {
    let idx = (mag.ln() * SKETCH_INV_LN_GAMMA + SKETCH_BIAS).floor();
    idx.clamp(0.0, (SKETCH_N - 1) as f64) as usize
}

/// Geometric midpoint value represented by bucket `i`.
fn sketch_value_at(i: usize) -> f64 {
    ((i as f64 - SKETCH_BIAS + 0.5) / SKETCH_INV_LN_GAMMA).exp()
}

impl QuantileSketch {
    fn record(&mut self, v: f64) {
        // Saturating: a u16 bucket that overflows must clamp, never wrap (a wrap
        // would corrupt the cumulative rank the quantile walk depends on).
        if v > 0.0 {
            let b = &mut self.pos[sketch_bucket(v)];
            *b = b.saturating_add(1);
        } else if v < 0.0 {
            let b = &mut self.neg[sketch_bucket(-v)];
            *b = b.saturating_add(1);
        } else {
            self.zero = self.zero.saturating_add(1);
        }
    }

    fn count(&self) -> u64 {
        let neg: u64 = self.neg.iter().map(|&c| c as u64).sum();
        let pos: u64 = self.pos.iter().map(|&c| c as u64).sum();
        neg + pos + self.zero as u64
    }

    /// Approximate `q`-quantile (`q ∈ [0,1]`) over all recorded values. `0.0` on
    /// an empty sketch. Walks buckets in ascending value order: most-negative
    /// (high `neg` index) → zero → most-positive.
    fn quantile(&self, q: f64) -> f64 {
        let total = self.count();
        if total == 0 {
            return 0.0;
        }
        let target = ((q * total as f64) as u64).min(total - 1);
        let mut cum = 0u64;
        // Negative side: larger |v| = more negative = smaller value first.
        for i in (0..SKETCH_N).rev() {
            cum += self.neg[i] as u64;
            if cum > target {
                return -sketch_value_at(i);
            }
        }
        cum += self.zero as u64;
        if cum > target {
            return 0.0;
        }
        for i in 0..SKETCH_N {
            cum += self.pos[i] as u64;
            if cum > target {
                return sketch_value_at(i);
            }
        }
        // Rank ≥ total only on rounding; fall back to the largest seen bucket.
        sketch_value_at(SKETCH_N - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(pnl_sol: f32, pnl_pct: f32, exit: ExitCode, holding: i64) -> TokenOutcome {
        TokenOutcome {
            fired: true,
            holding_secs: holding,
            pnl_percent: pnl_pct,
            pnl_sol,
            exit,
        }
    }

    #[test]
    fn aggregates_wins_losses_and_factor() {
        let mut a = ComboAgg::default();
        a.record(&outcome(2.0, 100.0, ExitCode::TakeProfit, 10));
        a.record(&outcome(-1.0, -50.0, ExitCode::StopLoss, 20));
        a.record(&TokenOutcome::no_entry()); // ignored
        let m = a.finalize(7);
        assert_eq!(m.combo_id, 7);
        assert_eq!(m.n_fired, 2);
        assert_eq!(m.n_closed, 2);
        assert!((m.win_rate - 0.5).abs() < 1e-9);
        assert!((m.total_pnl_sol - 1.0).abs() < 1e-9);
        assert_eq!(m.profit_factor, Some(2.0)); // 2.0 win / 1.0 loss
        assert_eq!(m.n_exit_take_profit, 1);
        assert_eq!(m.n_exit_stop_loss, 1);
    }

    #[test]
    fn no_losses_gives_infinite_factor() {
        let mut a = ComboAgg::default();
        a.record(&outcome(1.0, 10.0, ExitCode::TakeProfit, 5));
        assert_eq!(a.finalize(0).profit_factor, None);
    }

    #[test]
    fn score_none_under_two_closed_trades() {
        // One closed trade: a great return but no evidence it repeats → no score.
        let mut a = ComboAgg::default();
        a.record(&outcome(2.0, 200.0, ExitCode::TakeProfit, 10));
        let m = a.finalize(0);
        assert_eq!(m.n_closed, 1);
        assert_eq!(m.score, None);
        assert_eq!(m.std_pnl_pct, 0.0);
    }

    #[test]
    fn score_equals_mean_when_returns_are_identical() {
        // σ = 0 → the shrink term vanishes → score == realized mean.
        let mut a = ComboAgg::default();
        for _ in 0..3 {
            a.record(&outcome(0.1, 10.0, ExitCode::TakeProfit, 5));
        }
        let m = a.finalize(0);
        assert!(m.std_pnl_pct.abs() < 1e-9);
        assert_eq!(m.score, Some(10.0));
    }

    #[test]
    fn score_penalises_dispersion_below_the_mean() {
        // Same realized mean (0%), but a wide spread → σ>0 → score < mean.
        let mut a = ComboAgg::default();
        a.record(&outcome(1.0, 100.0, ExitCode::TakeProfit, 5));
        a.record(&outcome(-1.0, -100.0, ExitCode::StopLoss, 5));
        let m = a.finalize(0);
        assert!(m.std_pnl_pct > 0.0);
        assert!(m.score.unwrap() < 0.0, "dispersion pulls the lower bound under the mean");
    }

    #[test]
    fn open_positions_excluded_from_score() {
        // An open (unrealized) winner must not feed the realized mean/stddev.
        let mut a = ComboAgg::default();
        a.record(&outcome(0.1, 10.0, ExitCode::TakeProfit, 5));
        a.record(&outcome(0.1, 10.0, ExitCode::TakeProfit, 5));
        a.record(&outcome(5.0, 999.0, ExitCode::Open, 0)); // ignored by the score
        let m = a.finalize(0);
        assert_eq!(m.n_closed, 2);
        assert!(m.std_pnl_pct.abs() < 1e-9, "two identical closed returns → σ=0");
        assert_eq!(m.score, Some(10.0));
    }

    #[test]
    fn open_position_excluded_from_holding() {
        let mut a = ComboAgg::default();
        a.record(&outcome(0.5, 5.0, ExitCode::Open, 0));
        let m = a.finalize(0);
        assert_eq!(m.n_open, 1);
        assert_eq!(m.n_closed, 0);
        assert_eq!(m.avg_holding_secs, 0.0);
        assert_eq!(m.n_exit_open, 1);
    }

    #[test]
    fn best_worst_and_mean_are_exact() {
        // best/worst (running min/max) and mean (running sum) stay precise even
        // though median/p90 go through the approximate sketch.
        let mut a = ComboAgg::default();
        for v in [10.0, -30.0, 250.0, 5.0, -100.0] {
            a.record(&outcome(0.1, v, ExitCode::TakeProfit, 7));
        }
        let m = a.finalize(0);
        assert_eq!(m.best_pnl_pct, 250.0);
        assert_eq!(m.worst_pnl_pct, -100.0);
        // mean = (10 − 30 + 250 + 5 − 100) / 5 = 27
        assert!((m.mean_pnl_pct - 27.0).abs() < 1e-9);
    }

    #[test]
    fn sketch_median_p90_within_relative_error() {
        // 1..=1000 → exact median 500/501, p90 ~900. The 64-bucket log sketch must
        // land within its ~15% relative-error band on both (wider than the old
        // 256-bucket ~4% band, the trade for a 4× smaller per-combo footprint).
        let mut a = ComboAgg::default();
        for v in 1..=1000 {
            a.record(&outcome(0.1, v as f32, ExitCode::TakeProfit, v));
        }
        let m = a.finalize(0);
        assert!((m.median_pnl_pct - 500.0).abs() / 500.0 < 0.16, "median {}", m.median_pnl_pct);
        assert!((m.p90_pnl_pct - 900.0).abs() / 900.0 < 0.16, "p90 {}", m.p90_pnl_pct);
        // Holding fed the same 1..=1000 sample (closed) → median ~500.
        assert!(
            (m.median_holding_secs - 500.0).abs() / 500.0 < 0.16,
            "holding median {}",
            m.median_holding_secs
        );
    }

    #[test]
    fn sketch_handles_signed_and_zero() {
        // Half strongly negative, half strongly positive, one zero → median ≈ 0.
        let mut a = ComboAgg::default();
        for _ in 0..50 {
            a.record(&outcome(0.1, -200.0, ExitCode::StopLoss, 1));
        }
        a.record(&outcome(0.1, 0.0, ExitCode::TimeStop, 1));
        for _ in 0..50 {
            a.record(&outcome(0.1, 300.0, ExitCode::TakeProfit, 1));
        }
        let m = a.finalize(0);
        // The 51st-of-101 ordered value is the single 0.0 sample.
        assert_eq!(m.median_pnl_pct, 0.0);
        assert_eq!(m.best_pnl_pct, 300.0);
        assert_eq!(m.worst_pnl_pct, -200.0);
    }

    #[test]
    fn sketch_quantile_empty_is_zero() {
        let s = QuantileSketch::default();
        assert_eq!(s.quantile(0.5), 0.0);
        assert_eq!(s.count(), 0);
    }
}
