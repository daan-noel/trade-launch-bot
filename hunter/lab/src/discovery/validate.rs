//! **Layer 3 — out-of-sample validation** (plan §4): the overfit verdict.
//!
//! Layers 1–2 *tune*. Everything they emit is, by construction, the best-looking
//! combo on the very tokens it was fitted to — which is exactly what an overfit
//! combo looks like too. Layer 3 is the only place that can tell them apart: re-score
//! each winner on tokens it was **never tuned on** and report the train→validate
//! delta. A big positive-to-nothing drop is the overfit signature, and now it is
//! visible per combo instead of discovered later with real SOL.
//!
//! ## The split
//!
//! [`SplitPolicy`] splits the cohort **by token age**, because that is the split the
//! data actually supports: the DB is being extended forward in time, so "earlier
//! tokens" is a real regime boundary rather than a random partition of one regime.
//! Both of decision **D4**'s options are the same call:
//!
//! * *time-split now* — [`split_tokens`] on the loaded cohort, fit on
//!   [`CorpusSplit::train`], validate on [`CorpusSplit::validate`];
//! * *wait-for-new-data cycle* — pass last cycle's cohort as `train` and a freshly
//!   loaded newer one as `validate`. [`validate_candidates`] takes two arbitrary token
//!   slices, so no extra machinery is needed for it.
//!
//! ## The re-score
//!
//! Both slices are re-simulated with [`simulate_one_combo`], the same primitive the
//! sweep drill-in uses, under the **run's own** [`Pricing`] and `as_of`. That pairing
//! is not optional: re-simulating under different pricing (or a later "now", which
//! turns `Open` positions into `Dead` ones) changes the very numbers being compared,
//! and the train→validate delta would then measure the re-run, not the combo (parity
//! plan B7 / step-4 finding (a)).
//!
//! The **train side is re-scored too**, never taken from the Layer-2 report: Layer 2
//! scored over the whole fitted cohort, so comparing that to a validate-slice score
//! would confound "held out" with "smaller sample". Re-scoring the train slice makes
//! the two sides like-for-like — same objective, same pricing, same `as_of`, differing
//! only in which tokens they saw.
//!
//! Per-token rows are folded with [`ComboMetrics::exact_from_rows`] (exact quantiles,
//! not the sweep's bounded sketch) — affordable here because Layer 3 scores a handful
//! of combos, and it keeps the median that anchors the objective free of sketch error
//! on both sides of the delta.

use anyhow::{bail, Result};
use chrono::{DateTime, Utc};
use rayon::prelude::*;
use serde_json::Value;

use crate::sweep::aggregate::ComboMetrics;
use crate::sweep::corpus::CorpusToken;
use crate::sweep::generic::Pricing;
use crate::sweep::progress::SweepObserver;
use crate::sweep::registry::simulate_one_combo;

use super::family::{FamilyReport, FamilyResult, JointResult};
use super::objective::{discovery_score, ComboStats, DiscoveryWeights, ScoreOutcome};

// ───────────────────────────── the split ───────────────────────────────────

/// How the cohort is cut into train / validate. Always by token **creation time** —
/// see the module docs for why a time split is the honest one here.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SplitPolicy {
    /// The earliest `fraction` of tokens (by `created_at`, ties broken by mint so the
    /// split is deterministic) train, the rest validate. `0.7` = 70/30.
    AgeFraction(f64),
    /// Tokens created **strictly before** this instant train, the rest validate. Use
    /// when the boundary is a known regime change rather than a share of the cohort.
    Boundary(DateTime<Utc>),
}

impl Default for SplitPolicy {
    /// 70/30: enough validation tokens to clear the min-N gate on a typical cohort
    /// while leaving the fit the bulk of the data. A D1-style seed — override per run.
    fn default() -> Self {
        SplitPolicy::AgeFraction(0.7)
    }
}

/// A cohort cut in two, plus the boundary that cut it.
#[derive(Clone)]
pub struct CorpusSplit {
    /// Earlier tokens — what Layers 1–2 fit on.
    pub train: Vec<CorpusToken>,
    /// Later tokens — never seen by the fit.
    pub validate: Vec<CorpusToken>,
    /// First `created_at` on the validate side (`None` when it is empty). The
    /// reported cut point, so a report says *where* the split fell, not just how big
    /// each side was.
    pub boundary: Option<DateTime<Utc>>,
}

