//! The **discovery objective** — a robust re-rank over a combo's already-persisted
//! metrics (plan §1).
//!
//! Why a re-rank and not a new kernel score: [`checklist_score`] is the SSOT the
//! live, paper, and sweep paths all fold through, so mutating it would silently move
//! *live* rankings. Instead the discovery objective is a **pure function of the
//! `ComboMetrics` columns** ([`ComboStats`]), applied after the fact. No kernel
//! change, no migration, and it ranks any stored run.
//!
//! It encodes the three goals the pipeline optimises — **profit (open-aware),
//! frequency, and stability (many small wins ≫ one whale)** — as:
//!
//! ```text
//! DiscoveryScore = robust_profit × fire_rate × win_component      (min-N gated)
//! ```
//!
//! * **`robust_profit`** — median-anchored, not mean, so a couple of whale winners
//!   can't inflate it (the exact failure the objective must avoid). Open positions
//!   are marked to last price and **haircut** so paper gains can't masquerade as
//!   realised edge.
//! * **`fire_rate`** = `n_fired / matched` — frequency; a combo that fires on 3 of
//!   500 tokens is penalised ~166× vs one that fires on all 500.
//! * **`win_component`** = `win_rate × clamp(profit_factor)` — win-rate alone rewards
//!   99 tiny wins + 1 ruinous loss; multiplying by a capped profit-factor demands the
//!   wins actually outweigh the losses.
//! * **min-N gate** — a combo with `n_closed < min_closed` is unrankable (its "edge"
//!   is noise). Reported, never silently dropped (see [`ScoreOutcome::is_gated`]).
//!
//! ## The gate is cohort-aware, and its edge is a ramp, not a cliff
//!
//! A fixed `min_closed = 20` is right for a five-figure corpus and wrong for a
//! single-fingerprint cohort: a regime-scoped run leaves tens-to-hundreds of tokens,
//! a selective gate fires on a third of them, and *every* pick lands under the cliff —
//! the whole screen then reads "no metric has an edge" when what actually happened is
//! "this cohort cannot support a 20-trade gate". Two changes, both in
//! [`DiscoveryWeights`]:
//!
//! * **[`DiscoveryWeights::effective_min_closed`]** scales the gate with the cohort
//!   (`min_closed_frac` of the tokens), clamped to `[min_closed_floor, min_closed]`.
//!   It can only ever *relax* the configured gate, never tighten it, and the effective
//!   value is reported so a reader knows which gate ran.
//! * **[`DiscoveryWeights::confidence`]** discounts a score by its sample size as a
//!   share of the full `min_closed`, so a 12-closed combo ranks *low* instead of
//!   vanishing, while anything at or above `min_closed` is untouched. On a corpus big
//!   enough that the two coincide the ramp is identically 1 — the historical behaviour.
//!
//! Note the ramp is applied on top of `fire_rate`, which already penalises selectivity
//! linearly; it is deliberately flat (=1) above `min_closed` so a healthy sample is
//! never double-charged for being selective.
//!
//! Like [`checklist_score`], the score is multiplicative over a signed profit term,
//! so the ordering among *negative* combos is not meaningful — the objective exists
//! to rank the profitable top, and callers filter to `> 0` before trusting a rank.
//!
//! [`checklist_score`]: trading_core::strategies::kernel::checklist_score

use crate::sweep::aggregate::ComboMetrics;

