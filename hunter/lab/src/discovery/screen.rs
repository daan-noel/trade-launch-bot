//! **Layer 1 — the univariate screen** (plan §2.2): before any grid, learn which
//! metrics have an edge at all, at what threshold, in which direction — and throw
//! out the dead ones.
//!
//! The screen is **additive, not multiplicative**: every screenable
//! `(side, metric, operator)` is swept *alone* across its generated menu
//! ([`candidates`](super::candidates)) against a fixed baseline TP/SL, so the cost is
//! `Σ menu_len` combos (~6N) rather than the `6^N` a blind full grid would cost. Each
//! menu carries the `off` sentinel as pick 0, so a metric's marginal value is read
//! straight off its own curve — with-vs-without, same cohort, same baseline.
//!
//! ## One precompute for every metric (the D2 payoff)
//!
//! The whole screen runs as ONE [`AdditiveStrategy`] pass: each `(metric, operator)`
//! is a segment, every segment shares one per-token precompute, and the sweep engine
//! builds each token's series once for all of them instead of once per metric. See
//! [`additive`](super::additive) for the mechanism and why it is a scan mode rather
//! than a second engine.
//!
//! The percentile pass ([`collect_percentiles`]) still walks the corpus once before
//! this, and deliberately so: the menus are an *input* to the sparse grid's
//! `time`/`stall` horizons, so a single fused pass would have to size the grid from
//! percentiles it has not measured yet. That pass folds trades only (no synthetic
//! ticks), so it is the cheap half; the corpus itself is loaded once for both.
//!
//! ## The verdict
//!
//! Each segment's curve is scored with the discovery objective
//! ([`discovery_score`]) and classified (plan §2.2):
//!
//! * *smooth + directional + plateau* → [`Verdict::Keep`], with the suggested
//!   operator and a narrowed 2–3 value range for Layer 2;
//! * *flat* (score ≈ the `off` pick everywhere) → [`Verdict::DropNoEdge`];
//! * *single spike* surrounded by noise → [`Verdict::DropSpike`] (overfit).
//!
//! Nothing is dropped silently: a metric with no menu, no baseline, or nothing but
//! min-N-gated picks is reported with its reason, alongside the plan's `skipped`
//! list and the count of combos the min-N gate killed.

use anyhow::{bail, Result};
use hunter_engine::metrics::evaluator::Operator;
use hunter_engine::metrics::Ts;

use crate::sweep::aggregate::ComboMetrics;
use crate::sweep::corpus::Corpus;
use crate::sweep::generic::axes::{AxesModel, AxesRequest, AxisSpec, ResolvedAxis};
use crate::sweep::generic::Pricing;
use crate::sweep::progress::SweepObserver;

use super::additive::AdditiveStrategy;
use super::candidates::{
    build_menus, collect_percentiles, MenuGap, MetricCandidates, PercentileTable, ScreenConfig,
    ScreenMetric, ScreenPlan, Skipped,
};
use super::objective::{discovery_score, ComboStats, DiscoveryWeights, ScoreOutcome};

// ───────────────────────────── the baseline ────────────────────────────────

/// The fixed exit the whole screen runs against (plan §2.2: "sweep each metric alone
/// … with a fixed baseline TP/SL").
///
/// **Part of a screen's identity, not a tuning knob** — the same reasoning as
/// [`Pricing`]. Every metric is ranked by what it adds *over this baseline*, so two
/// screens run under different baselines are not comparable and their shortlists must
/// not be merged. Deliberately no `Default`: the caller names the baseline it screened
/// against, and it is echoed back in the report.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenBaseline {
    /// Take-profit %, or `None` to omit the TP guard.
    pub take_profit_pct: Option<f64>,
    /// Stop-loss %, or `None` to omit the SL guard.
    pub stop_loss_pct: Option<f64>,
}

impl ScreenBaseline {
    /// The TP/SL axes this baseline contributes to every segment's model.
    fn axes(&self) -> Vec<AxisSpec> {
        let mut out = Vec::with_capacity(2);
        if let Some(tp) = self.take_profit_pct {
            out.push(fixed_axis("take_profit", tp));
        }
        if let Some(sl) = self.stop_loss_pct {
            out.push(fixed_axis("stop_loss", sl));
        }
        out
    }

