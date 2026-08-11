//! **Layer 0 — baseline selection**: measure the bare TP/SL brackets on the cohort
//! and screen against the one that actually works there.
//!
//! ## Why this layer exists
//!
//! Layer 1 ranks every metric by what it adds *over a fixed bracket*
//! ([`ScreenBaseline`]), and [`classify`](super::screen::classify) refuses any pick
//! that is still a losing combo. Both are right. Together they mean a single wrong
//! bracket poisons the entire run: if bare TP 30 / SL 15 loses money on this cohort,
//! nearly every metric lands in a drop bucket regardless of the signal it carries, and
//! the report reads "no metric has an edge here" when what it measured was "this
//! bracket does not fit this regime".
//!
//! That failure is invisible from the output — which is exactly why it must not be
//! left to the caller to guess the bracket. This layer measures the candidates
//! instead: one combo per `(tp, sl)` pair, all in a single additive pass over one
//! shared per-token precompute, scored with the same objective as everything
//! downstream. The winner becomes the run's baseline; the whole table is reported so
//! the choice is auditable and a reader can see how much the bracket mattered.
//!
//! ## What it is not
//!
//! Not a TP/SL sweep. The grid here is deliberately coarse and its winner is a
//! *reference line*, not a recommendation — the sweep seed still expands TP/SL onto
//! the canonical ladders around it ([`seed`](super::seed)). And the baseline remains
//! part of a screen's identity: two runs that selected different brackets are not
//! comparable and their shortlists must not be merged.

use anyhow::{bail, Result};
use hunter_engine::metrics::Ts;

use crate::sweep::aggregate::ComboMetrics;
use crate::sweep::corpus::Corpus;
use crate::sweep::generic::axes::{AxesModel, AxesRequest, AxisSpec};
use crate::sweep::generic::Pricing;
use crate::sweep::progress::SweepObserver;

use super::additive::AdditiveStrategy;
use super::candidates::ScreenConfig;
use super::objective::{discovery_score, ComboStats, DiscoveryWeights, ScoreOutcome};
use super::screen::ScreenBaseline;

/// The brackets to measure. Each menu may carry `None` ("omit this guard"), so a
/// TP-only or SL-only bracket is expressible — but a pair with neither is not a
/// bracket at all and is rejected up front.
#[derive(Clone, Debug, PartialEq)]
pub struct BaselineGrid {
    pub take_profit_pct: Vec<Option<f64>>,
    pub stop_loss_pct: Vec<Option<f64>>,
}

impl BaselineGrid {
    /// A grid of exactly one bracket — the "caller named the baseline" case, which
    /// needs no measurement pass.
    pub fn single(baseline: ScreenBaseline) -> Self {
        Self {
            take_profit_pct: vec![baseline.take_profit_pct],
            stop_loss_pct: vec![baseline.stop_loss_pct],
        }
    }

    /// Every valid bracket in the grid, in menu order. A `(None, None)` pair is
    /// skipped rather than erroring: it is a hole in the cross-product, not a request.
    pub fn brackets(&self) -> Vec<ScreenBaseline> {
        let tps = if self.take_profit_pct.is_empty() { &[None][..] } else { &self.take_profit_pct };
        let sls = if self.stop_loss_pct.is_empty() { &[None][..] } else { &self.stop_loss_pct };
        let mut out = Vec::with_capacity(tps.len() * sls.len());
        for tp in tps {
            for sl in sls {
                let b = ScreenBaseline { take_profit_pct: *tp, stop_loss_pct: *sl };
                if b.take_profit_pct.is_none() && b.stop_loss_pct.is_none() {
                    continue;
                }
                out.push(b);
            }
        }
        out
    }
}

/// One measured bracket.
#[derive(Clone, Debug)]
pub struct BaselineCandidate {
    pub baseline: ScreenBaseline,
    pub outcome: ScoreOutcome,
    /// The full row, so the table shows money and not just a score.
    pub metrics: ComboMetrics,
}

impl BaselineCandidate {
    pub fn score(&self) -> Option<f64> {
        self.outcome.score()
    }
}

/// The selection outcome: which bracket won, and what every candidate scored.
#[derive(Clone, Debug)]
pub struct BaselineSelection {
    /// The bracket Layers 1–3 run against.
    pub chosen: ScreenBaseline,
    /// Index of [`Self::chosen`] within [`Self::candidates`].
    pub chosen_index: usize,
    /// Every bracket measured, in grid order.
    pub candidates: Vec<BaselineCandidate>,
    /// True when **no** bracket scored positive — the run proceeds against the least
    /// bad one, and every downstream `Keep` is a rescue from a losing baseline. Never
    /// silently swallowed: a losing reference line changes what the whole report means.
    pub all_unprofitable: bool,
    /// Combos measured (one per bracket) — this layer's honest budget.
    pub combos_scanned: usize,
}

