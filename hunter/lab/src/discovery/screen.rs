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
//! Discovery is scan/precompute-bound, not fold-bound (plan §6.1): the wall-clock is
//! the corpus load plus the per-token [`MetricSeries`] build, not the handful of
//! combos. N separate `run_grouped` calls would rebuild that series N times. Instead
//! [`ScreenStrategy`] presents the N per-metric sub-models as **one flat combo space**
//! over **one shared precompute** ([`GenericSweepStrategy::share_precompute`]): the
//! sweep engine builds each token's series once over the union of every screened
//! metric's columns, and each segment's combos scan it. That saves N−1 of the
//! expensive passes and reuses the engine's RAM ladder / wave driver / cancellation
//! verbatim — it is a scan mode, not a second engine.
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
use hunter_engine::metrics::series::SeriesColumn;
use hunter_engine::metrics::Ts;

use crate::sweep::aggregate::ComboMetrics;
use crate::sweep::corpus::Corpus;
use crate::sweep::engine::{combo_batch_size, run_sweep};
use crate::sweep::generic::axes::{AxesModel, AxesRequest, AxisSpec, ResolvedAxis};
use crate::sweep::generic::strategy::{GenericCombo, SparseGrid};
use crate::sweep::generic::{GenericSweepStrategy, Pricing};
use crate::sweep::progress::SweepObserver;
use crate::sweep::strategy::{ParamSpace, Strategy, SweepMethod, TokenOutcome};

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
pub struct ScreenSegment {
    pub metric: ScreenMetric,
    pub operator: Operator,
    /// The swept values in combo order, read back off the **resolved** axis so the
    /// curve can never drift from what the sweep actually ran. `values[0]` is the
    /// `off` sentinel (`None`).
    pub values: Vec<Option<f64>>,
    strategy: GenericSweepStrategy,
}

impl ScreenSegment {
    /// Build one segment's sub-model: the metric axis (menu incl. `off`) plus the
    /// run's fixed TP/SL.
    fn build(
        menu: &MetricCandidates,
        operator: Operator,
        baseline: &ScreenBaseline,
        pricing: Pricing,
        as_of: Ts,
        flow: Option<&hunter_engine::metrics::flow_split::FlowPatterns>,
    ) -> Result<Self, String> {
        let mut axes = vec![menu.axis_spec(operator)];
        axes.extend(baseline.axes());
        let model = AxesModel::resolve(&AxesRequest { axes })?;
        let values = model
            .axes
            .iter()
            .find_map(|a| match a {
                ResolvedAxis::Metric { values, .. } => Some(values.clone()),
                _ => None,
            })
            .ok_or_else(|| "segment lost its metric axis".to_string())?;
        Ok(Self {
            metric: menu.metric,
            operator,
            values,
            strategy: GenericSweepStrategy::new(model, pricing, as_of, flow.cloned()),
        })
    }
}

/// A registry metric whose menu existed but whose sub-model could not be resolved
/// (a malformed axis) — reported, never silently dropped.
#[derive(Clone, Debug, PartialEq)]
pub struct SegmentError {
    pub metric: ScreenMetric,
    pub operator: Operator,
    pub error: String,
}

// ───────────────────────────── the scan mode ───────────────────────────────

/// One combo of the additive screen: which segment, and which pick on that segment's
/// menu. Index-only and `Copy`, exactly like [`GenericCombo`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScreenCombo {
    pub segment: u32,
    pub pick: u32,
}

/// The additive screen as one sweep [`Strategy`]: N per-metric sub-strategies over a
/// **shared** per-token precompute.
///
/// Every segment is widened to the union of all segments' columns and the widest grid
/// horizons ([`GenericSweepStrategy::share_precompute`]), so `prepare_token` on any one
/// of them produces a series every segment can scan. The engine therefore builds each
/// token's `MetricSeries` exactly once for the whole screen — the dominant cost, paid
/// once instead of N times.
pub struct ScreenStrategy {
    segments: Vec<ScreenSegment>,
}