/// Tunable weights for [`discovery_score`]. Defaults are the plan's D1 seeds — set
/// to mirror the existing kernel constants where they overlap
/// (`open_haircut` = the kernel's `SCORE_OPEN_DRAG`, `win_rate_floor` its
/// `SCORE_WIN_RATE_FLOOR`) so the re-rank starts from the same intuitions and is
/// tuned against real cohorts, not in the abstract.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DiscoveryWeights {
    /// Minimum **closed** trades a combo needs to be scored at full confidence. The
    /// hard gate is [`Self::effective_min_closed`], which relaxes this on a small
    /// cohort; between the two the score is ramped by [`Self::confidence`].
    pub min_closed: u64,
    /// Hard floor under the cohort-scaled gate. Below this many closed trades a combo
    /// is unrankable no matter how small the cohort — a handful of trades is noise at
    /// any scale.
    pub min_closed_floor: u64,
    /// Share of the cohort's tokens the gate scales with. The effective gate is
    /// `clamp(round(frac × cohort_tokens), min_closed_floor, min_closed)`, so it can
    /// only ever relax `min_closed`, never tighten it.
    pub min_closed_frac: f64,
    /// Discount `∈ [0, 1]` on the mark-to-last-price contribution of still-open
    /// positions, so unrealised paper gains can't read as realised profit.
    pub open_haircut: f64,
    /// Ceiling on `profit_factor` inside `win_component`. A combo with no losses
    /// (`profit_factor = None`, i.e. ∞) is treated as exactly this cap, so an
    /// all-wins-so-far combo doesn't run away with the rank on a tiny sample.
    pub profit_factor_cap: f64,
    /// Floor under `win_rate` so an all-open / zero-win book keeps a tiny multiplier
    /// instead of zeroing the whole rank outright.
    pub win_rate_floor: f64,
}

impl Default for DiscoveryWeights {
    fn default() -> Self {
        Self {
            min_closed: 20,
            // 8 closed trades is the floor a median is worth reading at all; 5% of the
            // cohort is roughly "a gate that fires on one token in twenty still gets a
            // hearing". Both are D1 seeds — see the module docs.
            min_closed_floor: 8,
            min_closed_frac: 0.05,
            open_haircut: 0.5,
            profit_factor_cap: 3.0,
            win_rate_floor: 0.01,
        }
    }
}

impl DiscoveryWeights {
    /// The gate that actually runs on a cohort of `cohort_tokens` tokens: the
    /// configured [`Self::min_closed`], relaxed toward [`Self::min_closed_floor`] when
    /// the cohort is too small to support it. Never exceeds `min_closed`.
    pub fn effective_min_closed(&self, cohort_tokens: u64) -> u64 {
        let floor = self.min_closed_floor.min(self.min_closed);
        let scaled = (self.min_closed_frac.max(0.0) * cohort_tokens as f64).round();
        let scaled = if scaled.is_finite() { scaled as u64 } else { self.min_closed };
        scaled.clamp(floor, self.min_closed)
    }

    /// The small-sample discount applied to a rankable combo's score: sample size as a
    /// share of the full [`Self::min_closed`], capped at `1`. A combo scored under a
    /// relaxed gate is therefore ranked *below* an equally-profitable one measured on a
    /// full sample, instead of being erased or trusted equally.
    ///
    /// Identically `1` whenever the effective gate equals `min_closed` (any cohort big
    /// enough to afford the full gate), so this is a no-op on a large corpus.
    pub fn confidence(&self, n_closed: u64) -> f64 {
        if self.min_closed == 0 {
            return 1.0;
        }
        (n_closed as f64 / self.min_closed as f64).clamp(0.0, 1.0)
    }
}

/// The subset of a combo's persisted metrics the objective reads. Every field is a
/// column already stored on `ComboMetrics` / `GroupedSweepResult`, so building this
/// is a pure projection — no re-simulation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ComboStats {
    pub n_fired: u64,
    pub n_open: u64,
    pub n_closed: u64,
    /// Realised win-rate over closed trades (`0..=1`).
    pub win_rate: f64,
    /// Realised **mean** per-trade pnl% (closed only). Used only to recover the
    /// open-mark mean; never the profit center itself.
    pub mean_pnl_pct: f64,
    /// Realised **median** per-trade pnl% (closed only) — the stability-robust
    /// profit center.
    pub median_pnl_pct: f64,
    /// Mean per-trade pnl% over **all fired** positions, still-open marks included.
    pub mtm_pnl_pct: f64,
    /// Gross wins ÷ gross losses; `None` = no losing trades (infinite).
    pub profit_factor: Option<f64>,
}

