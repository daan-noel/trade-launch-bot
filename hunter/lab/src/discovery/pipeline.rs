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

use super::baseline::{select_baseline, BaselineGrid, BaselineSelection};
use super::candidates::ScreenConfig;
use super::family::{run_family_layer, FamilyLimits, FamilyReport};
use super::objective::DiscoveryWeights;
use super::screen::{run_screen, ScreenBaseline, ScreenReport, ScreenThresholds, Verdict};
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
    /// Used as-is when [`Self::baseline_grid`] holds fewer than two brackets.
    pub baseline: ScreenBaseline,
    /// Candidate brackets to measure before screening
    /// ([`baseline`](super::baseline)). With two or more, the winner **replaces**
    /// [`Self::baseline`] for every later layer; with fewer, the caller's baseline
    /// stands and no selection pass runs.
    pub baseline_grid: Option<BaselineGrid>,
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
            baseline_grid: None,
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
    /// Layer 0 — `None` when the caller named a single baseline and no brackets were
    /// measured.
    pub baseline_selection: Option<BaselineSelection>,
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
    /// Plain-language findings about the **run**, not about any one metric: which gate
    /// ran, whether the baseline was profitable, how much of the field failed for lack
    /// of data, how much statistical power the validate slice actually had.
    ///
    /// These are the questions a reader otherwise has to reverse-engineer out of a
    /// wall of drop reasons — and the ones that decide whether a shortlist means
    /// anything at all. Stated, never left to be inferred.
    pub diagnostics: Vec<String>,
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

    // Layer 0: pick the bracket that fits this cohort, on the *fit* slice only — a
    // baseline chosen with the validate slice in view would leak the held-out data
    // into every number Layer 1 produces.
    let baseline_selection = match &cfg.baseline_grid {
        Some(grid) if grid.brackets().len() > 1 => Some(select_baseline(
            &fit,
            &cfg.screen,
            grid,
            pricing,
            as_of,
            cfg.weights,
            observer,
        )?),
        _ => None,
    };
    let baseline = baseline_selection.as_ref().map_or(cfg.baseline, |s| s.chosen);
    if observer.cancelled() {
        anyhow::bail!("discovery pipeline cancelled");
    }

    let screen = run_screen(
        &fit,
        &cfg.screen,
        baseline,
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

    let diagnostics = diagnose(
        cohort_tokens,
        fit.token_count(),
        baseline_selection.as_ref(),
        &screen,
        &family,
        validation.as_ref(),
        cfg,
    );

    Ok(PipelineReport {
        cohort_tokens,
        fit_tokens: fit.token_count(),
        baseline_selection,
        screen,
        family,
        validation,
        no_validation,
        diagnostics,
    })
}