impl CorpusSplit {
    /// True when either side is empty — nothing can be validated, and the caller must
    /// say so rather than report a vacuous "holds".
    pub fn is_degenerate(&self) -> bool {
        self.train.is_empty() || self.validate.is_empty()
    }
}

/// Cut `tokens` into train / validate by age.
///
/// Sorting is by `(created_at, mint)`: two tokens created in the same second must not
/// land on different sides depending on lake row order, or the same cohort would
/// validate differently on a re-run.
pub fn split_tokens(tokens: &[CorpusToken], policy: SplitPolicy) -> CorpusSplit {
    let mut ordered: Vec<CorpusToken> = tokens.to_vec();
    ordered.sort_by(|a, b| a.created_at.cmp(&b.created_at).then_with(|| a.mint.cmp(&b.mint)));

    let cut = match policy {
        SplitPolicy::AgeFraction(f) => {
            let f = f.clamp(0.0, 1.0);
            ((ordered.len() as f64) * f).round() as usize
        }
        SplitPolicy::Boundary(at) => ordered.partition_point(|t| t.created_at < at),
    };
    let cut = cut.min(ordered.len());
    let validate = ordered.split_off(cut);
    let boundary = validate.first().map(|t| t.created_at);
    CorpusSplit { train: ordered, validate, boundary }
}

// ───────────────────────────── candidates ──────────────────────────────────

/// One combo to validate: a canonical `RuleParams` JSON plus the label it is reported
/// under. [`BestCombo::params_json`](super::family::BestCombo::params_json) drops
/// straight in — Layer 2's winner needs no adapter.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub label: String,
    pub params_json: Value,
}

impl Candidate {
    /// The winner of one family's grid, labelled by its family. `None` when the grid
    /// produced no rankable combo.
    pub fn from_family(f: &FamilyResult) -> Option<Self> {
        let best = f.best.as_ref()?;
        Some(Self {
            label: format!("family:{}", f.family.as_str()),
            params_json: best.params_json.clone(),
        })
    }

    /// The winner of one joint-cluster grid. `None` when nothing was rankable.
    pub fn from_joint(j: &JointResult) -> Option<Self> {
        let best = j.best.as_ref()?;
        let names = j
            .families
            .iter()
            .map(|f| f.as_str())
            .collect::<Vec<_>>()
            .join("+");
        Some(Self {
            label: format!("joint:{names}"),
            params_json: best.params_json.clone(),
        })
    }
}

/// Every Layer-2 family winner **and** joint-cluster winner — the default Layer-3 input.
pub fn candidates_from_family_report(report: &FamilyReport) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = report.families.iter().filter_map(Candidate::from_family).collect();
    for j in &report.joints {
        if let Some(c) = Candidate::from_joint(j) {
            out.push(c);
        }
    }
    out
}

// ───────────────────────────── verdict tuning ──────────────────────────────

/// Thresholds the train→validate verdict is measured against. Seeded like the other
/// D1 constants: sane defaults, overridable per run, pinned in `docs/plans/` once
/// validated against a real cohort.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ValidationThresholds {
    /// Fraction of the train score the validate score must retain to count as
    /// [`ValidationVerdict::Holds`]. Deliberately well below 1.0: an out-of-sample
    /// score that merely *survives* is the signal, and demanding parity would reject
    /// every honest edge on sampling noise alone.
    pub min_retention: f64,
}

impl Default for ValidationThresholds {
    fn default() -> Self {
        Self { min_retention: 0.5 }
    }
}

/// What the held-out slice said about a combo.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ValidationVerdict {
    /// Kept its edge out-of-sample (`retention >= min_retention`).
    Holds { retention: f64 },
    /// Still positive out-of-sample, but materially weaker than in-sample.
    Degraded { retention: f64 },
    /// Positive in-sample, non-positive out-of-sample — the overfit signature.
    Failed { retention: f64 },
    /// The validate slice never cleared the min-N gate: too few closed trades to
    /// conclude anything. **Not** a pass and **not** a failure — widen the cohort or
    /// move the boundary.
    ThinValidate { n_closed: u64 },
    /// The combo never fired on the validate slice at all.
    NoFireValidate,
    /// The **train** side is itself unrankable, so there is no in-sample number to
    /// hold anything against.
    UnrankableTrain,
}