impl ComboStats {
    /// Project a flat-sweep [`ComboMetrics`] row into the objective's inputs.
    pub fn from_combo_metrics(m: &ComboMetrics) -> Self {
        Self {
            n_fired: m.n_fired,
            n_open: m.n_open,
            n_closed: m.n_closed,
            win_rate: m.win_rate,
            mean_pnl_pct: m.mean_pnl_pct,
            median_pnl_pct: m.median_pnl_pct,
            mtm_pnl_pct: m.mtm_pnl_pct,
            profit_factor: m.profit_factor,
        }
    }

    /// Mean mark over just the **open** positions, recovered from the persisted
    /// sums — no extra column needed:
    ///
    /// ```text
    /// mtm_pnl_pct · n_fired  = closed_pct_sum + open_pct_sum   (fired sum)
    /// mean_pnl_pct · n_closed = closed_pct_sum                 (closed sum)
    /// ⇒ open_pct_sum = mtm·n_fired − mean·n_closed
    /// ⇒ mean-over-opens = open_pct_sum / n_open
    /// ```
    ///
    /// `0.0` when nothing is open (the caller multiplies it by `open_share = 0`).
    fn mean_open_pct(&self) -> f64 {
        if self.n_open == 0 {
            return 0.0;
        }
        let fired = self.n_fired as f64;
        let closed = self.n_closed as f64;
        let open = self.n_open as f64;
        (self.mtm_pnl_pct * fired - self.mean_pnl_pct * closed) / open
    }
}

/// The outcome of scoring one combo. Distinguishes an unrankable combo *because it
/// never fired* from one gated *by the min-N rule*, so the pipeline can report "N
/// combos killed by the min-closed gate" without conflating it with dead combos
/// (plan §1.2 — no silent caps).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScoreOutcome {
    /// A rankable combo and its discovery score (may be negative — see module doc).
    Ranked(f64),
    /// Fired, but fewer closed trades than the cohort's effective gate
    /// ([`DiscoveryWeights::effective_min_closed`]) — noise, not ranked. Carries the
    /// gate it failed so a report can say *which* gate ran, not just that one did.
    BelowMinClosed { n_closed: u64, gate: u64 },
    /// Never fired (or the group matched no tokens) — nothing to rank.
    NoFire,
}

impl ScoreOutcome {
    /// The score if rankable, else `None` (gated or no-fire).
    pub fn score(self) -> Option<f64> {
        match self {
            ScoreOutcome::Ranked(s) => Some(s),
            _ => None,
        }
    }

    /// True only for a combo killed by the min-N gate (not a no-fire). Lets the
    /// caller tally how many candidates the gate removed.
    pub fn is_gated(self) -> bool {
        matches!(self, ScoreOutcome::BelowMinClosed { .. })
    }
}