    fn validate(&self) -> Result<()> {
        let ok = |v: Option<f64>| v.is_none_or(|x| x.is_finite() && x > 0.0);
        if !ok(self.take_profit_pct) || !ok(self.stop_loss_pct) {
            bail!("screen baseline TP/SL must be finite and > 0");
        }
        // With neither guard a position only ever closes on the analysis death-close,
        // so almost every combo lands under the min-N gate and the whole screen reads
        // as "no edge anywhere" — a silent no-op run. Refuse it up front.
        if self.take_profit_pct.is_none() && self.stop_loss_pct.is_none() {
            bail!(
                "screen baseline needs at least one of take_profit_pct / stop_loss_pct — \
                 with neither, positions never close and every metric is min-N gated"
            );
        }
        Ok(())
    }
}

fn fixed_axis(kind: &str, value: f64) -> AxisSpec {
    AxisSpec {
        kind: kind.to_string(),
        side: None,
        group: None,
        metric: None,
        operator: None,
        window: None,
        values: vec![Some(value)],
    }
}

// ───────────────────────────── verdict tuning ──────────────────────────────

/// Thresholds the keep/drop verdict is measured against (plan §2.2). Seeded as D1
/// constants are: sane defaults, overridable per run, pinned in `docs/plans/` once
/// validated against a real cohort.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenThresholds {
    /// The best pick must beat the `off` baseline by at least this fraction of
    /// `|baseline|` — the "score ≈ off everywhere ⇒ flat ⇒ no edge" test.
    pub min_lift_ratio: f64,
    /// …and by at least this absolute score, so a baseline sitting near zero can't
    /// make an arbitrarily small wobble read as an edge. `0.0` = ratio-only.
    pub min_abs_lift: f64,
    /// A **neighbouring** candidate value must retain at least this fraction of the
    /// best pick's lift. A peak whose neighbours collapse is a single spike (the
    /// overfit signature); one that holds is a plateau. Also the width test for the
    /// narrowed range handed to Layer 2.
    pub min_plateau: f64,
    /// Cap on the number of values a kept metric forwards to Layer 2.
    pub narrow_to: usize,
}

impl Default for ScreenThresholds {
    fn default() -> Self {
        Self { min_lift_ratio: 0.10, min_abs_lift: 0.0, min_plateau: 0.5, narrow_to: 3 }
    }
}

// ───────────────────────────── the segments ────────────────────────────────

/// One `(side, metric, operator)` swept alone — the screen's unit of work.
#[derive(Clone, Debug)]
pub struct ScreenSegment {
    pub metric: ScreenMetric,
    pub operator: Operator,
    /// The swept values in combo order, read back off the **resolved** axis so the
    /// curve can never drift from what the sweep actually ran. `values[0]` is the
    /// `off` sentinel (`None`).
    pub values: Vec<Option<f64>>,
    /// The sub-model handed to the additive scan.
    pub model: AxesModel,
}

impl ScreenSegment {
    /// Build one segment's sub-model: the metric axis (menu incl. `off`) plus the
    /// run's fixed TP/SL.
    fn build(
        menu: &MetricCandidates,
        operator: Operator,
        baseline: &ScreenBaseline,
    ) -> Result<Self, String> {
        let mut axes = vec![menu.axis_spec(operator)];
        axes.extend(baseline.axes());
        let model = AxesModel::resolve(&AxesRequest { axes })?;
        let values = metric_axis_values(&model)
            .ok_or_else(|| "segment lost its metric axis".to_string())?;
        Ok(Self { metric: menu.metric, operator, values, model })
    }
}

/// The swept values of a model's (single) metric axis, in pick order.
fn metric_axis_values(model: &AxesModel) -> Option<Vec<Option<f64>>> {
    model.axes.iter().find_map(|a| match a {
        ResolvedAxis::Metric { values, .. } => Some(values.clone()),
        _ => None,
    })
}

/// A registry metric whose menu existed but whose sub-model could not be resolved
/// (a malformed axis) — reported, never silently dropped.
#[derive(Clone, Debug, PartialEq)]
pub struct SegmentError {
    pub metric: ScreenMetric,
    pub operator: Operator,
    pub error: String,
}

// ───────────────────────────── response curves ─────────────────────────────

/// One point on a metric's response curve: the value swept, what it scored, and the
/// money behind that score.
///
/// The objective is a unitless product (median pnl% × fire rate × win quality ×
/// confidence), so a lift of `2.0` is a **ranking key and nothing else**. The realised
/// columns travel alongside it so a reader can answer "…and how much SOL was that?"
/// without re-running the combo through simulate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResponsePoint {
    /// `None` = the `off` pick (the baseline this metric is measured against).
    pub value: Option<f64>,
    pub outcome: ScoreOutcome,
    pub n_fired: u64,
    pub n_closed: u64,
    /// Realised win rate over closed trades (`0..=1`).
    pub win_rate: f64,
    /// Realised median per-trade pnl% (closed only) — the objective's profit centre.
    pub median_pnl_pct: f64,
    /// Realised PnL in SOL at the run's `buy_amount_sol` — the only column in money.
    pub total_pnl_sol: f64,
}