impl ValidationVerdict {
    /// True only for a combo that demonstrably survived out-of-sample. Everything
    /// else — including the two "can't tell" outcomes — is deliberately not a pass.
    pub fn is_pass(&self) -> bool {
        matches!(self, ValidationVerdict::Holds { .. })
    }

    /// `validate_score / train_score`, where that ratio is defined.
    pub fn retention(&self) -> Option<f64> {
        match *self {
            ValidationVerdict::Holds { retention }
            | ValidationVerdict::Degraded { retention }
            | ValidationVerdict::Failed { retention } => Some(retention),
            _ => None,
        }
    }
}

// ───────────────────────────── results ─────────────────────────────────────

/// One combo re-scored on one slice.
#[derive(Clone, Debug)]
pub struct SliceScore {
    /// Tokens the combo could have fired on — the `fire_rate` denominator.
    pub tokens: usize,
    pub outcome: ScoreOutcome,
    /// The full exact-quantile row behind the score, so a report can show *why* a
    /// verdict landed (fire count, win rate, median) and not just the scalar.
    pub metrics: ComboMetrics,
}

impl SliceScore {
    pub fn score(&self) -> Option<f64> {
        self.outcome.score()
    }
}

/// One candidate's train→validate comparison.
#[derive(Clone, Debug)]
pub struct CandidateValidation {
    pub label: String,
    pub params_json: Value,
    pub train: SliceScore,
    pub validate: SliceScore,
    pub verdict: ValidationVerdict,
}

/// The Layer-3 deliverable.
#[derive(Clone, Debug)]
pub struct ValidationReport {
    pub policy: SplitPolicy,
    pub thresholds: ValidationThresholds,
    pub train_tokens: usize,
    pub validate_tokens: usize,
    /// The min-N gate the **validate slice** was scored under, from its own token
    /// count ([`DiscoveryWeights::effective_min_closed`]). A held-out slice is smaller
    /// than the cohort by construction, so it carries a different — and always
    /// reported — gate: `ThinValidate` otherwise reads as a verdict on the candidate
    /// when it is a statement about the slice's size.
    pub effective_min_closed: u64,
    /// Where the cut fell (first validate-side `created_at`).
    pub boundary: Option<DateTime<Utc>>,
    /// One entry per candidate, in the order they were supplied.
    pub candidates: Vec<CandidateValidation>,
}

impl ValidationReport {
    /// The candidates that survived out-of-sample — what is actually worth promoting.
    pub fn survivors(&self) -> impl Iterator<Item = &CandidateValidation> {
        self.candidates.iter().filter(|c| c.verdict.is_pass())
    }
}

// ───────────────────────────── the driver ──────────────────────────────────