/// Turn the run's shape into the handful of sentences that decide how to read it.
///
/// Every entry answers a question the layer reports can only answer by arithmetic
/// across sections — "was the thing I lifted profitable", "which gate ran", "did the
/// held-out slice have the power to conclude anything". A run whose shortlist is empty
/// for a *structural* reason must say so here; silence would read as "no edge exists".
#[allow(clippy::too_many_arguments)]
fn diagnose(
    cohort_tokens: usize,
    fit_tokens: usize,
    selection: Option<&BaselineSelection>,
    screen: &ScreenReport,
    family: &FamilyReport,
    validation: Option<&ValidationReport>,
    cfg: &PipelineConfig,
) -> Vec<String> {
    let mut out = Vec::new();
    let bracket = |b: &ScreenBaseline| match (b.take_profit_pct, b.stop_loss_pct) {
        (Some(tp), Some(sl)) => format!("TP {tp}% / SL {sl}%"),
        (Some(tp), None) => format!("TP {tp}% (no stop)"),
        (None, Some(sl)) => format!("SL {sl}% (no take-profit)"),
        (None, None) => "no bracket".to_string(),
    };

    // ── the reference line ──────────────────────────────────────────────────
    if let Some(s) = selection {
        out.push(format!(
            "Measured {} bracket(s); screened against {} — the best-scoring one on the fit slice.",
            s.candidates.len(),
            bracket(&s.chosen),
        ));
        if s.all_unprofitable {
            out.push(
                "No bracket was profitable bare on this cohort. Every Keep below is a rescue \
                 from a losing baseline, not an improvement on a working one — re-check the \
                 cohort before trusting a shortlist."
                    .to_string(),
            );
        }
    }
    if let Some(b) = &screen.baseline_stats {
        let m = &b.metrics;
        out.push(format!(
            "Reference line ({}, no metric gate): {} fired / {} closed, {:.0}% win, median {:.1}%, {:.3} ◎.",
            bracket(&screen.baseline),
            m.n_fired,
            m.n_closed,
            m.win_rate * 100.0,
            m.median_pnl_pct,
            m.total_pnl_sol,
        ));
        if selection.is_none() && b.is_unprofitable() {
            out.push(format!(
                "The bare {} loses money here, so a metric can only ever be ranked on how much \
                 of that loss it removes. Supply a bracket menu to let the run pick a baseline \
                 that fits this cohort.",
                bracket(&screen.baseline),
            ));
        }
    }

    // ── the gate ────────────────────────────────────────────────────────────
    if screen.effective_min_closed < cfg.weights.min_closed {
        out.push(format!(
            "Cohort of {fit_tokens} fit token(s) cannot support the {}-closed gate — relaxed to \
             {}. Combos scored under it are discounted by sample size, not trusted equally.",
            cfg.weights.min_closed, screen.effective_min_closed,
        ));
    }

    // ── can this cohort measure a selective gate at all? ────────────────────
    // The scan opens at most one position per token, so `n_closed <= fit_tokens` and a
    // gate must fire on `gate / fit_tokens` of the slice merely to be *scored*. Past a
    // quarter, the selective end of every menu (the p75/p90 rungs — the ones most
    // likely to carry an edge) cannot clear the gate no matter what it screens, and
    // the whole run reads as `DropThin`. Indistinguishable from "no edge here" unless
    // the arithmetic is stated, so it is stated.
    let gate = screen.effective_min_closed;
    if fit_tokens > 0 && gate > 0 {
        let required = gate as f64 / fit_tokens as f64;
        if required > 0.25 {
            out.push(format!(
                "Fit slice of {fit_tokens} token(s) against a {gate}-closed gate: a gate must fire \
                 on {:.0}% of the slice just to be scored, so the selective end of every menu is \
                 unmeasurable on this cohort. Widen the time range or loosen the scope before \
                 reading any drop here as an absence of edge.",
                required * 100.0,
            ));
        }
    }

    // ── how the field died ──────────────────────────────────────────────────
    let thin = screen
        .responses
        .iter()
        .filter(|r| matches!(r.verdict, Verdict::DropThin { .. }))
        .count();
    let negative = screen
        .responses
        .iter()
        .filter(|r| matches!(r.verdict, Verdict::DropNegative { .. }))
        .count();
    let total = screen.responses.len();
    if thin > 0 {
        out.push(format!(
            "{thin} of {total} screened metric(s) never cleared the {}-closed gate — too few \
             trades to judge, not evidence of no edge. Widen the cohort to screen them.",
            screen.effective_min_closed,
        ));
    }
    if negative > 0 {
        out.push(format!(
            "{negative} metric(s) lifted the baseline but stayed unprofitable — real signal on a \
             bracket that does not fit. They are seeded as optional sweep axes."
        ));
    }
    if screen.shortlist.is_empty() && total > 0 {
        out.push(
            "Nothing was kept: no metric beat its own `off` pick by the lift threshold. Check the \
             reference line above before concluding the cohort has no structure."
                .to_string(),
        );
    }

    // ── the rescue ──────────────────────────────────────────────────────────
    let rescued = family.rescues.iter().filter(|r| r.verdict.is_keep()).count();
    if rescued > 0 {
        out.push(format!(
            "{rescued} metric(s) had no standalone edge but earned an axis once the strongest \
             winner was pinned — conditional edges, valid only alongside that pin."
        ));
    }

    // ── validation power ────────────────────────────────────────────────────
    if let Some(v) = validation {
        let gate = cfg.weights.effective_min_closed(v.validate_tokens as u64);
        let cant_tell = v
            .candidates
            .iter()
            .filter(|c| {
                matches!(
                    c.verdict,
                    super::validate::ValidationVerdict::ThinValidate { .. }
                        | super::validate::ValidationVerdict::NoFireValidate
                )
            })
            .count();
        out.push(format!(
            "Held out {} of {cohort_tokens} token(s); a candidate needs {gate} closed trade(s) \
             there to return a verdict at all.",
            v.validate_tokens,
        ));
        if cant_tell > 0 {
            out.push(format!(
                "{cant_tell} candidate(s) could not be judged out-of-sample — neither a pass nor \
                 a failure. Widen the cohort or lower the train split."
            ));
        }
    }
    out
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

    /// Layer 0 measures the bracket menu and the rest of the run screens against the
    /// winner — a mis-chosen bracket must not be able to read as "no metric has an
    /// edge here".
    #[test]
    fn baseline_grid_selects_the_bracket_layers_1_to_3_run_against() {
        let c = corpus(24);
        let cfg = PipelineConfig {
            baseline_grid: Some(BaselineGrid {
                take_profit_pct: vec![Some(20.0), Some(60.0)],
                stop_loss_pct: vec![Some(15.0), Some(40.0)],
            }),
            ..cfg()
        };
        let out = run_pipeline(&c, &cfg, pricing(), as_of(), &NoopObserver).unwrap();

        let sel = out.baseline_selection.as_ref().expect("a 4-bracket grid runs Layer 0");
        assert_eq!(sel.candidates.len(), 4);
        // The chosen bracket — not the config's — is what Layer 1 screened against.
        assert_eq!(out.screen.baseline, sel.chosen);
        assert_eq!(out.family.baseline, sel.chosen);
        // …and it is genuinely the best of the table, not just the first.
        let best = sel.candidates[sel.chosen_index].score().unwrap_or(f64::NEG_INFINITY);
        assert!(sel
            .candidates
            .iter()
            .all(|x| x.score().unwrap_or(f64::NEG_INFINITY) <= best));

        // A single-bracket grid is the caller naming a baseline: no pass, no override.
        let single = PipelineConfig {
            baseline_grid: Some(BaselineGrid::single(cfg.baseline)),
            ..cfg.clone()
        };
        let out = run_pipeline(&c, &single, pricing(), as_of(), &NoopObserver).unwrap();
        assert!(out.baseline_selection.is_none());
        assert_eq!(out.screen.baseline, cfg.baseline);
    }

    /// The gate that ran, the reference line, and why the field died are stated — a
    /// reader must never have to reverse-engineer them out of the drop table.
    #[test]
    fn diagnostics_state_the_gate_and_the_reference_line() {
        let c = corpus(24);
        // A cohort this small cannot afford the default 20-closed gate.
        let cfg = PipelineConfig { weights: DiscoveryWeights::default(), ..cfg() };
        let out = run_pipeline(&c, &cfg, pricing(), as_of(), &NoopObserver).unwrap();

        assert!(out.screen.effective_min_closed < cfg.weights.min_closed);
        let joined = out.diagnostics.join("\n");
        assert!(joined.contains("relaxed"), "the relaxed gate must be stated: {joined}");
        assert!(joined.contains("Reference line"), "the reference line must be stated: {joined}");
        // Nothing is asserted about *which* verdicts appear — only that a structurally
        // empty shortlist explains itself instead of reading as "no edge exists".
        if out.screen.shortlist.is_empty() && !out.screen.responses.is_empty() {
            assert!(joined.contains("Nothing was kept"), "{joined}");
        }
    }

    /// A cohort too small to score a selective gate must say so *with the
    /// arithmetic*. Otherwise `DropThin` across the whole field reads as "this cohort
    /// has no edge", when what it measured was "no gate here can clear the floor".
    #[test]
    fn a_cohort_too_small_for_the_gate_states_the_arithmetic() {
        let c = corpus(24);
        let cfg = PipelineConfig { weights: DiscoveryWeights::default(), ..cfg() };
        let out = run_pipeline(&c, &cfg, pricing(), as_of(), &NoopObserver).unwrap();

        // 24 tokens on the default 70/30 split leave ~16 to fit on, against a gate
        // that floors at 8 — half the slice.
        let required = out.screen.effective_min_closed as f64 / out.fit_tokens as f64;
        assert!(required > 0.25, "precondition: this cohort cannot afford its gate");
        let joined = out.diagnostics.join("\n");
        assert!(
            joined.contains("just to be scored"),
            "the cohort/gate arithmetic must be stated: {joined}",
        );
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