impl ResponsePoint {
    fn score(&self) -> Option<f64> {
        self.outcome.score()
    }

    /// Project a scored sweep row into a curve point. Shared with Layer 2's synergy
    /// rescue so a rescued curve is built exactly like a screened one.
    pub(super) fn from_row(value: Option<f64>, m: &ComboMetrics, outcome: ScoreOutcome) -> Self {
        Self {
            value,
            outcome,
            n_fired: m.n_fired,
            n_closed: m.n_closed,
            win_rate: m.win_rate,
            median_pnl_pct: m.median_pnl_pct,
            total_pnl_sol: m.total_pnl_sol,
        }
    }
}

/// Why a screened metric is kept or dropped (plan §2.2).
#[derive(Clone, Debug, PartialEq)]
pub enum Verdict {
    /// Smooth, directional, with a plateau — carry it into Layer 2.
    Keep {
        /// The best candidate value under `operator`.
        best_value: f64,
        /// `best − off`, in objective units. The metric's marginal value, and the
        /// shortlist's ranking key.
        lift: f64,
        /// How much of that lift the best neighbouring value retains (`0..=1`+). The
        /// across-neighbour stability measure of plan §1.3.
        plateau: f64,
        /// The narrowed 2–3 value range Layer 2 grids over.
        narrowed: Vec<f64>,
    },
    /// Flat — the best pick never meaningfully beats its own `off`.
    DropNoEdge { best_lift: f64 },
    /// The metric **does** lift its baseline, but the best pick is still a losing
    /// combo. Split out of [`Verdict::DropNoEdge`] deliberately: "no signal here" and
    /// "real signal, unprofitable baseline" call for opposite next moves, and reading
    /// the second as the first is what makes a run against a losing bracket look like
    /// a dead cohort.
    DropNegative { best_value: f64, lift: f64, best_score: f64 },
    /// A single spike surrounded by noise — the overfit signature.
    DropSpike { best_value: f64, lift: f64, plateau: f64 },
    /// Every candidate value fell under the min-N gate (`n_closed` too small).
    DropThin { best_n_closed: u64 },
    /// The `off` pick itself is unrankable, so there is nothing to measure against.
    DropNoBaseline,
}

impl Verdict {
    pub fn is_keep(&self) -> bool {
        matches!(self, Verdict::Keep { .. })
    }
}

/// One screened `(side, metric, operator)`, its measured curve, and the verdict.
#[derive(Clone, Debug, PartialEq)]
pub struct MetricResponse {
    pub metric: ScreenMetric,
    pub operator: Operator,
    /// `[off, v1 … vN]` — pick order, exactly as swept.
    pub curve: Vec<ResponsePoint>,
    /// The `off` pick's score (the with-vs-without baseline).
    pub baseline: Option<f64>,
    pub verdict: Verdict,
}

impl MetricResponse {
    /// Ranking key: marginal value over `off`. Non-keeps sort last.
    pub fn lift(&self) -> Option<f64> {
        match self.verdict {
            Verdict::Keep { lift, .. } => Some(lift),
            _ => None,
        }
    }

    /// The menu this metric was screened over, read back off the curve (`off` first,
    /// then the values ascending) — exactly the axis the sweep ran. Layer 2's synergy
    /// rescue re-screens a dropped metric over this same menu rather than
    /// re-measuring the cohort's percentiles.
    pub fn menu_values(&self) -> Vec<Option<f64>> {
        self.curve.iter().map(|p| p.value).collect()
    }

    /// The best score any non-`off` pick reached, whatever the verdict. The ranking
    /// key for which dropped metrics are worth a rescue attempt.
    pub fn best_pick_score(&self) -> Option<f64> {
        self.curve
            .iter()
            .filter(|p| p.value.is_some())
            .filter_map(ResponsePoint::score)
            .fold(None, |acc: Option<f64>, s| Some(acc.map_or(s, |a| a.max(s))))
    }
}

/// What the run's bare TP/SL bracket did on the cohort, with no metric gate at all.
///
/// It is the `off` pick of every segment — identical across them by construction, so
/// it is hoisted here once and becomes the report's **reference line**: every Keep's
/// `lift` is measured against exactly this, and a reader can see immediately whether
/// the thing being lifted was profitable to begin with.
#[derive(Clone, Debug)]
pub struct BaselineStats {
    pub outcome: ScoreOutcome,
    /// The full row, so the reference line carries money and not just a score.
    pub metrics: ComboMetrics,
}