/// Re-score every candidate on both slices and classify the delta.
///
/// `pricing` / `as_of` **must** be the pair Layers 1–2 ran under (see the module
/// docs). Runs on the caller's rayon pool; cancellation is polled through `observer`
/// between units of work, and a cancelled run errors rather than returning a partial
/// report that reads like a complete one.
#[allow(clippy::too_many_arguments)]
pub fn validate_candidates(
    train: &[CorpusToken],
    validate: &[CorpusToken],
    candidates: &[Candidate],
    pricing: Pricing,
    as_of: DateTime<Utc>,
    weights: DiscoveryWeights,
    thresholds: ValidationThresholds,
    ix_patterns: Option<&[Vec<String>]>,
    policy: SplitPolicy,
    observer: &dyn SweepObserver,
) -> Result<ValidationReport> {
    let report = |candidates| ValidationReport {
        policy,
        thresholds,
        train_tokens: train.len(),
        validate_tokens: validate.len(),
        effective_min_closed: weights.effective_min_closed(validate.len() as u64),
        boundary: validate.first().map(|t| t.created_at),
        candidates,
    };
    if candidates.is_empty() {
        return Ok(report(Vec::new()));
    }
    if train.is_empty() || validate.is_empty() {
        bail!(
            "validation needs tokens on both sides of the split (train {}, validate {}) — \
             widen the cohort or move the boundary",
            train.len(),
            validate.len()
        );
    }

    // One unit = one (candidate, slice) re-simulation. Both slices of every candidate
    // are independent, so the whole layer is one flat parallel fan-out.
    let units: Vec<(usize, bool)> =
        (0..candidates.len()).flat_map(|i| [(i, true), (i, false)]).collect();
    observer.set_total(units.len(), 1);

    let scored: Vec<Result<(usize, bool, SliceScore)>> = units
        .par_iter()
        .map(|&(i, is_train)| {
            if observer.cancelled() {
                bail!("validation cancelled");
            }
            let tokens = if is_train { train } else { validate };
            let rows = simulate_one_combo(
                "generic",
                tokens,
                &candidates[i].params_json,
                pricing,
                as_of,
                ix_patterns,
            )?;
            let metrics = ComboMetrics::exact_from_rows(i as u32, &rows);
            let outcome =
                discovery_score(ComboStats::from_combo_metrics(&metrics), tokens.len() as u64, weights);
            observer.token_done(1);
            Ok((i, is_train, SliceScore { tokens: tokens.len(), outcome, metrics }))
        })
        .collect();

    let mut sides: Vec<(Option<SliceScore>, Option<SliceScore>)> =
        vec![(None, None); candidates.len()];
    for r in scored {
        let (i, is_train, s) = r?;
        if is_train {
            sides[i].0 = Some(s);
        } else {
            sides[i].1 = Some(s);
        }
    }

    let out = candidates
        .iter()
        .zip(sides)
        .map(|(c, (train_s, validate_s))| {
            let train_s = train_s.expect("every candidate scored its train slice");
            let validate_s = validate_s.expect("every candidate scored its validate slice");
            let verdict = classify(&train_s, &validate_s, thresholds);
            CandidateValidation {
                label: c.label.clone(),
                params_json: c.params_json.clone(),
                train: train_s,
                validate: validate_s,
                verdict,
            }
        })
        .collect();
    Ok(report(out))
}

