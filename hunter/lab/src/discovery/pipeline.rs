//! The **pipeline orchestrator** (plan §8, step 6): runs Layer 1 → Layer 2 →
//! Layer 3 over **one loaded corpus** and returns one [`PipelineReport`] carrying
//! all three sub-reports.
//!
//! ## Why the split comes first
//!
//! The honest shape is *fit on train, validate on held-out* — so the corpus is
//! [`split`](super::validate::split_tokens) **before** the fit, and Layers 1–2 see
//! only the train slice. Fitting on the whole cohort and then "validating" on a piece
//! of it would leak: the validate tokens would have already moved the shortlist and
//! the family grids that produced the very combos being validated. Splitting first is
//! the one thing that makes the train→validate delta mean anything (validate module
//! docs / plan §4).
//!
//! When the cohort is too small to spare a validate slice ([`SplitPolicy`] leaves one
//! side empty), the pipeline still runs Layers 1–2 over the whole cohort and reports
//! `validation: None` with a reason — a fit with no overfit verdict, never a silent
//! pass.
//!
//! ## One corpus load, one pricing identity
//!
//! Everything downstream reuses the passed-in [`Corpus`] (the handler loads the lake
//! once, plan §6) and the run's own [`Pricing`] + `as_of`. That pair threads
//! unchanged through the screen, the family grids, and the validation re-sim, because
//! two of those numbers computed under different pricing are not comparable (parity
//! plan B7). The pipeline never re-derives "now".

use anyhow::Result;
use chrono::{DateTime, Utc};

use crate::sweep::corpus::Corpus;
use crate::sweep::generic::Pricing;
use crate::sweep::progress::SweepObserver;

use super::candidates::ScreenConfig;
use super::family::{run_family_layer, FamilyLimits, FamilyReport};
use super::objective::DiscoveryWeights;
use super::screen::{run_screen, ScreenBaseline, ScreenReport, ScreenThresholds};
use super::validate::{
    candidates_from_family_report, split_tokens, validate_candidates, SplitPolicy,
    ValidationReport, ValidationThresholds,
};

/// Everything a full pipeline run needs beyond the corpus itself. Groups the four
/// layers' knobs so a caller (handler / test) sets them once.
#[derive(Clone, Debug)]
pub struct PipelineConfig {
    /// Candidate-generation + screen-plan knobs (windows, flow patterns, sampling).
    pub screen: ScreenConfig,
    /// The fixed TP/SL every metric is screened against — part of the run's identity.
    pub baseline: ScreenBaseline,
    /// Objective constants (D1), shared by all three layers so a combo is scored the
    /// same everywhere it appears.
    pub weights: DiscoveryWeights,
    /// Keep/drop thresholds for the Layer-1 verdict.
    pub screen_thresholds: ScreenThresholds,
    /// Bounds on each Layer-2 family grid.
    pub family_limits: FamilyLimits,
    /// How the cohort is cut for out-of-sample validation.
    pub split: SplitPolicy,
    /// Retention thresholds for the Layer-3 verdict.
    pub validation_thresholds: ValidationThresholds,
    /// The run's raw `volume_ix_patterns` (label sequences), if any. Kept beside the
    /// compiled `cfg.screen.flow_patterns` because the compiled set is a one-way hash
    /// with no reverse, and Layer 3's `simulate_one_combo` re-derives the classifier
    /// from these sequences. `None` ⇒ no flow gating, exactly like the screen.
    pub flow_label_sequences: Option<Vec<Vec<String>>>,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            screen: ScreenConfig::default(),
            // No `Default` on `ScreenBaseline` by design — pick the canonical dip
            // scalper exit as the pipeline's seed. Callers override per run.
            baseline: ScreenBaseline { take_profit_pct: Some(30.0), stop_loss_pct: Some(15.0) },
            weights: DiscoveryWeights::default(),
            screen_thresholds: ScreenThresholds::default(),
            family_limits: FamilyLimits::default(),
            split: SplitPolicy::default(),
            validation_thresholds: ValidationThresholds::default(),
            flow_label_sequences: None,
        }
    }
}

/// Why the pipeline produced no out-of-sample verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoValidationReason {
    /// The split left the train or validate slice empty (cohort too small for the
    /// chosen policy) — Layers 1–2 ran over the whole cohort instead.
    DegenerateSplit,
    /// Layer 2 crowned no promotable combo, so there was nothing to validate.
    NoCandidates,
}

/// The full three-layer deliverable.
#[derive(Clone, Debug)]
pub struct PipelineReport {
    /// Tokens loaded for the run (both split sides).
    pub cohort_tokens: usize,
    /// Tokens Layers 1–2 fit on (the train slice, or the whole cohort when the split
    /// was degenerate).
    pub fit_tokens: usize,
    /// Layer 1.
    pub screen: ScreenReport,
    /// Layer 2.
    pub family: FamilyReport,
    /// Layer 3 — `None` when there was nothing to validate (see [`no_validation`]).
    ///
    /// [`no_validation`]: Self::no_validation
    pub validation: Option<ValidationReport>,
    /// Set exactly when `validation` is `None`.
    pub no_validation: Option<NoValidationReason>,
}

