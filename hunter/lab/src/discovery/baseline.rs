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

    // Best by score; an unrankable bracket can still win only if nothing ranked, in
    // which case the run is explicitly flagged rather than reading as a normal choice.
    let chosen_index = candidates
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| {
            let sa = a.score().unwrap_or(f64::NEG_INFINITY);
            let sb = b.score().unwrap_or(f64::NEG_INFINITY);
            sa.partial_cmp(&sb).unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(i, _)| i)
        .expect("non-empty");
    let all_unprofitable = candidates.iter().all(|c| c.score().is_none_or(|s| s <= 0.0));

    Ok(BaselineSelection {
        chosen: candidates[chosen_index].baseline,
        chosen_index,
        candidates,
        all_unprofitable,
        combos_scanned,
    })
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
        let best = out.candidates[out.chosen_index].score().unwrap_or(f64::NEG_INFINITY);
        for c in &out.candidates {
            assert!(c.score().unwrap_or(f64::NEG_INFINITY) <= best, "a better bracket was passed over");
        }
        assert_eq!(
            out.all_unprofitable,
            out.candidates.iter().all(|c| c.score().is_none_or(|s| s <= 0.0)),
        );
    }
}