impl ScreenStrategy {
    /// Widen every segment onto one shared precompute and take ownership of them.
    pub fn new(mut segments: Vec<ScreenSegment>) -> Self {
        let mut columns: Vec<SeriesColumn> = Vec::new();
        let mut grid = SparseGrid::default();
        for s in &segments {
            for c in s.strategy.columns() {
                if !columns.contains(c) {
                    columns.push(*c);
                }
            }
            let g = s.strategy.grid();
            grid.max_window_secs = grid.max_window_secs.max(g.max_window_secs);
            grid.time_horizon_secs = grid.time_horizon_secs.max(g.time_horizon_secs);
            grid.stall_horizon_secs = grid.stall_horizon_secs.max(g.stall_horizon_secs);
        }
        for s in &mut segments {
            s.strategy.share_precompute(columns.clone(), grid);
        }
        Self { segments }
    }

    pub fn segments(&self) -> &[ScreenSegment] {
        &self.segments
    }

    /// The flat combo list, in `(segment, pick)` order. Also the order every result
    /// row comes back in, so `combo_id` indexes straight into it.
    pub fn combos(&self) -> Vec<ScreenCombo> {
        let mut out = Vec::new();
        for (s, seg) in self.segments.iter().enumerate() {
            for pick in 0..seg.values.len() {
                out.push(ScreenCombo { segment: s as u32, pick: pick as u32 });
            }
        }
        out
    }

    fn seg(&self, c: &ScreenCombo) -> &ScreenSegment {
        &self.segments[c.segment as usize]
    }

    fn inner(&self, c: &ScreenCombo) -> (&GenericSweepStrategy, GenericCombo) {
        let seg = self.seg(c);
        (&seg.strategy, GenericCombo { idx: c.pick as usize })
    }
}

impl ParamSpace for ScreenStrategy {
    type Params = ScreenCombo;

    /// Always the full additive set. Sampling a screen makes no sense: the point is
    /// that it is already `Σ menu_len` combos, and dropping menu entries would punch
    /// holes in the response curves the verdict reads.
    fn sample(&self, _method: SweepMethod) -> Vec<Self::Params> {
        self.combos()
    }

    // No `order_for_entry_cache` override — and that is deliberate. Within a segment
    // an entry-side menu is the only entry axis (so each pick is its own contiguous
    // one-combo block) and an exit-side menu leaves the entry key constant across the
    // whole segment. Flat order is therefore already entry-contiguous, and leaving it
    // alone keeps `combo_id == flat index`, which is what lets the report map rows
    // back to `(segment, pick)` without a lookup table.
}

impl Strategy for ScreenStrategy {
    type Entry = <GenericSweepStrategy as Strategy>::Entry;
    /// Segment-qualified: two segments' sub-keys are unrelated, so a bare sub-key
    /// would let one segment's cached entry be served to another's combo.
    type EntryKey = (u32, <GenericSweepStrategy as Strategy>::EntryKey);
    type TokenState = <GenericSweepStrategy as Strategy>::TokenState;
    type BoundParams = <GenericSweepStrategy as Strategy>::BoundParams;
    type ExitCtx = <GenericSweepStrategy as Strategy>::ExitCtx;

    fn entry_key(&self, p: &Self::Params) -> Self::EntryKey {
        let (s, c) = self.inner(p);
        (p.segment, s.entry_key(&c))
    }

    fn bind_param(&self, p: &Self::Params) -> Self::BoundParams {
        let (s, c) = self.inner(p);
        s.bind_param(&c)
    }

    /// One series for the whole screen — every segment shares the same columns/grid,
    /// so segment 0 speaks for all of them.
    fn prepare_token(&self, token: &crate::sweep::corpus::CorpusToken) -> Self::TokenState {
        self.segments[0].strategy.prepare_token(token)
    }

    fn build_exit_ctx(
        &self,
        trades: &[crate::sweep::projection::CorpusTrade],
        state: &Self::TokenState,
        bound: &Self::BoundParams,
        entry: &Self::Entry,
        p: &Self::Params,
        ctx: &mut Self::ExitCtx,
    ) {
        let (s, c) = self.inner(p);
        s.build_exit_ctx(trades, state, bound, entry, &c, ctx);
    }

    fn resolve_entry(
        &self,
        trades: &[crate::sweep::projection::CorpusTrade],
        state: &Self::TokenState,
        bound: &Self::BoundParams,
        p: &Self::Params,
    ) -> Self::Entry {
        let (s, c) = self.inner(p);
        s.resolve_entry(trades, state, bound, &c)
    }