/// Classify one train→validate pair.
fn classify(train: &SliceScore, validate: &SliceScore, th: ValidationThresholds) -> ValidationVerdict {
    // A non-positive in-sample score has no edge to hold: the objective is
    // multiplicative over a signed profit term, so ratios against it are meaningless
    // (see the objective's module docs).
    let Some(t) = train.score().filter(|s| *s > 0.0) else {
        return ValidationVerdict::UnrankableTrain;
    };
    let v = match validate.outcome {
        ScoreOutcome::Ranked(v) => v,
        ScoreOutcome::BelowMinClosed { n_closed, .. } => {
            return ValidationVerdict::ThinValidate { n_closed }
        }
        ScoreOutcome::NoFire => return ValidationVerdict::NoFireValidate,
    };
    let retention = v / t;
    if v <= 0.0 {
        ValidationVerdict::Failed { retention }
    } else if retention >= th.min_retention {
        ValidationVerdict::Holds { retention }
    } else {
        ValidationVerdict::Degraded { retention }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    use super::super::fixtures::{corpus, created_at, pricing, token};
    use crate::sweep::progress::NoopObserver;

    // `created_at` is used both to stagger token ages and to anchor a live `as_of`.

    /// A slice score with an empty metrics row — `classify` reads only the outcome.
    fn slice(tokens: usize, score: ScoreOutcome) -> SliceScore {
        SliceScore { tokens, outcome: score, metrics: ComboMetrics::exact_from_rows(0, &[]) }
    }

    /// The split is by age, deterministic, and reports where it cut.
    #[test]
    fn age_fraction_splits_earliest_first() {
        let mut tokens: Vec<CorpusToken> = (0..10).map(|i| token(&format!("m{i}"), 20)).collect();
        // Stagger creation times, then hand them in shuffled — the split must not care.
        for (i, t) in tokens.iter_mut().enumerate() {
            t.created_at = created_at() + Duration::seconds(i as i64 * 60);
        }
        tokens.reverse();

        let s = split_tokens(&tokens, SplitPolicy::AgeFraction(0.7));
        assert_eq!(s.train.len(), 7);
        assert_eq!(s.validate.len(), 3);
        assert!(!s.is_degenerate());
        // Every train token is older than every validate token — that is the whole
        // point of an out-of-sample split.
        let newest_train = s.train.iter().map(|t| t.created_at).max().unwrap();
        let oldest_validate = s.validate.iter().map(|t| t.created_at).min().unwrap();
        assert!(newest_train < oldest_validate);
        assert_eq!(s.boundary, Some(oldest_validate));
    }

    /// Same-second tokens must not land on different sides depending on load order.
    #[test]
    fn split_is_deterministic_under_ties() {
        let tokens: Vec<CorpusToken> = (0..8).map(|i| token(&format!("m{i}"), 20)).collect();
        let mut reversed = tokens.clone();
        reversed.reverse();
        let a = split_tokens(&tokens, SplitPolicy::AgeFraction(0.5));
        let b = split_tokens(&reversed, SplitPolicy::AgeFraction(0.5));
        let mints = |s: &CorpusSplit| -> Vec<String> { s.train.iter().map(|t| t.mint.clone()).collect() };
        assert_eq!(mints(&a), mints(&b));
    }

    #[test]
    fn boundary_split_cuts_at_the_instant() {
        let mut tokens: Vec<CorpusToken> = (0..6).map(|i| token(&format!("m{i}"), 20)).collect();
        for (i, t) in tokens.iter_mut().enumerate() {
            t.created_at = created_at() + Duration::seconds(i as i64 * 10);
        }
        let at = created_at() + Duration::seconds(25);
        let s = split_tokens(&tokens, SplitPolicy::Boundary(at));
        assert_eq!(s.train.len(), 3, "0/10/20s train");
        assert_eq!(s.validate.len(), 3, "30/40/50s validate");
        // A boundary past every token leaves nothing to validate — degenerate, and the
        // caller must see that rather than a vacuous pass.
        let past = split_tokens(&tokens, SplitPolicy::Boundary(created_at() + Duration::days(1)));
        assert!(past.is_degenerate());
        assert_eq!(past.boundary, None);
    }

    // ── the verdict ────────────────────────────────────────────────────────

    #[test]
    fn holds_degraded_and_failed_split_on_retention() {
        let th = ValidationThresholds::default(); // 0.5
        let train = slice(100, ScoreOutcome::Ranked(10.0));
        assert_eq!(
            classify(&train, &slice(50, ScoreOutcome::Ranked(8.0)), th),
            ValidationVerdict::Holds { retention: 0.8 }
        );
        assert_eq!(
            classify(&train, &slice(50, ScoreOutcome::Ranked(2.0)), th),
            ValidationVerdict::Degraded { retention: 0.2 }
        );
        // Positive in-sample, negative out-of-sample: the overfit signature.
        match classify(&train, &slice(50, ScoreOutcome::Ranked(-3.0)), th) {
            ValidationVerdict::Failed { retention } => assert!((retention + 0.3).abs() < 1e-9),
            other => panic!("expected a failure, got {other:?}"),
        }
        assert!(ValidationVerdict::Holds { retention: 0.8 }.is_pass());
        assert!(!ValidationVerdict::Degraded { retention: 0.2 }.is_pass());
    }

    /// "Can't tell" is its own outcome — never silently a pass, never a failure.
    #[test]
    fn thin_or_dead_validate_slice_is_inconclusive_not_a_pass() {
        let th = ValidationThresholds::default();
        let train = slice(100, ScoreOutcome::Ranked(10.0));
        assert_eq!(
            classify(&train, &slice(50, ScoreOutcome::BelowMinClosed { n_closed: 4, gate: 8 }), th),
            ValidationVerdict::ThinValidate { n_closed: 4 }
        );
        assert_eq!(
            classify(&train, &slice(50, ScoreOutcome::NoFire), th),
            ValidationVerdict::NoFireValidate
        );
        // An unrankable (or non-positive) train side has nothing to hold against.
        assert_eq!(
            classify(&slice(100, ScoreOutcome::NoFire), &slice(50, ScoreOutcome::Ranked(9.0)), th),
            ValidationVerdict::UnrankableTrain
        );
        assert_eq!(
            classify(
                &slice(100, ScoreOutcome::Ranked(-1.0)),
                &slice(50, ScoreOutcome::Ranked(9.0)),
                th
            ),
            ValidationVerdict::UnrankableTrain
        );
        for v in [
            ValidationVerdict::ThinValidate { n_closed: 4 },
            ValidationVerdict::NoFireValidate,
            ValidationVerdict::UnrankableTrain,
        ] {
            assert!(!v.is_pass());
            assert_eq!(v.retention(), None);
        }
    }

    // ── end-to-end ─────────────────────────────────────────────────────────

    /// Both slices are re-scored under one pricing/`as_of` pair, and a candidate's
    /// params round-trip so a survivor is promotable straight from the report.
    #[test]
    fn validates_a_candidate_on_both_slices() {
        let c = corpus(20);
        let split = split_tokens(&c.tokens, SplitPolicy::AgeFraction(0.7));
        let params = serde_json::json!({
            "take_profit": 30.0, "stop_loss": 15.0,
            "entry": { "m_state": { "time": [{ "operator": ">=", "value": 5.0 }] } }
        });
        hunter_engine::rule_params::RuleParams::parse(&params).expect("valid params");
        let cands = vec![Candidate { label: "test".into(), params_json: params }];

        // The fixture's tokens live in the synthetic-`created_at` past, so deadness
        // must be judged from an instant just past their trades — `Utc::now()` would
        // mark every one dead before entry (exactly the pairing the module warns about).
        let as_of = created_at() + chrono::Duration::seconds(120);
        let out = validate_candidates(
            &split.train,
            &split.validate,
            &cands,
            pricing(),
            as_of,
            DiscoveryWeights { min_closed: 1, ..DiscoveryWeights::default() },
            ValidationThresholds::default(),
            None,
            SplitPolicy::AgeFraction(0.7),
            &NoopObserver,
        )
        .unwrap();

        assert_eq!(out.candidates.len(), 1);
        assert_eq!(out.train_tokens, split.train.len());
        assert_eq!(out.validate_tokens, split.validate.len());
        let v = &out.candidates[0];
        // Each side's fire-rate denominator is its own slice, not the whole cohort.
        assert_eq!(v.train.tokens, split.train.len());
        assert_eq!(v.validate.tokens, split.validate.len());
        assert!(v.train.metrics.n_fired > 0, "the gate must fire somewhere in-sample");
        // Survivors are exactly the passing verdicts.
        assert_eq!(out.survivors().count(), usize::from(v.verdict.is_pass()));
    }

    /// No candidates is an empty report, not an error; a one-sided split IS an error
    /// (a "validation" with nothing held out would read as a pass).
    #[test]
    fn empty_candidates_ok_but_degenerate_split_errors() {
        let c = corpus(6);
        let split = split_tokens(&c.tokens, SplitPolicy::AgeFraction(0.5));
        let call = |cands: &[Candidate], train: &[CorpusToken], validate: &[CorpusToken]| {
            validate_candidates(
                train,
                validate,
                cands,
                pricing(),
                Utc::now(),
                DiscoveryWeights::default(),
                ValidationThresholds::default(),
                None,
                SplitPolicy::AgeFraction(0.5),
                &NoopObserver,
            )
        };
        let empty = call(&[], &split.train, &split.validate).unwrap();
        assert!(empty.candidates.is_empty());
        assert_eq!(empty.train_tokens, split.train.len());

        let cands = vec![Candidate { label: "x".into(), params_json: serde_json::json!({}) }];
        assert!(call(&cands, &split.train, &[]).is_err());
    }

    #[test]
    fn candidates_include_joint_winners() {
        use hunter_engine::metrics::MetricFamily;
        use super::super::family::{BestCombo, FamilyLimits, FamilyReport, FamilyResult, JointResult};
        use super::super::screen::ScreenBaseline;

        let best = BestCombo {
            idx: 0,
            picks: vec![Some(5.0)],
            score: 1.0,
            n_fired: 10,
            n_closed: 10,
            params_json: serde_json::json!({ "take_profit": 30.0 }),
        };
        let report = FamilyReport {
            cohort_tokens: 10,
            baseline: ScreenBaseline { take_profit_pct: Some(30.0), stop_loss_pct: Some(15.0) },
            limits: FamilyLimits::default(),
            families: vec![FamilyResult {
                family: MetricFamily::Price,
                members: vec![],
                dropped: vec![],
                combos: 1,
                best: Some(best.clone()),
                n_gated: 0,
            }],
            interactions: vec![],
            joints: vec![JointResult {
                families: vec![MetricFamily::Price, MetricFamily::Flow],
                members: vec![],
                dropped: vec![],
                combos: 4,
                best: Some(best),
                n_gated: 0,
            }],
            rescues: vec![],
            combos_scanned: 0,
        };
        let cands = candidates_from_family_report(&report);
        assert_eq!(cands.len(), 2);
        assert_eq!(cands[0].label, "family:price");
        assert_eq!(cands[1].label, "joint:price+flow");
    }
}
