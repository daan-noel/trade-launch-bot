//! Serializable wire shape for a [`PipelineReport`](super::pipeline::PipelineReport).
//!
//! The layer reports are rich Rust types (registry enums, `ScoreOutcome`, verdicts)
//! that the frontend can't consume directly, so this module flattens them into a
//! single JSON tree of plain strings + numbers — the analogue of the sweep's
//! `GroupedSweepResult` row. Nothing here recomputes: every field is read straight off
//! the in-memory report, so the DTO can never disagree with what the pipeline scored.
//!
//! Vocabulary matches the rest of the redesign wire contract: metric/group names are
//! the registry's `name`s, sides are `entry`/`exit`, operators are their JSON symbols
//! (`>=`, `<`), and every verdict is a stable tag string the frontend switches on.

use serde::Serialize;

use hunter_engine::metrics::group_spec;

use super::baseline::{BaselineCandidate, BaselineSelection};
use super::family::{
    BestCombo, DropReason, FamilyReport, FamilyResult, Interaction, InteractionVerdict,
    JointResult, Rescue,
};
use super::objective::ScoreOutcome;
use super::pipeline::{NoValidationReason, PipelineReport};
use super::screen::{
    BaselineStats, MetricResponse, ResponsePoint, ScreenBaseline, ScreenReport, Verdict,
};
use super::seed::{SeedCluster, SweepSeed};
use super::validate::{
    CandidateValidation, SliceScore, ValidationReport, ValidationVerdict,
};

// ───────────────────────────── top level ───────────────────────────────────

/// The whole pipeline result, as one JSON object.
#[derive(Debug, Serialize)]
pub struct PipelineDto {
    pub cohort_tokens: usize,
    pub fit_tokens: usize,
    /// Whether the cohort hit the run's `token_cap` — set by the handler, which is the
    /// only layer that knows the cap. A truncated cohort silently scores "the newest N
    /// tokens" instead of the range that was asked for, so it can never be a flag the
    /// reader has to go looking for.
    pub cohort_capped: bool,
    /// The cap that produced [`Self::cohort_capped`] (handler-set).
    pub token_cap: Option<usize>,
    /// Layer 0 — the measured bracket table, absent when the caller named one baseline.
    pub baseline_selection: Option<BaselineSelectionDto>,
    /// Run-level findings in plain language (see `PipelineReport::diagnostics`).
    pub diagnostics: Vec<String>,
    pub screen: ScreenDto,
    pub family: FamilyDto,
    pub validation: Option<ValidationDto>,
    /// Present exactly when `validation` is `null`.
    pub no_validation: Option<String>,
    /// Discovery → grouped-sweep handoff (`AxisSpec[]` ready for the generic form).
    pub sweep_seed: SweepSeedDto,
}

impl PipelineDto {
    pub fn from_report(r: &PipelineReport) -> Self {
        let seed = super::seed::build_sweep_seed(&r.screen, &r.family);
        Self {
            cohort_tokens: r.cohort_tokens,
            fit_tokens: r.fit_tokens,
            cohort_capped: false,
            token_cap: None,
            baseline_selection: r.baseline_selection.as_ref().map(BaselineSelectionDto::from),
            diagnostics: r.diagnostics.clone(),
            screen: ScreenDto::from_report(&r.screen),
            family: FamilyDto::from_report(&r.family),
            validation: r.validation.as_ref().map(ValidationDto::from_report),
            no_validation: r.no_validation.map(no_validation_tag),
            sweep_seed: SweepSeedDto::from(&seed),
        }
    }
}

// ───────────────────────────── Layer 0 ─────────────────────────────────────

/// A TP/SL bracket on the wire. `null` on either side means that guard is omitted.
#[derive(Debug, Serialize, Clone, Copy)]
pub struct BracketDto {
    pub take_profit_pct: Option<f64>,
    pub stop_loss_pct: Option<f64>,
}