    fn resolve_exit(
        &self,
        trades: &[crate::sweep::projection::CorpusTrade],
        state: &Self::TokenState,
        bound: &Self::BoundParams,
        entry: &Self::Entry,
        p: &Self::Params,
        ctx: &Self::ExitCtx,
    ) -> TokenOutcome {
        let (s, c) = self.inner(p);
        s.resolve_exit(trades, state, bound, entry, &c, ctx)
    }

    fn params_json(&self, p: &Self::Params) -> serde_json::Value {
        let (s, c) = self.inner(p);
        s.params_json(&c)
    }

    fn token_state_bytes_estimate(&self, token: &crate::sweep::corpus::CorpusToken) -> usize {
        self.segments[0].strategy.token_state_bytes_estimate(token)
    }
}

// ───────────────────────────── response curves ─────────────────────────────

/// One point on a metric's response curve: the value swept and what it scored.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResponsePoint {
    /// `None` = the `off` pick (the baseline this metric is measured against).
    pub value: Option<f64>,
    pub outcome: ScoreOutcome,
    pub n_fired: u64,
    pub n_closed: u64,
}

impl ResponsePoint {
    fn score(&self) -> Option<f64> {
        self.outcome.score()
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
}

/// The Layer-1 deliverable: a ranked metric shortlist plus everything that did not
/// make it, with reasons.
#[derive(Clone, Debug)]
pub struct ScreenReport {
    /// Tokens in the cohort — the `fire_rate` denominator every score used.
    pub cohort_tokens: usize,
    pub baseline: ScreenBaseline,
    pub weights: DiscoveryWeights,
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
            match ScreenSegment::build(
                menu,
                *op,
                &baseline,
                pricing,
                as_of,
                cfg.flow_patterns.as_ref(),
            ) {
                Ok(seg) => segments.push(seg),
                Err(error) => {
                    errors.push(SegmentError { metric: menu.metric, operator: *op, error })
                }
            }
        }
    }

    let matched = corpus.token_count() as u64;
    if segments.is_empty() {
        return Ok(ScreenReport {
            cohort_tokens: corpus.token_count(),
            baseline,
            weights,
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

    let strategy = ScreenStrategy::new(segments);
    let params = strategy.sample(SweepMethod::Grid);
    let threads = rayon::current_num_threads().max(1);
    let batch = combo_batch_size(params.len(), threads);
    observer.set_total(corpus.token_count(), params.len());
    let (_stats, metrics) = run_sweep(&strategy, &params, corpus, observer, batch)?;

    let mut responses = Vec::with_capacity(strategy.segments().len());
    let mut n_gated = 0usize;
    let mut base = 0usize;
    for seg in strategy.segments() {
        let rows = &metrics[base..base + seg.values.len()];
        let (r, gated) = analyse(seg, rows, matched, weights, thresholds);
        n_gated += gated;
        responses.push(r);
        base += seg.values.len();
    }

    let mut shortlist: Vec<usize> = (0..responses.len()).filter(|i| responses[*i].verdict.is_keep()).collect();
    shortlist.sort_by(|a, b| {
        let la = responses[*a].lift().unwrap_or(f64::NEG_INFINITY);
        let lb = responses[*b].lift().unwrap_or(f64::NEG_INFINITY);
        lb.partial_cmp(&la).unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(ScreenReport {
        cohort_tokens: corpus.token_count(),
        baseline,
        weights,
        thresholds,
        responses,
        shortlist,
        skipped: plan.skipped.clone(),
        gaps,
        errors,
        percentiles,
        n_gated,
        combos_scanned: params.len(),
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
        curve.push(ResponsePoint {
            value: seg.values[pick],
            outcome,
            n_fired: m.n_fired,
            n_closed: m.n_closed,
        });
    }
    let verdict = classify(&curve, th);
    let baseline = curve.first().and_then(ResponsePoint::score);
    (MetricResponse { metric: seg.metric, operator: seg.operator, curve, baseline, verdict }, gated)
}

/// Classify a response curve. `curve[0]` is the `off` pick; `curve[1..]` the values
/// ascending.
fn classify(curve: &[ResponsePoint], th: ScreenThresholds) -> Verdict {
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
    let (best_i, best) = ranked
        .iter()
        .copied()
        .fold((0usize, f64::NEG_INFINITY), |acc, x| if x.1 > acc.1 { x } else { acc });
    let best_value = picks[best_i].value.unwrap_or(f64::NAN);

    let lift = best - baseline;
    let required = (th.min_lift_ratio * baseline.abs()).max(th.min_abs_lift);
    if lift <= required || best <= 0.0 {
        return Verdict::DropNoEdge { best_lift: lift };
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
    use crate::sweep::progress::NoopObserver;
    use crate::sweep::projection::CorpusTrade;
    use chrono::{Duration, TimeZone, Utc};
    use hunter_engine::metrics::{MetricGroupId, MetricId};
    use std::sync::Arc;
    use trading_core::strategies::kernel::CostModel;
    use trading_core::strategies::paper_fill::FillModel;

    use super::super::candidates::{screen_plan, ValueSource};
    use crate::sweep::corpus::CorpusToken;
    use crate::sweep::generic::axes::AxisSide;

    fn pricing() -> Pricing {
        Pricing {
            buy_amount_sol: 1.0,
            fill_model: FillModel::FirstInWindow,
            cost: CostModel::pumpfun_fee_only(),
        }
    }

    fn baseline() -> ScreenBaseline {
        ScreenBaseline { take_profit_pct: Some(30.0), stop_loss_pct: Some(15.0) }
    }

    fn point(value: Option<f64>, score: Option<f64>, n_closed: u64) -> ResponsePoint {
        ResponsePoint {
            value,
            outcome: match score {
                Some(s) => ScoreOutcome::Ranked(s),
                None => ScoreOutcome::BelowMinClosed { n_closed },
            },
            n_fired: n_closed,
            n_closed,
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

    // ── the scan mode ──────────────────────────────────────────────────────

    fn trade(secs: i64, price: f64, reserve: f64, is_buy: bool) -> CorpusTrade {
        CorpusTrade {
            block_time: Utc.timestamp_opt(1_700_000_000, 0).unwrap() + Duration::seconds(secs),
            amount_sol: 1.0,
            token_amount: 1.0,
            price_per_token: price,
            reserve_sol: Some(reserve),
            reserve_token: Some(1_000.0),
            real_reserve_sol: Some(reserve),
            real_token_reserves: None,
            slot: secs as u64,
            leg_index: 0,
            is_buy,
            tx_signature: None,
            ix_labels: None,
            wallet: None,
        }
    }

    /// A token that rises then falls, so TP and SL both get exercised.
    fn token(mint: &str, n: usize) -> CorpusToken {
        let created = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let trades: Vec<CorpusTrade> = (0..n)
            .map(|i| {
                let f = i as f64;
                let price = if i < n / 2 { 1.0 + f * 0.2 } else { 1.0 + (n as f64 - f) * 0.1 };
                trade(i as i64 * 5, price, 40.0 + f, i % 3 != 0)
            })
            .collect();
        CorpusToken {
            mint: mint.into(),
            symbol: mint.into(),
            created_at: created,
            trades: Arc::new(trades),
            fp: Default::default(),
        }
    }

    fn corpus(n_tokens: usize) -> Corpus {
        Corpus {
            tokens: (0..n_tokens).map(|i| token(&format!("m{i}"), 20 + i % 7)).collect(),
            hash: "screen-test".into(),
            has_fingerprints: false,
            candidates_capped: false,
        }
    }

    fn segment_for(metric: MetricId, side: AxisSide, values: Vec<f64>) -> ScreenSegment {
        let cfg = ScreenConfig::default();
        let plan = screen_plan(&cfg);
        let m = plan
            .metrics
            .iter()
            .find(|m| m.metric == metric && m.side == side)
            .copied()
            .expect("metric screened");
        let menu = MetricCandidates {
            metric: m,
            operators: vec![Operator::Gte],
            values: std::iter::once(None).chain(values.into_iter().map(Some)).collect(),
            anchors: Vec::new(),
        };
        ScreenSegment::build(
            &menu,
            Operator::Gte,
            &baseline(),
            pricing(),
            Utc::now(),
            None,
        )
        .expect("segment resolves")
    }

    /// The shared-precompute contract: every segment widens onto ONE column union +
    /// grid, so the engine builds a token's series once for the whole screen.
    #[test]
    fn segments_share_one_precompute() {
        let a = segment_for(MetricId::Time, AxisSide::Entry, vec![5.0, 30.0]);
        let b = segment_for(MetricId::Liquidity, AxisSide::Entry, vec![40.0, 50.0]);
        let strat = ScreenStrategy::new(vec![a, b]);
        let cols: Vec<_> = strat.segments().iter().map(|s| s.strategy.columns().to_vec()).collect();
        assert_eq!(cols[0], cols[1], "segments must share the column union");
        assert!(cols[0].contains(&SeriesColumn::Static(MetricId::Time)));
        assert!(cols[0].contains(&SeriesColumn::Static(MetricId::Liquidity)));
        // The union grid takes the widest horizon of either segment.
        let grids: Vec<_> = strat.segments().iter().map(|s| s.strategy.grid()).collect();
        assert_eq!(grids[0].time_horizon_secs, grids[1].time_horizon_secs);
        assert!(grids[0].time_horizon_secs >= 30.0);
    }

    /// The additive budget: combos are `Σ menu_len`, never the product.
    #[test]
    fn combo_space_is_additive() {
        let a = segment_for(MetricId::Time, AxisSide::Entry, vec![5.0, 30.0, 60.0]);
        let b = segment_for(MetricId::Liquidity, AxisSide::Entry, vec![40.0, 50.0]);
        let strat = ScreenStrategy::new(vec![a, b]);
        // (off + 3) + (off + 2) = 7, not 4 × 3 = 12.
        assert_eq!(strat.combos().len(), 7);
        let combos = strat.combos();
        assert_eq!(combos[0], ScreenCombo { segment: 0, pick: 0 });
        assert_eq!(combos[4], ScreenCombo { segment: 1, pick: 0 });
    }

    /// Entry keys must be segment-qualified — otherwise the engine's single-slot
    /// entry cache can hand one segment's resolved entry to another segment's combo.
    #[test]
    fn entry_keys_are_segment_qualified() {
        let a = segment_for(MetricId::Time, AxisSide::Entry, vec![5.0, 30.0]);
        let b = segment_for(MetricId::Liquidity, AxisSide::Entry, vec![40.0, 50.0]);
        let strat = ScreenStrategy::new(vec![a, b]);
        let k0 = strat.entry_key(&ScreenCombo { segment: 0, pick: 1 });
        let k1 = strat.entry_key(&ScreenCombo { segment: 1, pick: 1 });
        assert_ne!(k0, k1, "same sub-key on two segments must not collide");
    }

    /// End-to-end: the additive sweep must produce exactly `Σ menu_len` rows, in
    /// `(segment, pick)` order, and each segment's `off` pick must be the same
    /// baseline rule for every segment on the same side.
    #[test]
    fn additive_sweep_returns_one_row_per_pick_in_order() {
        let a = segment_for(MetricId::Time, AxisSide::Entry, vec![5.0, 30.0]);
        let b = segment_for(MetricId::Liquidity, AxisSide::Entry, vec![40.0, 50.0]);
        let strat = ScreenStrategy::new(vec![a, b]);
        let params = strat.sample(SweepMethod::Grid);
        let c = corpus(6);
        let (stats, metrics) =
            run_sweep(&strat, &params, &c, &NoopObserver, params.len()).unwrap();
        assert_eq!(metrics.len(), params.len());
        assert_eq!(stats.rows, (c.token_count() * params.len()) as u64);
        for (i, m) in metrics.iter().enumerate() {
            assert_eq!(m.combo_id, i as u32, "combo_id must index the flat combo list");
        }
        // Both segments' `off` picks are the bare baseline rule ⇒ identical results.
        assert_eq!(metrics[0].n_fired, metrics[3].n_fired);
        assert_eq!(metrics[0].total_pnl_sol, metrics[3].total_pnl_sol);
        // A `time >= 30` gate can only fire on a subset of what `off` fires on.
        assert!(metrics[2].n_fired <= metrics[0].n_fired);
    }

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