/// Measure every bracket in `grid` on `corpus` and pick the best-scoring one.
///
/// One [`AdditiveStrategy`] segment per bracket, each a single-combo model, so the
/// whole layer costs one shared precompute pass and `|grid|` folds. `pricing` /
/// `as_of` are the run's identity and must be the pair every later layer uses — a
/// bracket chosen under different pricing is a bracket chosen for a different run.
pub fn select_baseline(
    corpus: &Corpus,
    cfg: &ScreenConfig,
    grid: &BaselineGrid,
    pricing: Pricing,
    as_of: Ts,
    weights: DiscoveryWeights,
    observer: &dyn SweepObserver,
) -> Result<BaselineSelection> {
    let brackets = grid.brackets();
    if brackets.is_empty() {
        bail!("baseline grid contains no bracket with a take-profit or a stop-loss");
    }

    let models: Vec<AxesModel> = brackets
        .iter()
        .map(bracket_model)
        .collect::<Result<_, String>>()
        .map_err(|e| anyhow::anyhow!("baseline grid axes: {e}"))?;
    let strategy = AdditiveStrategy::new(models, pricing, as_of, cfg.flow_patterns.as_ref());
    let combos_scanned = strategy.combos().len();
    let rows = strategy.run(corpus, observer)?;

    let matched = corpus.token_count() as u64;
    let candidates: Vec<BaselineCandidate> = brackets
        .iter()
        .zip(&rows)
        .filter_map(|(baseline, rows)| {
            let m = rows.first()?;
            Some(BaselineCandidate {
                baseline: *baseline,
                outcome: discovery_score(ComboStats::from_combo_metrics(m), matched, weights),
                metrics: m.clone(),
            })
        })
        .collect();
    if candidates.is_empty() {
        bail!("baseline selection measured no bracket — the grid resolved to an empty scan");
    }

    let (chosen_index, all_unprofitable) =
        choose_bracket(candidates.iter().map(|c| (c.score(), c.metrics.total_pnl_sol)));

    Ok(BaselineSelection {
        chosen: candidates[chosen_index].baseline,
        chosen_index,
        candidates,
        all_unprofitable,
        combos_scanned,
    })
}

/// Pick the winning bracket from `(score, realised ◎)` pairs, returning its index
/// and whether the choice had to fall back to money.
///
/// The rank runs over the **positive** scores only. The objective is multiplicative
/// over a signed profit term, so below zero a larger `fire_rate` drives the score
/// *further* negative and a bare `max_by` crowns whichever bracket barely trades —
/// the "ordering among negative combos is not meaningful" caveat in
/// [`objective`](super::objective), which this is the caller-side half of.
///
/// With nothing positive there is no meaningful rank to take at all, so the fallback
/// is realised money: the least-bad bracket in ◎. That is monotone in the quantity a
/// reader actually cares about, and the returned flag says the run took it.
fn choose_bracket(rows: impl Iterator<Item = (Option<f64>, f64)>) -> (usize, bool) {
    let rows: Vec<(Option<f64>, f64)> = rows.collect();
    let all_unprofitable = rows.iter().all(|(s, _)| s.is_none_or(|s| s <= 0.0));
    let key = |(score, pnl_sol): &(Option<f64>, f64)| {
        if all_unprofitable {
            *pnl_sol
        } else {
            score.filter(|s| *s > 0.0).unwrap_or(f64::NEG_INFINITY)
        }
    };
    let idx = rows
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| key(a).partial_cmp(&key(b)).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i)
        .unwrap_or(0);
    (idx, all_unprofitable)
}