impl From<&ScreenBaseline> for BracketDto {
    fn from(b: &ScreenBaseline) -> Self {
        Self { take_profit_pct: b.take_profit_pct, stop_loss_pct: b.stop_loss_pct }
    }
}

/// One scored row, in score **and** in money. The objective is unitless, so a report
/// that shows only the score cannot be checked against anything.
#[derive(Debug, Serialize)]
pub struct ScoredRowDto {
    pub score: Option<f64>,
    /// One of `ranked` | `below_min_closed` | `no_fire`.
    pub outcome: String,
    pub n_fired: u64,
    pub n_closed: u64,
    pub win_rate: f64,
    pub median_pnl_pct: f64,
    pub total_pnl_sol: f64,
}

impl ScoredRowDto {
    fn new(outcome: ScoreOutcome, m: &crate::sweep::aggregate::ComboMetrics) -> Self {
        Self {
            score: outcome.score(),
            outcome: outcome_tag(outcome),
            n_fired: m.n_fired,
            n_closed: m.n_closed,
            win_rate: m.win_rate,
            median_pnl_pct: m.median_pnl_pct,
            total_pnl_sol: m.total_pnl_sol,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct BaselineSelectionDto {
    pub chosen: BracketDto,
    pub chosen_index: usize,
    /// True when nothing measured positive — every downstream Keep is a rescue from a
    /// losing baseline.
    pub all_unprofitable: bool,
    pub combos_scanned: usize,
    pub candidates: Vec<BaselineCandidateDto>,
}

impl From<&BaselineSelection> for BaselineSelectionDto {
    fn from(s: &BaselineSelection) -> Self {
        Self {
            chosen: BracketDto::from(&s.chosen),
            chosen_index: s.chosen_index,
            all_unprofitable: s.all_unprofitable,
            combos_scanned: s.combos_scanned,
            candidates: s.candidates.iter().map(BaselineCandidateDto::from).collect(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct BaselineCandidateDto {
    pub bracket: BracketDto,
    #[serde(flatten)]
    pub row: ScoredRowDto,
}

impl From<&BaselineCandidate> for BaselineCandidateDto {
    fn from(c: &BaselineCandidate) -> Self {
        Self {
            bracket: BracketDto::from(&c.baseline),
            row: ScoredRowDto::new(c.outcome, &c.metrics),
        }
    }
}

fn no_validation_tag(r: NoValidationReason) -> String {
    match r {
        NoValidationReason::DegenerateSplit => "degenerate_split",
        NoValidationReason::NoCandidates => "no_candidates",
    }
    .to_string()
}

// ───────────────────────────── Layer 1 ─────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ScreenDto {
    pub cohort_tokens: usize,
    pub combos_scanned: usize,
    pub n_gated: usize,
    /// The bracket every `lift` on this page is measured against.
    pub baseline: BracketDto,
    /// What that bracket did bare — the reference line. `null` only when nothing was
    /// screened.
    pub baseline_stats: Option<ScoredRowDto>,
    /// The min-N gate that actually ran, after cohort scaling.
    pub effective_min_closed: u64,
    /// The configured gate the effective one was relaxed from (equal when it wasn't).
    pub min_closed: u64,
    /// The kept metrics, best lift first — the shortlist.
    pub shortlist: Vec<MetricResponseDto>,
    /// Every screened metric (kept or dropped), in plan order, so the UI can show the
    /// full field with reasons.
    pub responses: Vec<MetricResponseDto>,
    /// Registry metrics excluded up front, with the reason.
    pub skipped: Vec<SkippedDto>,
    /// Screened metrics that produced no usable menu.
    pub gaps: Vec<MenuGapDto>,
}

impl ScreenDto {
    fn from_report(r: &ScreenReport) -> Self {
        Self {
            cohort_tokens: r.cohort_tokens,
            combos_scanned: r.combos_scanned,
            n_gated: r.n_gated,
            baseline: BracketDto::from(&r.baseline),
            baseline_stats: r.baseline_stats.as_ref().map(|b: &BaselineStats| {
                ScoredRowDto::new(b.outcome, &b.metrics)
            }),
            effective_min_closed: r.effective_min_closed,
            min_closed: r.weights.min_closed,
            shortlist: r.shortlisted().map(MetricResponseDto::from).collect(),
            responses: r.responses.iter().map(MetricResponseDto::from).collect(),
            skipped: r
                .skipped
                .iter()
                .map(|s| SkippedDto {
                    side: side_str(s.side),
                    group: group_spec(s.group).name.to_string(),
                    metric: s.metric.name().to_string(),
                    reason: format!("{:?}", s.reason),
                })
                .collect(),
            gaps: r
                .gaps
                .iter()
                .map(|(m, gap)| MenuGapDto {
                    side: side_str(m.side),
                    group: group_spec(m.group).name.to_string(),
                    metric: m.metric.name().to_string(),
                    reason: format!("{gap:?}"),
                })
                .collect(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct MetricResponseDto {
    pub side: String,
    pub group: String,
    pub metric: String,
    pub operator: String,
    pub window_sec: Option<f64>,
    /// One of `keep` | `drop_no_edge` | `drop_negative` | `drop_spike` | `drop_thin`
    /// | `drop_no_baseline`.
    pub verdict: String,
    /// The `off`-pick score this metric was measured against (`null` = unrankable).
    pub baseline: Option<f64>,
    /// `best − off`, present on a keep and on the two drops that still measured one.
    pub lift: Option<f64>,
    /// The best pick's absolute score — the number that separates "lifted into profit"
    /// from "lifted, still losing".
    pub best_score: Option<f64>,
    /// Neighbour retention, present only on a keep.
    pub plateau: Option<f64>,
    pub best_value: Option<f64>,
    /// The 2–3 value range forwarded to Layer 2 (keep only).
    pub narrowed: Vec<f64>,
    /// The full response curve — `off` first, then ascending values.
    pub curve: Vec<ResponsePointDto>,
}

impl From<&MetricResponse> for MetricResponseDto {
    fn from(r: &MetricResponse) -> Self {
        // Tag from the one table (`verdict_tag`); only the payload varies here.
        let (lift, plateau, best_value, narrowed) = match &r.verdict {
            Verdict::Keep { best_value, lift, plateau, narrowed } => {
                (Some(*lift), Some(*plateau), Some(*best_value), narrowed.clone())
            }
            Verdict::DropNoEdge { best_lift } => (Some(*best_lift), None, None, vec![]),
            Verdict::DropNegative { best_value, lift, .. } => {
                (Some(*lift), None, Some(*best_value), vec![])
            }
            Verdict::DropSpike { best_value, lift, plateau } => {
                (Some(*lift), Some(*plateau), Some(*best_value), vec![])
            }
            Verdict::DropThin { .. } => (None, None, None, vec![]),
            Verdict::DropNoBaseline => (None, None, None, vec![]),
        };
        Self {
            best_score: r.best_pick_score(),
            side: side_str(r.metric.side),
            group: group_spec(r.metric.group).name.to_string(),
            metric: r.metric.metric.name().to_string(),
            operator: r.operator.symbol().to_string(),
            window_sec: r.metric.window,
            verdict: verdict_tag(&r.verdict).to_string(),
            baseline: r.baseline,
            lift,
            plateau,
            best_value,
            narrowed,
            curve: r.curve.iter().map(ResponsePointDto::from).collect(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ResponsePointDto {
    /// `null` = the `off` pick.
    pub value: Option<f64>,
    /// The discovery score, or `null` when the pick was gated / never fired.
    pub score: Option<f64>,
    /// One of `ranked` | `below_min_closed` | `no_fire`.
    pub outcome: String,
    pub n_fired: u64,
    pub n_closed: u64,
    /// The gate this pick failed, when it was gated.
    pub gate: Option<u64>,
    pub win_rate: f64,
    pub median_pnl_pct: f64,
    /// Realised PnL in SOL — the only column on the curve that is in money.
    pub total_pnl_sol: f64,
}

impl From<&ResponsePoint> for ResponsePointDto {
    fn from(p: &ResponsePoint) -> Self {
        Self {
            value: p.value,
            score: p.outcome.score(),
            outcome: outcome_tag(p.outcome),
            n_fired: p.n_fired,
            n_closed: p.n_closed,
            gate: match p.outcome {
                ScoreOutcome::BelowMinClosed { gate, .. } => Some(gate),
                _ => None,
            },
            win_rate: p.win_rate,
            median_pnl_pct: p.median_pnl_pct,
            total_pnl_sol: p.total_pnl_sol,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct SkippedDto {
    pub side: String,
    pub group: String,
    pub metric: String,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct MenuGapDto {
    pub side: String,
    pub group: String,
    pub metric: String,
    pub reason: String,
}

// ───────────────────────────── Layer 2 ─────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct FamilyDto {
    pub combos_scanned: usize,
    pub families: Vec<FamilyResultDto>,
    pub interactions: Vec<InteractionDto>,
    /// Joint grids over interacting connected components.
    pub joints: Vec<JointResultDto>,
    /// Every synergy-rescue attempt, kept or refused.
    pub rescues: Vec<RescueDto>,
}

impl FamilyDto {
    fn from_report(r: &FamilyReport) -> Self {
        Self {
            combos_scanned: r.combos_scanned,
            families: r.families.iter().map(FamilyResultDto::from).collect(),
            interactions: r.interactions.iter().map(InteractionDto::from).collect(),
            joints: r.joints.iter().map(JointResultDto::from).collect(),
            rescues: r.rescues.iter().map(RescueDto::from).collect(),
        }
    }
}

/// One Layer-1 reject re-screened under a pinned winner.
#[derive(Debug, Serialize)]
pub struct RescueDto {
    pub side: String,
    pub group: String,
    pub metric: String,
    pub operator: String,
    /// The family whose winner was pinned.
    pub pinned: String,
    /// The pinned-alone score this rescue's lift is measured against.
    pub pinned_score: f64,
    /// Same tag vocabulary as a Layer-1 verdict — `keep` means rescued.
    pub verdict: String,
    /// Lift given the pin, on a successful rescue.
    pub lift: Option<f64>,
}

impl From<&Rescue> for RescueDto {
    fn from(r: &Rescue) -> Self {
        Self {
            side: side_str(r.metric.side),
            group: group_spec(r.metric.group).name.to_string(),
            metric: r.metric.metric.name().to_string(),
            operator: r.operator.symbol().to_string(),
            pinned: r.pinned.as_str().to_string(),
            pinned_score: r.pinned_score,
            verdict: verdict_tag(&r.verdict).to_string(),
            lift: r.lift(),
        }
    }
}

/// The stable wire tag for a Layer-1 verdict. One table, so a rescue's verdict and a
/// screened metric's verdict can never drift into two vocabularies.
fn verdict_tag(v: &Verdict) -> &'static str {
    match v {
        Verdict::Keep { .. } => "keep",
        Verdict::DropNoEdge { .. } => "drop_no_edge",
        Verdict::DropNegative { .. } => "drop_negative",
        Verdict::DropSpike { .. } => "drop_spike",
        Verdict::DropThin { .. } => "drop_thin",
        Verdict::DropNoBaseline => "drop_no_baseline",
    }
}

#[derive(Debug, Serialize)]
pub struct FamilyResultDto {
    pub family: String,
    pub combos: usize,
    pub n_gated: usize,
    pub members: Vec<FamilyMemberDto>,
    /// Members a bound removed, with the reason — no silent caps.
    pub dropped: Vec<DroppedMemberDto>,
    /// The winning combo, `null` when everything was gated / nothing fired.
    pub best: Option<BestComboDto>,
}

impl From<&FamilyResult> for FamilyResultDto {
    fn from(f: &FamilyResult) -> Self {
        Self {
            family: f.family.as_str().to_string(),
            combos: f.combos,
            n_gated: f.n_gated,
            members: f
                .members
                .iter()
                .map(|m| FamilyMemberDto {
                    side: side_str(m.metric.side),
                    group: group_spec(m.metric.group).name.to_string(),
                    metric: m.metric.metric.name().to_string(),
                    operator: m.operator.symbol().to_string(),
                    values: m.values.clone(),
                    lift: m.lift,
                    rescued: m.rescued,
                })
                .collect(),
            dropped: f
                .dropped
                .iter()
                .map(|(m, reason)| DroppedMemberDto {
                    metric: m.metric.metric.name().to_string(),
                    reason: match reason {
                        DropReason::AxisCap => "axis_cap",
                        DropReason::ComboCap => "combo_cap",
                    }
                    .to_string(),
                })
                .collect(),
            best: f.best.as_ref().map(BestComboDto::from),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct FamilyMemberDto {
    pub side: String,
    pub group: String,
    pub metric: String,
    pub operator: String,
    pub values: Vec<f64>,
    pub lift: f64,
    /// True when the synergy rescue supplied this axis — its lift is conditional on
    /// the pinned winner, not standalone.
    pub rescued: bool,
}

#[derive(Debug, Serialize)]
pub struct DroppedMemberDto {
    pub metric: String,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct BestComboDto {
    pub score: f64,
    pub n_fired: u64,
    pub n_closed: u64,
    /// The value each metric axis picked (`null` = `off`), in axis order.
    pub picks: Vec<Option<f64>>,
    /// The canonical `RuleParams` — the promote handoff.
    pub params: serde_json::Value,
}

impl From<&BestCombo> for BestComboDto {
    fn from(b: &BestCombo) -> Self {
        Self {
            score: b.score,
            n_fired: b.n_fired,
            n_closed: b.n_closed,
            picks: b.picks.clone(),
            params: b.params_json.clone(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct InteractionDto {
    pub pinned: String,
    pub swept: String,
    /// One of `independent` | `interacting` | `inconclusive`.
    pub verdict: String,
    pub alone: Vec<Option<f64>>,
    pub given: Vec<Option<f64>>,
    pub score_alone: f64,
    pub score_given: Option<f64>,
}

impl From<&Interaction> for InteractionDto {
    fn from(i: &Interaction) -> Self {
        Self {
            pinned: i.pinned.as_str().to_string(),
            swept: i.swept.as_str().to_string(),
            verdict: match i.verdict {
                InteractionVerdict::Independent => "independent",
                InteractionVerdict::Interacting => "interacting",
                InteractionVerdict::Inconclusive => "inconclusive",
            }
            .to_string(),
            alone: i.alone.clone(),
            given: i.given.clone(),
            score_alone: i.score_alone,
            score_given: i.score_given,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct JointResultDto {
    pub families: Vec<String>,
    pub combos: usize,
    pub n_gated: usize,
    pub members: Vec<FamilyMemberDto>,
    pub dropped: Vec<DroppedMemberDto>,
    pub best: Option<BestComboDto>,
}

impl From<&JointResult> for JointResultDto {
    fn from(j: &JointResult) -> Self {
        Self {
            families: j.families.iter().map(|f| f.as_str().to_string()).collect(),
            combos: j.combos,
            n_gated: j.n_gated,
            members: j
                .members
                .iter()
                .map(|m| FamilyMemberDto {
                    side: side_str(m.metric.side),
                    group: group_spec(m.metric.group).name.to_string(),
                    metric: m.metric.metric.name().to_string(),
                    operator: m.operator.symbol().to_string(),
                    values: m.values.clone(),
                    lift: m.lift,
                    rescued: m.rescued,
                })
                .collect(),
            dropped: j
                .dropped
                .iter()
                .map(|(m, reason)| DroppedMemberDto {
                    metric: m.metric.metric.name().to_string(),
                    reason: match reason {
                        DropReason::AxisCap => "axis_cap",
                        DropReason::ComboCap => "combo_cap",
                    }
                    .to_string(),
                })
                .collect(),
            best: j.best.as_ref().map(BestComboDto::from),
        }
    }
}

// ───────────────────────────── sweep seed ──────────────────────────────────

#[derive(Debug, Serialize)]
pub struct SweepSeedDto {
    pub axes: Vec<crate::sweep::generic::axes::AxisSpec>,
    pub optional_axes: Vec<crate::sweep::generic::axes::AxisSpec>,
    pub clusters: Vec<SeedClusterDto>,
    pub combo_estimate: usize,
    pub notes: Vec<String>,
}

impl From<&SweepSeed> for SweepSeedDto {
    fn from(s: &SweepSeed) -> Self {
        Self {
            axes: s.axes.clone(),
            optional_axes: s.optional_axes.clone(),
            clusters: s.clusters.iter().map(SeedClusterDto::from).collect(),
            combo_estimate: s.combo_estimate,
            notes: s.notes.clone(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct SeedClusterDto {
    pub families: Vec<String>,
    pub interacting: bool,
    pub members: Vec<String>,
}

impl From<&SeedCluster> for SeedClusterDto {
    fn from(c: &SeedCluster) -> Self {
        Self {
            families: c.families.clone(),
            interacting: c.interacting,
            members: c.members.clone(),
        }
    }
}

// ───────────────────────────── Layer 3 ─────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ValidationDto {
    pub train_tokens: usize,
    pub validate_tokens: usize,
    /// Closed trades a candidate needs on the held-out slice to return a verdict at
    /// all — the slice's own cohort-scaled gate. Without it, `thin_validate` reads as
    /// a property of the candidate rather than of the slice's size.
    pub effective_min_closed: u64,
    /// RFC3339 of the split boundary (first validate-side `created_at`).
    pub boundary: Option<String>,
    pub candidates: Vec<CandidateValidationDto>,
}

impl ValidationDto {
    fn from_report(r: &ValidationReport) -> Self {
        Self {
            train_tokens: r.train_tokens,
            validate_tokens: r.validate_tokens,
            effective_min_closed: r.effective_min_closed,
            boundary: r.boundary.map(|b| b.to_rfc3339()),
            candidates: r.candidates.iter().map(CandidateValidationDto::from).collect(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct CandidateValidationDto {
    pub label: String,
    /// One of `holds` | `degraded` | `failed` | `thin_validate` | `no_fire_validate`
    /// | `unrankable_train`.
    pub verdict: String,
    /// `validate_score / train_score`, where defined.
    pub retention: Option<f64>,
    pub train: SliceScoreDto,
    pub validate: SliceScoreDto,
    /// The canonical `RuleParams` — a survivor promotes straight from here.
    pub params: serde_json::Value,
}

impl From<&CandidateValidation> for CandidateValidationDto {
    fn from(c: &CandidateValidation) -> Self {
        Self {
            label: c.label.clone(),
            verdict: validation_verdict_tag(c.verdict),
            retention: c.verdict.retention(),
            train: SliceScoreDto::from(&c.train),
            validate: SliceScoreDto::from(&c.validate),
            params: c.params_json.clone(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct SliceScoreDto {
    pub tokens: usize,
    pub score: Option<f64>,
    pub outcome: String,
    pub n_fired: u64,
    pub n_closed: u64,
    pub win_rate: f64,
    pub median_pnl_pct: f64,
    pub total_pnl_sol: f64,
}

impl From<&SliceScore> for SliceScoreDto {
    fn from(s: &SliceScore) -> Self {
        Self {
            tokens: s.tokens,
            score: s.score(),
            outcome: outcome_tag(s.outcome),
            n_fired: s.metrics.n_fired,
            n_closed: s.metrics.n_closed,
            win_rate: s.metrics.win_rate,
            median_pnl_pct: s.metrics.median_pnl_pct,
            total_pnl_sol: s.metrics.total_pnl_sol,
        }
    }
}

// ───────────────────────────── helpers ─────────────────────────────────────

fn side_str(side: crate::sweep::generic::axes::AxisSide) -> String {
    use crate::sweep::generic::axes::AxisSide;
    match side {
        AxisSide::Entry => "entry",
        AxisSide::Exit => "exit",
    }
    .to_string()
}

fn outcome_tag(o: ScoreOutcome) -> String {
    match o {
        ScoreOutcome::Ranked(_) => "ranked",
        ScoreOutcome::BelowMinClosed { .. } => "below_min_closed",
        ScoreOutcome::NoFire => "no_fire",
    }
    .to_string()
}

fn validation_verdict_tag(v: ValidationVerdict) -> String {
    match v {
        ValidationVerdict::Holds { .. } => "holds",
        ValidationVerdict::Degraded { .. } => "degraded",
        ValidationVerdict::Failed { .. } => "failed",
        ValidationVerdict::ThinValidate { .. } => "thin_validate",
        ValidationVerdict::NoFireValidate => "no_fire_validate",
        ValidationVerdict::UnrankableTrain => "unrankable_train",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};

    use super::super::fixtures::{corpus, created_at, pricing};
    use super::super::objective::DiscoveryWeights;
    use super::super::pipeline::{run_pipeline, PipelineConfig};
    use crate::sweep::progress::NoopObserver;

    fn as_of() -> DateTime<Utc> {
        created_at() + chrono::Duration::seconds(120)
    }

    /// The DTO serializes cleanly and mirrors the report — the fields the frontend
    /// reads are all present, with stable tag strings and no recomputation.
    #[test]
    fn pipeline_report_serializes_to_stable_tags() {
        let c = corpus(24);
        let cfg = PipelineConfig {
            weights: DiscoveryWeights { min_closed: 1, ..DiscoveryWeights::default() },
            ..PipelineConfig::default()
        };
        let report = run_pipeline(&c, &cfg, pricing(), as_of(), &NoopObserver).unwrap();
        let dto = PipelineDto::from_report(&report);
        let j = serde_json::to_value(&dto).expect("DTO serializes");

        assert_eq!(j["cohort_tokens"], 24);
        assert!(j["screen"]["responses"].as_array().unwrap().len() > 0);
        // Every response verdict is one of the stable tags — no Debug leakage.
        let allowed = [
            "keep",
            "drop_no_edge",
            "drop_negative",
            "drop_spike",
            "drop_thin",
            "drop_no_baseline",
        ];
        for r in j["screen"]["responses"].as_array().unwrap() {
            let v = r["verdict"].as_str().unwrap();
            assert!(allowed.contains(&v), "unexpected verdict tag {v}");
            // Operators are JSON symbols, never enum Debug.
            assert!([">=", "<", ">", "<=", "=", "!="].contains(&r["operator"].as_str().unwrap()));
            // Every curve point carries money, not just a ranking score.
            for p in r["curve"].as_array().unwrap() {
                assert!(p["total_pnl_sol"].is_number(), "a curve point must carry SOL");
                assert!(p["median_pnl_pct"].is_number());
                assert!(p["win_rate"].is_number());
            }
        }
        // A rescue speaks the same verdict vocabulary as a screened metric.
        for r in j["family"]["rescues"].as_array().unwrap() {
            assert!(allowed.contains(&r["verdict"].as_str().unwrap()));
        }
        // The reference line and the gate that ran are always on the wire.
        assert!(j["screen"]["effective_min_closed"].is_number());
        assert!(j["screen"]["min_closed"].is_number());
        assert!(j["screen"]["baseline"].is_object());
        assert!(j["diagnostics"].is_array(), "a run always explains how to read it");
        assert!(j["family"]["families"].is_array());
        assert!(j["family"]["joints"].is_array());
        assert!(j["sweep_seed"]["axes"].is_array());
        assert!(j["sweep_seed"]["combo_estimate"].is_number());
        if j["validation"].is_null() {
            assert!(j["no_validation"].is_string());
        } else {
            for c in j["validation"]["candidates"].as_array().unwrap() {
                assert!(c["params"].is_object(), "a validated candidate carries promotable params");
            }
        }
    }
}