/// Run the whole pipeline over one loaded, already-scoped corpus.
///
/// `pricing` / `as_of` are the run's identity and thread through every layer
/// unchanged. Runs synchronously on the caller's rayon pool; wrap it in
/// `spawn_blocking` + a bounded pool exactly like `sweep_generic` does. Cancellation
/// is polled through `observer` inside each layer's sweep.
pub fn run_pipeline(
    corpus: &Corpus,
    cfg: &PipelineConfig,
    pricing: Pricing,
    as_of: DateTime<Utc>,
    observer: &dyn SweepObserver,
) -> Result<PipelineReport> {
    let cohort_tokens = corpus.token_count();
    let split = split_tokens(&corpus.tokens, cfg.split);

    // Fit on the train slice; fall back to the whole cohort when the split can't
    // spare a held-out side (a small cohort still deserves a fit, just no verdict).
    let degenerate = split.is_degenerate();
    let fit: Corpus = if degenerate {
        borrow_corpus(corpus, corpus.tokens.clone())
    } else {
        borrow_corpus(corpus, split.train.clone())
    };

    let screen = run_screen(
        &fit,
        &cfg.screen,
        cfg.baseline,
        pricing,
        as_of,
        cfg.weights,
        cfg.screen_thresholds,
        observer,
    )?;
    if observer.cancelled() {
        anyhow::bail!("discovery pipeline cancelled");
    }

    let family = run_family_layer(
        &fit,
        &cfg.screen,
        &screen,
        cfg.family_limits,
        pricing,
        as_of,
        cfg.weights,
        observer,
    )?;
    if observer.cancelled() {
        anyhow::bail!("discovery pipeline cancelled");
    }

    // Layer 3: validate the family winners on the held-out slice.
    let candidates = candidates_from_family_report(&family);
    let (validation, no_validation) = if degenerate {
        (None, Some(NoValidationReason::DegenerateSplit))
    } else if candidates.is_empty() {
        (None, Some(NoValidationReason::NoCandidates))
    } else {
        let report = validate_candidates(
            &split.train,
            &split.validate,
            &candidates,
            pricing,
            as_of,
            cfg.weights,
            cfg.validation_thresholds,
            cfg.flow_label_sequences.as_deref(),
            cfg.split,
            observer,
        )?;
        (Some(report), None)
    };

    Ok(PipelineReport {
        cohort_tokens,
        fit_tokens: fit.token_count(),
        screen,
        family,
        validation,
        no_validation,
    })
}

/// A sub-corpus over `tokens` that carries the parent's identity flags. Keeps the
/// hash suffixed so a warm-cache key can't confuse a split slice for the whole.
fn borrow_corpus(parent: &Corpus, tokens: Vec<crate::sweep::corpus::CorpusToken>) -> Corpus {
    Corpus {
        tokens,
        hash: format!("{}::pipeline", parent.hash),
        has_fingerprints: parent.has_fingerprints,
        candidates_capped: parent.candidates_capped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::fixtures::{corpus, created_at, pricing};
    use super::super::validate::ValidationVerdict;
    use crate::sweep::progress::NoopObserver;

    fn cfg() -> PipelineConfig {
        PipelineConfig {
            // The synthetic cohort clears the min-N gate only at 1 closed trade.
            weights: DiscoveryWeights { min_closed: 1, ..DiscoveryWeights::default() },
            ..PipelineConfig::default()
        }
    }

    fn as_of() -> DateTime<Utc> {
        created_at() + chrono::Duration::seconds(120)
    }

    /// The full pipeline runs end-to-end on one corpus: all three layers present, the
    /// fit ran on the train slice, and validation ran on the held-out slice.
    #[test]
    fn runs_all_three_layers_over_one_corpus() {
        let c = corpus(24);
        let out = run_pipeline(&c, &cfg(), pricing(), as_of(), &NoopObserver).unwrap();

        assert_eq!(out.cohort_tokens, 24);
        // 70/30 default split: the fit sees the earlier 70%.
        assert!(out.fit_tokens < out.cohort_tokens, "the validate slice is held out of the fit");
        assert_eq!(out.screen.cohort_tokens, out.fit_tokens, "Layer 1 fit on the train slice");
        // Layer 3 ran (there was a held-out slice); every verdict is one of the enum's.
        match (&out.validation, out.no_validation) {
            (Some(v), None) => {
                assert_eq!(v.train_tokens + v.validate_tokens, out.cohort_tokens);
                for cand in &v.candidates {
                    // A promoted winner round-trips to a rule straight from the report.
                    hunter_engine::rule_params::RuleParams::parse(&cand.params_json)
                        .expect("validated candidate is promotable");
                    // Every verdict is well-formed (the enum has no "unset").
                    let _: ValidationVerdict = cand.verdict;
                }
            }
            // No family winner is a legitimate outcome on a toy cohort — but then the
            // reason must say so, never a silent empty.
            (None, Some(NoValidationReason::NoCandidates)) => {
                assert!(candidates_from_family_report(&out.family).is_empty());
            }
            other => panic!("unexpected validation state: {other:?}"),
        }
    }

    /// Too small to split → Layers 1–2 still run on the whole cohort, and the report
    /// says why there is no verdict rather than fabricating one.
    #[test]
    fn degenerate_split_fits_whole_cohort_and_reports_no_verdict() {
        let c = corpus(2);
        let cfg = PipelineConfig {
            split: SplitPolicy::AgeFraction(1.0), // everything trains, nothing validates
            ..cfg()
        };
        let out = run_pipeline(&c, &cfg, pricing(), as_of(), &NoopObserver).unwrap();
        assert_eq!(out.fit_tokens, out.cohort_tokens, "the whole cohort trains");
        assert!(out.validation.is_none());
        assert_eq!(out.no_validation, Some(NoValidationReason::DegenerateSplit));
    }
}