/// One bracket as a single-combo model: its TP/SL axes and nothing else.
fn bracket_model(b: &ScreenBaseline) -> Result<AxesModel, String> {
    let mut axes: Vec<AxisSpec> = Vec::with_capacity(2);
    let mut push = |kind: &str, v: Option<f64>| {
        if let Some(v) = v {
            axes.push(AxisSpec {
                kind: kind.to_string(),
                side: None,
                group: None,
                metric: None,
                operator: None,
                window: None,
                values: vec![Some(v)],
            });
        }
    };
    push("take_profit", b.take_profit_pct);
    push("stop_loss", b.stop_loss_pct);
    AxesModel::resolve(&AxesRequest { axes })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    use super::super::fixtures::{corpus, pricing};
    use crate::sweep::progress::NoopObserver;

    fn weights() -> DiscoveryWeights {
        DiscoveryWeights { min_closed: 1, ..DiscoveryWeights::default() }
    }

    #[test]
    fn brackets_cross_the_menus_and_skip_the_empty_pair() {
        let grid = BaselineGrid {
            take_profit_pct: vec![None, Some(30.0), Some(60.0)],
            stop_loss_pct: vec![None, Some(15.0)],
        };
        let got = grid.brackets();
        // 3 × 2 = 6 pairs, minus the (None, None) hole.
        assert_eq!(got.len(), 5);
        assert!(!got
            .iter()
            .any(|b| b.take_profit_pct.is_none() && b.stop_loss_pct.is_none()));
        // A single-bracket grid is the identity case.
        let one = ScreenBaseline { take_profit_pct: Some(30.0), stop_loss_pct: Some(15.0) };
        assert_eq!(BaselineGrid::single(one).brackets(), vec![one]);
    }

    #[test]
    fn an_all_none_grid_is_refused_not_silently_empty() {
        let grid = BaselineGrid { take_profit_pct: vec![None], stop_loss_pct: vec![None] };
        let err = select_baseline(
            &corpus(4),
            &ScreenConfig::default(),
            &grid,
            pricing(),
            Utc::now(),
            weights(),
            &NoopObserver,
        )
        .unwrap_err();
        assert!(err.to_string().contains("no bracket"), "{err}");
    }

    /// The layer measures every bracket, crowns the best-scoring one, and reports the
    /// whole table — the choice must be auditable, not just asserted.
    #[test]
    fn measures_every_bracket_and_picks_the_best_scoring() {
        let grid = BaselineGrid {
            take_profit_pct: vec![Some(20.0), Some(60.0)],
            stop_loss_pct: vec![Some(15.0), Some(40.0)],
        };
        let out = select_baseline(
            &corpus(12),
            &ScreenConfig::default(),
            &grid,
            pricing(),
            Utc::now(),
            weights(),
            &NoopObserver,
        )
        .unwrap();

        assert_eq!(out.candidates.len(), 4);
        assert_eq!(out.combos_scanned, 4, "one combo per bracket — nothing multiplicative");
        assert_eq!(out.chosen, out.candidates[out.chosen_index].baseline);
        assert_eq!(
            out.all_unprofitable,
            out.candidates.iter().all(|c| c.score().is_none_or(|s| s <= 0.0)),
        );
        let winner = &out.candidates[out.chosen_index];
        if out.all_unprofitable {
            // No rankable bracket ⇒ the pick is by realised money, not by score.
            for c in &out.candidates {
                assert!(
                    c.metrics.total_pnl_sol <= winner.metrics.total_pnl_sol,
                    "a bracket that lost less was passed over",
                );
            }
        } else {
            let best = winner.score().expect("a profitable winner is rankable");
            assert!(best > 0.0, "a rankable winner must be positive, got {best}");
            for c in &out.candidates {
                assert!(
                    c.score().unwrap_or(f64::NEG_INFINITY) <= best,
                    "a better bracket was passed over",
                );
            }
        }
    }

    /// The inverted-ranking guard. Among losing brackets the objective scores a
    /// *higher* fire rate *lower*, so a bare `max_by` crowns whichever bracket barely
    /// trades. With nothing positive the choice must fall back to realised money.
    #[test]
    fn a_losing_grid_is_chosen_by_money_not_by_an_inverted_score() {
        // A bracket that traded a lot for −2 ◎ vs one that barely traded for −8 ◎.
        // Below zero the score prefers the second (closer to zero); money prefers the
        // first, and money is the one that means something.
        let barely_trades = (Some(-0.4_f64), -8.0_f64);
        let trades_a_lot = (Some(-9.0_f64), -2.0_f64);
        assert!(
            barely_trades.0 > trades_a_lot.0,
            "precondition: the raw score ranking is inverted below zero",
        );
        let (idx, fell_back) = choose_bracket([barely_trades, trades_a_lot].into_iter());
        assert!(fell_back, "an all-losing grid must report the fallback");
        assert_eq!(idx, 1, "the least-bad bracket by money wins, not the least-traded");
    }

    /// Above zero the rank is the score, and an unrankable (gated) bracket never wins
    /// over a rankable one just by being unmeasured.
    #[test]
    fn a_profitable_grid_ranks_by_score_and_ignores_gated_brackets() {
        let rows = [
            (None, 99.0),        // gated: huge ◎ but unrankable
            (Some(2.0), 1.0),
            (Some(5.0), 0.5),    // best score, less money — the score is the rank
            (Some(-3.0), 0.2),
        ];
        let (idx, fell_back) = choose_bracket(rows.into_iter());
        assert!(!fell_back, "a positive score exists, so no money fallback");
        assert_eq!(idx, 2);
    }
}