impl BaselineStats {
    pub fn score(&self) -> Option<f64> {
        self.outcome.score()
    }

    /// True when the bare bracket loses money on this cohort. Every `Keep` is then a
    /// rescue from a losing baseline rather than an improvement on a working one, and
    /// the report says so instead of leaving it to be inferred.
    pub fn is_unprofitable(&self) -> bool {
        self.outcome.score().is_none_or(|s| s <= 0.0)
    }
}

/// The Layer-1 deliverable: a ranked metric shortlist plus everything that did not
/// make it, with reasons.
#[derive(Clone, Debug)]
pub struct ScreenReport {
    /// Tokens in the cohort — the `fire_rate` denominator every score used.
    pub cohort_tokens: usize,
    pub baseline: ScreenBaseline,
    /// The bare bracket's own result — the reference line every `lift` is measured
    /// against. `None` only when nothing was screened at all.
    pub baseline_stats: Option<BaselineStats>,
    pub weights: DiscoveryWeights,
    /// The min-N gate that actually ran, after cohort scaling
    /// ([`DiscoveryWeights::effective_min_closed`]). Reported because a relaxed gate
    /// changes what "gated" means and a reader must not have to re-derive it.
    pub effective_min_closed: u64,
    pub thresholds: ScreenThresholds,
    /// One entry per screened `(metric, operator)`, in plan order.
    pub responses: Vec<MetricResponse>,
    /// Indices into [`Self::responses`] whose verdict is `Keep`, best lift first.
    /// **This is the shortlist.**
    pub shortlist: Vec<usize>,
    /// Registry metrics the plan excluded up front, with the reason.
    pub skipped: Vec<Skipped>,
    /// Screened metrics that produced no usable menu, with the reason.
    pub gaps: Vec<(ScreenMetric, MenuGap)>,
    /// Menus whose sub-model failed to resolve.
    pub errors: Vec<SegmentError>,
    /// The measured distributions the menus were spaced on (the audit trail).
    pub percentiles: PercentileTable,
    /// How many swept combos the min-N gate removed — reported, never silent.
    pub n_gated: usize,
    /// Combos actually scanned (`Σ menu_len` — the additive budget).
    pub combos_scanned: usize,
}

impl ScreenReport {
    /// The shortlist as responses, best first.
    pub fn shortlisted(&self) -> impl Iterator<Item = &MetricResponse> {
        self.shortlist.iter().map(move |i| &self.responses[*i])
    }
}

/// Run the whole Layer-1 screen over `corpus`.
///
/// Sequence: [`screen_plan`](super::candidates::screen_plan) → [`collect_percentiles`]
/// → [`build_menus`] → one additive sweep over the shared precompute → score → verdict.
/// Runs synchronously on the caller's rayon pool (wrap it in `spawn_blocking` +
/// a bounded pool exactly like `sweep_generic` does).
///
/// `as_of` is the run's deadness "now"; `pricing` is the run's pricing identity. When
/// `cfg.flow_patterns` is set the corpus must have been loaded with
/// `Selection::with_flow` — otherwise the flow columns read `NaN` and every flow
/// metric reports as thin.
#[allow(clippy::too_many_arguments)]
pub fn run_screen(
    corpus: &Corpus,
    cfg: &ScreenConfig,
    baseline: ScreenBaseline,
    pricing: Pricing,
    as_of: Ts,
    weights: DiscoveryWeights,
    thresholds: ScreenThresholds,
    observer: &dyn SweepObserver,
) -> Result<ScreenReport> {
    baseline.validate()?;
    let plan = super::candidates::screen_plan(cfg);
    let percentiles = collect_percentiles(&corpus.tokens, &plan, cfg);
    let menus = build_menus(&plan, &percentiles, cfg);
    screen_with_menus(
        corpus,
        cfg,
        &plan,
        &menus.menus,
        menus.gaps,
        percentiles,
        baseline,
        pricing,
        as_of,
        weights,
        thresholds,
        observer,
    )
}