/// Score one combo against the discovery objective (plan §1).
///
/// `matched` is the number of tokens the combo could have fired on (the group's
/// token count) — the `fire_rate` denominator. Returns a [`ScoreOutcome`] so the
/// caller can separate ranked / gated / no-fire.
pub fn discovery_score(s: ComboStats, matched: u64, w: DiscoveryWeights) -> ScoreOutcome {
    if s.n_fired == 0 || matched == 0 {
        return ScoreOutcome::NoFire;
    }
    // Min-N gate — the anti-overfit backbone. An edge on a handful of closed trades
    // is noise no matter how large its profit%. The gate scales down with the cohort
    // (a regime-scoped run cannot afford the corpus-wide gate), and the shortfall
    // between it and `min_closed` is charged as a confidence discount below rather
    // than waved through.
    let gate = w.effective_min_closed(matched);
    if s.n_closed < gate {
        return ScoreOutcome::BelowMinClosed { n_closed: s.n_closed, gate };
    }

    let fired = s.n_fired as f64;
    let closed = s.n_closed as f64;
    let open = s.n_open as f64;
    let closed_share = closed / fired;
    let open_share = open / fired;

    // Median-anchored profit, open positions marked-to-last and haircut so paper
    // gains can't masquerade as realised edge.
    let robust_profit =
        s.median_pnl_pct * closed_share + s.mean_open_pct() * open_share * w.open_haircut;

    // Frequency.
    let fire_rate = (fired / matched as f64).min(1.0);

    // Consistency: win-rate scaled by how decisively wins outweigh losses. A `None`
    // profit_factor (no losses yet) is treated as the cap, not ∞.
    let pf = s
        .profit_factor
        .map(|p| p.min(w.profit_factor_cap))
        .unwrap_or(w.profit_factor_cap);
    let win_component = s.win_rate.max(w.win_rate_floor) * (pf / w.profit_factor_cap);

    // Small-sample discount — the soft half of the gate (module docs). A no-op once
    // the sample clears `min_closed`.
    let confidence = w.confidence(s.n_closed);

    ScoreOutcome::Ranked(robust_profit * fire_rate * win_component * confidence)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A closed-only combo (no open positions) with a given median / mean / win-rate.
    fn closed(n_closed: u64, win_rate: f64, median: f64, mean: f64, pf: Option<f64>) -> ComboStats {
        ComboStats {
            n_fired: n_closed,
            n_open: 0,
            n_closed,
            win_rate,
            mean_pnl_pct: mean,
            median_pnl_pct: median,
            mtm_pnl_pct: mean, // no opens ⇒ mtm == closed mean
            profit_factor: pf,
        }
    }

    #[test]
    fn no_fire_when_nothing_fired_or_no_match() {
        let s = closed(0, 0.0, 0.0, 0.0, None);
        assert_eq!(discovery_score(s, 100, DiscoveryWeights::default()), ScoreOutcome::NoFire);
        // Fired, but the group matched nothing → still no rank (fire_rate undefined).
        let fired = closed(30, 0.5, 5.0, 5.0, Some(2.0));
        assert_eq!(discovery_score(fired, 0, DiscoveryWeights::default()), ScoreOutcome::NoFire);
    }

    #[test]
    fn min_n_gate_kills_thin_combos() {
        let w = DiscoveryWeights::default(); // min_closed 20, floor 8
        // A 100-token cohort cannot afford the full gate: 5% ⇒ 5, clamped up to the
        // floor of 8. Below the floor is still noise at any cohort size.
        assert_eq!(w.effective_min_closed(100), 8);
        let thin = closed(7, 1.0, 500.0, 500.0, None);
        let out = discovery_score(thin, 100, w);
        assert_eq!(out, ScoreOutcome::BelowMinClosed { n_closed: 7, gate: 8 });
        assert!(out.is_gated());
        assert_eq!(out.score(), None);
        // One more closed trade clears the gate.
        let ok = closed(8, 1.0, 5.0, 5.0, Some(2.0));
        assert!(matches!(discovery_score(ok, 100, w), ScoreOutcome::Ranked(_)));
    }

    /// The gate scales with the cohort but can only ever *relax* `min_closed`: a
    /// fingerprint-scoped run must not read as "no metric has an edge" purely because
    /// the corpus-wide gate is unaffordable there.
    #[test]
    fn gate_scales_with_the_cohort_and_never_tightens() {
        let w = DiscoveryWeights::default();
        assert_eq!(w.effective_min_closed(0), 8, "an empty cohort still floors at 8");
        assert_eq!(w.effective_min_closed(60), 8, "5% of 60 is under the floor");
        assert_eq!(w.effective_min_closed(240), 12, "5% of 240");
        assert_eq!(w.effective_min_closed(400), 20, "5% of 400 reaches the full gate");
        assert_eq!(w.effective_min_closed(100_000), 20, "never above min_closed");
    }

    /// A combo measured under a relaxed gate is discounted, not trusted equally — and
    /// a full sample is never charged for it.
    #[test]
    fn confidence_discounts_small_samples_and_is_a_no_op_at_full_size() {
        let w = DiscoveryWeights::default();
        assert!((w.confidence(20) - 1.0).abs() < 1e-9);
        assert!((w.confidence(200) - 1.0).abs() < 1e-9, "capped at 1, never a bonus");
        assert!((w.confidence(10) - 0.5).abs() < 1e-9);

        // Same book, half the closed sample ⇒ half the score.
        let full = discovery_score(closed(20, 1.0, 10.0, 10.0, None), 100, w).score().unwrap();
        let half = discovery_score(closed(10, 1.0, 10.0, 10.0, None), 100, w).score().unwrap();
        // fire_rate also halves (10 vs 20 fired of 100), so the confidence ramp is the
        // *second* factor of two.
        assert!((full - 4.0 * half).abs() < 1e-9, "{full} vs {half}");
    }

    #[test]
    fn median_not_mean_drives_profit_so_whales_dont_win() {
        // Combo A: +5% on every one of 40 tokens — steady.
        // Combo B: two +400% whales dragging a mean up, but median is −20%.
        let w = DiscoveryWeights::default();
        let matched = 40;
        let steady = closed(40, 1.0, 5.0, 5.0, None);
        // B: same fire count + a *higher mean*, but a negative median and a mediocre
        // win-rate — the "one or two big profits" the objective must NOT reward.
        let whales = ComboStats {
            n_fired: 40,
            n_open: 0,
            n_closed: 40,
            win_rate: 0.10,
            mean_pnl_pct: 40.0,   // inflated by the whales
            median_pnl_pct: -20.0, // most tokens lost
            mtm_pnl_pct: 40.0,
            profit_factor: Some(1.2),
        };
        let a = discovery_score(steady, matched, w).score().unwrap();
        let b = discovery_score(whales, matched, w).score().unwrap();
        assert!(a > 0.0, "steady combo should score positive: {a}");
        assert!(b < 0.0, "whale combo has a negative median ⇒ negative profit center: {b}");
        assert!(a > b, "median-anchored objective must rank steady over whales ({a} vs {b})");
    }

    #[test]
    fn fire_rate_scales_the_score_linearly() {
        // Same book, half the coverage → half the score (mirrors checklist_score).
        let w = DiscoveryWeights::default();
        let s = closed(50, 1.0, 10.0, 10.0, None);
        let full = discovery_score(s, 50, w).score().unwrap();
        let half = discovery_score(s, 100, w).score().unwrap();
        assert!((full - 2.0 * half).abs() < 1e-9, "half coverage ⇒ half score: {full} vs {half}");
    }

    #[test]
    fn open_positions_are_haircut_not_counted_full() {
        // A combo with 20 closed at +10% and 20 open marked at +100%. The open marks
        // must be discounted by open_haircut (0.5) and blended by share, never
        // counted at face value.
        let w = DiscoveryWeights::default();
        let closed_sum = 10.0 * 20.0;
        let open_sum = 100.0 * 20.0;
        let s = ComboStats {
            n_fired: 40,
            n_open: 20,
            n_closed: 20,
            win_rate: 1.0,
            mean_pnl_pct: 10.0,
            median_pnl_pct: 10.0,
            mtm_pnl_pct: (closed_sum + open_sum) / 40.0, // 55.0
            profit_factor: None,
        };
        // mean_open_pct recovers to 100; robust_profit = 10·0.5 + 100·0.5·0.5 = 30.
        // win_component = 1.0 · (cap/cap) = 1.0 ; fire_rate = 40/40 = 1.
        let got = discovery_score(s, 40, w).score().unwrap();
        assert!((got - 30.0).abs() < 1e-6, "expected 30 (opens haircut), got {got}");
        // Sanity: recovered open mean is exactly the marked value.
        assert!((s.mean_open_pct() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn profit_factor_scales_win_component_and_caps_at_infinity() {
        let w = DiscoveryWeights::default(); // cap 3.0
        // pf == cap ⇒ full win_component; pf below cap ⇒ scaled down.
        let strong = closed(30, 0.5, 10.0, 10.0, Some(3.0));
        let weak = closed(30, 0.5, 10.0, 10.0, Some(1.5));
        let infinite = closed(30, 0.5, 10.0, 10.0, None); // no losses → treated as cap
        let strong_s = discovery_score(strong, 30, w).score().unwrap();
        let weak_s = discovery_score(weak, 30, w).score().unwrap();
        let inf_s = discovery_score(infinite, 30, w).score().unwrap();
        assert!(strong_s > weak_s, "higher profit factor ⇒ higher score");
        assert!((strong_s - inf_s).abs() < 1e-9, "None (∞) profit factor is treated as the cap");
        // weak is exactly half of strong (1.5/3.0 vs 3.0/3.0).
        assert!((weak_s - 0.5 * strong_s).abs() < 1e-9);
    }
}