/// The screen given pre-built menus — the seam Layer 2 and the tests reuse so a
/// narrowed menu can be re-screened without re-measuring the cohort.
#[allow(clippy::too_many_arguments)]
pub fn screen_with_menus(
    corpus: &Corpus,
    cfg: &ScreenConfig,
    plan: &ScreenPlan,
    menus: &[MetricCandidates],
    gaps: Vec<(ScreenMetric, MenuGap)>,
    percentiles: PercentileTable,
    baseline: ScreenBaseline,
    pricing: Pricing,
    as_of: Ts,
    weights: DiscoveryWeights,
    thresholds: ScreenThresholds,
    observer: &dyn SweepObserver,
) -> Result<ScreenReport> {
    baseline.validate()?;
    let mut segments = Vec::new();
    let mut errors = Vec::new();
    for menu in menus {
        for op in &menu.operators {
            match ScreenSegment::build(menu, *op, &baseline) {
                Ok(seg) => segments.push(seg),
                Err(error) => {
                    errors.push(SegmentError { metric: menu.metric, operator: *op, error })
                }
            }
        }
    }

    let matched = corpus.token_count() as u64;
    let effective_min_closed = weights.effective_min_closed(matched);
    if segments.is_empty() {
        return Ok(ScreenReport {
            cohort_tokens: corpus.token_count(),
            baseline,
            baseline_stats: None,
            weights,
            effective_min_closed,
            thresholds,
            responses: Vec::new(),
            shortlist: Vec::new(),
            skipped: plan.skipped.clone(),
            gaps,
            errors,
            percentiles,
            n_gated: 0,
            combos_scanned: 0,
        });
    }

    // One additive pass: every segment shares one per-token precompute.
    let strategy = AdditiveStrategy::new(
        segments.iter().map(|s| s.model.clone()).collect(),
        pricing,
        as_of,
        cfg.flow_patterns.as_ref(),
    );
    let combos_scanned = strategy.combos().len();
    let rows_by_segment = strategy.run(corpus, observer)?;

    let mut responses = Vec::with_capacity(segments.len());
    let mut n_gated = 0usize;
    for (seg, rows) in segments.iter().zip(&rows_by_segment) {
        let (r, gated) = analyse(seg, rows, matched, weights, thresholds);
        n_gated += gated;
        responses.push(r);
    }

    // The reference line: pick 0 of any segment is the bare bracket with no metric
    // gate, and every segment's is the same combo. Take the first rather than
    // re-simulating it — a second measurement of the same thing can only disagree.
    let baseline_stats = rows_by_segment.first().and_then(|rows| rows.first()).map(|m| BaselineStats {
        outcome: discovery_score(ComboStats::from_combo_metrics(m), matched, weights),
        metrics: m.clone(),
    });

    let mut shortlist: Vec<usize> = (0..responses.len()).filter(|i| responses[*i].verdict.is_keep()).collect();
    shortlist.sort_by(|a, b| {
        let la = responses[*a].lift().unwrap_or(f64::NEG_INFINITY);
        let lb = responses[*b].lift().unwrap_or(f64::NEG_INFINITY);
        lb.partial_cmp(&la).unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(ScreenReport {
        cohort_tokens: corpus.token_count(),
        baseline,
        baseline_stats,
        weights,
        effective_min_closed,
        thresholds,
        responses,
        shortlist,
        skipped: plan.skipped.clone(),
        gaps,
        errors,
        percentiles,
        n_gated,
        combos_scanned,
    })
}

/// Score one segment's rows and classify the curve. Returns the response plus how
/// many of its picks the min-N gate killed.
fn analyse(
    seg: &ScreenSegment,
    rows: &[ComboMetrics],
    matched: u64,
    weights: DiscoveryWeights,
    th: ScreenThresholds,
) -> (MetricResponse, usize) {
    let mut curve = Vec::with_capacity(rows.len());
    let mut gated = 0usize;
    for (pick, m) in rows.iter().enumerate() {
        let outcome = discovery_score(ComboStats::from_combo_metrics(m), matched, weights);
        if outcome.is_gated() {
            gated += 1;
        }
        curve.push(ResponsePoint::from_row(seg.values[pick], m, outcome));
    }
    let verdict = classify(&curve, th);
    let baseline = curve.first().and_then(ResponsePoint::score);
    (MetricResponse { metric: seg.metric, operator: seg.operator, curve, baseline, verdict }, gated)
}

/// Classify a response curve. `curve[0]` is the `off` pick; `curve[1..]` the values
/// ascending.
///
/// Shared with Layer 2's synergy rescue ([`family`](super::family)), which measures a
/// dropped metric's curve *given a pinned winner* — same shape, same verdict rules, so
/// "keep-grade" means one thing across the pipeline.
pub(super) fn classify(curve: &[ResponsePoint], th: ScreenThresholds) -> Verdict {
    // The off pick must exist and be rankable — it *is* the with-vs-without baseline.
    let Some(first) = curve.first() else { return Verdict::DropNoBaseline };
    if first.value.is_some() {
        return Verdict::DropNoBaseline;
    }
    let Some(baseline) = first.score() else { return Verdict::DropNoBaseline };

    let picks = &curve[1..];
    let ranked: Vec<(usize, f64)> =
        picks.iter().enumerate().filter_map(|(i, p)| p.score().map(|s| (i, s))).collect();
    if ranked.is_empty() {
        let best_n_closed = picks.iter().map(|p| p.n_closed).max().unwrap_or(0);
        return Verdict::DropThin { best_n_closed };
    }
    // The rank is only meaningful above zero (`objective` module docs), so the argmax
    // runs over the **positive** picks. With none, the curve cannot be ranked at all:
    // below zero a pick that fires more scores *worse*, so a bare max would return the
    // most restrictive gate on the menu — and `narrow` would then hand Layer 2 a range
    // built around it. Fall back to realised money, which stays monotone either side
    // of zero. The verdict below is unchanged: a non-positive best is still a drop.
    let (best_i, best) = match ranked.iter().copied().filter(|(_, s)| *s > 0.0).reduce(
        |acc, x| if x.1 > acc.1 { x } else { acc },
    ) {
        Some(top) => top,
        None => {
            let i = ranked
                .iter()
                .map(|(i, _)| *i)
                .reduce(|a, b| if picks[b].total_pnl_sol > picks[a].total_pnl_sol { b } else { a })
                .expect("ranked is non-empty");
            (i, picks[i].score().expect("a ranked pick has a score"))
        }
    };
    let best_value = picks[best_i].value.unwrap_or(f64::NAN);

    let lift = best - baseline;
    let required = (th.min_lift_ratio * baseline.abs()).max(th.min_abs_lift);
    if lift <= required {
        return Verdict::DropNoEdge { best_lift: lift };
    }
    // Real lift, still a losing combo — its own verdict, not "flat". A metric here is
    // a genuine signal measured against a baseline that loses money; the fix is the
    // baseline, not the metric.
    if best <= 0.0 {
        return Verdict::DropNegative { best_value, lift, best_score: best };
    }

    // Across-neighbour stability: how much of the lift the best adjacent candidate
    // retains. A collapse either side = a spike; a hold = a plateau.
    let retained = |i: usize| picks[i].score().map(|s| (s - baseline) / lift).unwrap_or(0.0);
    let neighbours: Vec<usize> = [best_i.checked_sub(1), (best_i + 1 < picks.len()).then_some(best_i + 1)]
        .into_iter()
        .flatten()
        .collect();
    // A one-value menu has no neighbour to contradict the peak; `candidate_menu`
    // already rejects those as `Degenerate`, so this is defensive only.
    let plateau = neighbours.iter().map(|i| retained(*i)).fold(f64::NEG_INFINITY, f64::max);
    let plateau = if neighbours.is_empty() { 1.0 } else { plateau };
    if plateau < th.min_plateau {
        return Verdict::DropSpike { best_value, lift, plateau };
    }

    Verdict::Keep {
        best_value,
        lift,
        plateau,
        narrowed: narrow(picks, best_i, baseline, lift, th),
    }
}

/// The contiguous 2–3 value window around the best pick whose members each retain
/// `min_plateau` of the lift. Grows toward the stronger side first, so a narrowed
/// range follows the curve's slope instead of always widening left.
fn narrow(
    picks: &[ResponsePoint],
    best_i: usize,
    baseline: f64,
    lift: f64,
    th: ScreenThresholds,
) -> Vec<f64> {
    let retained = |i: usize| picks[i].score().map(|s| (s - baseline) / lift).unwrap_or(f64::NEG_INFINITY);
    let (mut lo, mut hi) = (best_i, best_i);
    while hi - lo + 1 < th.narrow_to.max(1) {
        let left = lo.checked_sub(1).filter(|i| retained(*i) >= th.min_plateau);
        let right = (hi + 1 < picks.len()).then_some(hi + 1).filter(|i| retained(*i) >= th.min_plateau);
        match (left, right) {
            (Some(l), Some(r)) => {
                if retained(r) >= retained(l) {
                    hi = r;
                } else {
                    lo = l;
                }
            }
            (Some(l), None) => lo = l,
            (None, Some(r)) => hi = r,
            (None, None) => break,
        }
    }
    (lo..=hi).filter_map(|i| picks[i].value).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use hunter_engine::metrics::MetricGroupId;

    use super::super::candidates::{screen_plan, ValueSource};
    use super::super::fixtures::{corpus, pricing};
    use crate::sweep::generic::axes::AxisSide;
    use crate::sweep::progress::NoopObserver;

    fn baseline() -> ScreenBaseline {
        ScreenBaseline { take_profit_pct: Some(30.0), stop_loss_pct: Some(15.0) }
    }

    fn point(value: Option<f64>, score: Option<f64>, n_closed: u64) -> ResponsePoint {
        ResponsePoint {
            value,
            outcome: match score {
                Some(s) => ScoreOutcome::Ranked(s),
                None => ScoreOutcome::BelowMinClosed { n_closed, gate: 8 },
            },
            n_fired: n_closed,
            n_closed,
            win_rate: 0.5,
            median_pnl_pct: score.unwrap_or(0.0),
            total_pnl_sol: score.unwrap_or(0.0),
        }
    }

    #[test]
    fn baseline_needs_at_least_one_guard() {
        assert!(ScreenBaseline { take_profit_pct: None, stop_loss_pct: None }.validate().is_err());
        assert!(ScreenBaseline { take_profit_pct: Some(0.0), stop_loss_pct: None }.validate().is_err());
        assert!(ScreenBaseline { take_profit_pct: None, stop_loss_pct: Some(15.0) }.validate().is_ok());
    }

    #[test]
    fn flat_curve_is_dropped_as_no_edge() {
        let curve = vec![
            point(None, Some(10.0), 50),
            point(Some(5.0), Some(10.1), 50),
            point(Some(10.0), Some(9.8), 50),
            point(Some(20.0), Some(10.0), 50),
        ];
        // Best lift is 0.1 on a baseline of 10 → under the 10% ratio.
        assert!(matches!(
            classify(&curve, ScreenThresholds::default()),
            Verdict::DropNoEdge { .. }
        ));
    }

    #[test]
    fn single_spike_is_dropped_as_overfit() {
        let curve = vec![
            point(None, Some(10.0), 50),
            point(Some(5.0), Some(10.0), 50),
            point(Some(10.0), Some(40.0), 50), // the spike
            point(Some(20.0), Some(9.5), 50),
        ];
        match classify(&curve, ScreenThresholds::default()) {
            Verdict::DropSpike { best_value, plateau, .. } => {
                assert_eq!(best_value, 10.0);
                assert!(plateau < 0.5, "neighbours collapsed: {plateau}");
            }
            other => panic!("expected a spike drop, got {other:?}"),
        }
    }

    #[test]
    fn plateau_is_kept_and_narrowed_toward_the_stronger_side() {
        let curve = vec![
            point(None, Some(10.0), 50),
            point(Some(5.0), Some(11.0), 50),
            point(Some(10.0), Some(28.0), 50),
            point(Some(20.0), Some(30.0), 50), // best
            point(Some(40.0), Some(26.0), 50),
        ];
        match classify(&curve, ScreenThresholds::default()) {
            Verdict::Keep { best_value, lift, plateau, narrowed } => {
                assert_eq!(best_value, 20.0);
                assert!((lift - 20.0).abs() < 1e-9);
                assert!(plateau >= 0.5, "neighbour held the lift: {plateau}");
                // Both neighbours qualify; the window is capped at 3 and grows to the
                // stronger side first (10 retains 0.9, 40 retains 0.8).
                assert_eq!(narrowed, vec![10.0, 20.0, 40.0]);
            }
            other => panic!("expected a keep, got {other:?}"),
        }
    }

    /// Lift with a still-losing best is its own verdict — reading it as `DropNoEdge`
    /// is what makes a run against an unprofitable bracket look like a dead cohort.
    #[test]
    fn real_lift_on_a_losing_baseline_is_not_flat() {
        let curve = vec![
            point(None, Some(-10.0), 50),
            point(Some(5.0), Some(-8.0), 50),
            point(Some(10.0), Some(-2.0), 50), // best: lifted 8, still negative
            point(Some(20.0), Some(-6.0), 50),
        ];
        match classify(&curve, ScreenThresholds::default()) {
            Verdict::DropNegative { best_value, lift, best_score } => {
                assert_eq!(best_value, 10.0);
                assert!((lift - 8.0).abs() < 1e-9);
                assert!(best_score < 0.0);
            }
            other => panic!("expected a negative-baseline drop, got {other:?}"),
        }
        // …while a genuinely flat curve is still flat, negative baseline or not.
        let flat = vec![
            point(None, Some(-10.0), 50),
            point(Some(5.0), Some(-10.1), 50),
            point(Some(10.0), Some(-9.9), 50),
        ];
        assert!(matches!(
            classify(&flat, ScreenThresholds::default()),
            Verdict::DropNoEdge { .. }
        ));
    }

    /// Below zero the score ranks backwards — a pick that fires more scores *worse* —
    /// so the argmax must not run over negatives. The reported `best_value` (and the
    /// range `narrow` builds around it) has to follow realised money instead of
    /// crowning the most restrictive gate on the menu.
    #[test]
    fn a_losing_curve_reports_the_money_best_pick_not_the_least_traded() {
        let pt = |value: Option<f64>, score: f64, pnl_sol: f64| ResponsePoint {
            value,
            outcome: ScoreOutcome::Ranked(score),
            n_fired: 50,
            n_closed: 50,
            win_rate: 0.4,
            median_pnl_pct: -5.0,
            total_pnl_sol: pnl_sol,
        };
        let curve = vec![
            pt(None, -12.0, -9.0),      // the bare bracket
            pt(Some(5.0), -9.0, -2.0),  // loose gate: trades a lot, loses the least
            pt(Some(40.0), -0.4, -8.0), // tight gate: barely trades, score nearest zero
        ];
        match classify(&curve, ScreenThresholds::default()) {
            Verdict::DropNegative { best_value, best_score, lift } => {
                assert_eq!(best_value, 5.0, "the money-best pick must be the one reported");
                assert_eq!(best_score, -9.0);
                assert!((lift - 3.0).abs() < 1e-9, "lift is measured against the bare bracket");
            }
            other => panic!("expected a negative drop, got {other:?}"),
        }
    }

    #[test]
    fn all_picks_gated_reports_thin_not_flat() {
        let curve = vec![point(None, Some(5.0), 50), point(Some(5.0), None, 3), point(Some(9.0), None, 7)];
        assert_eq!(classify(&curve, ScreenThresholds::default()), Verdict::DropThin { best_n_closed: 7 });
    }

    #[test]
    fn unrankable_off_pick_has_nothing_to_measure_against() {
        let curve = vec![point(None, None, 2), point(Some(5.0), Some(50.0), 50)];
        assert_eq!(classify(&curve, ScreenThresholds::default()), Verdict::DropNoBaseline);
    }

    // ── end-to-end ─────────────────────────────────────────────────────────

    /// The screen orchestration end-to-end over a synthetic cohort: every screened
    /// metric gets a response, nothing is dropped without a reason, and the shortlist
    /// is ranked by lift.
    #[test]
    fn run_screen_reports_every_metric_with_a_reason() {
        let c = corpus(12);
        let cfg = ScreenConfig::default();
        let report = run_screen(
            &c,
            &cfg,
            baseline(),
            pricing(),
            Utc::now(),
            DiscoveryWeights { min_closed: 1, ..DiscoveryWeights::default() },
            ScreenThresholds::default(),
            &NoopObserver,
        )
        .unwrap();

        assert_eq!(report.cohort_tokens, 12);
        // The reference line is the bare bracket — hoisted, not re-simulated, so it is
        // exactly pick 0 of the first segment.
        let stats = report.baseline_stats.as_ref().expect("a screened run has a baseline");
        let first = &report.responses[0].curve[0];
        assert_eq!(first.value, None);
        assert_eq!(stats.metrics.n_fired, first.n_fired);
        assert_eq!(stats.score(), first.outcome.score());
        let plan = screen_plan(&cfg);
        // Every screened metric is either a response, a menu gap, or a segment error.
        for m in &plan.metrics {
            let responded = report.responses.iter().any(|r| r.metric == *m);
            let gapped = report.gaps.iter().any(|(g, _)| g == m);
            let errored = report.errors.iter().any(|e| e.metric == *m);
            assert!(
                responded || gapped || errored,
                "{:?} on {:?} vanished from the report",
                m.metric,
                m.side
            );
        }
        // Combo budget is additive: one combo per pick, nothing multiplicative.
        let expected: usize = report
            .responses
            .iter()
            .map(|r| r.curve.len())
            .sum();
        assert_eq!(report.combos_scanned, expected);
        // The shortlist is ranked by lift, descending.
        let lifts: Vec<f64> = report.shortlisted().filter_map(MetricResponse::lift).collect();
        assert!(lifts.windows(2).all(|w| w[0] >= w[1]), "shortlist not ranked: {lifts:?}");
        // Declared position menus are exit-only and must be screened here too.
        assert!(report
            .responses
            .iter()
            .any(|r| r.metric.group == MetricGroupId::Position && r.metric.side == AxisSide::Exit)
            || report.gaps.iter().any(|(m, _)| m.group == MetricGroupId::Position));
        // Position menus are declared, never measured.
        for r in &report.responses {
            if r.metric.group == MetricGroupId::Position {
                assert!(matches!(r.metric.source, ValueSource::Declared(_)));
            }
        }
    }
}
