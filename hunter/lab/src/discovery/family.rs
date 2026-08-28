//! **Layer 2 — the family grid + interaction check + joint grids** (plan §3):
//! combine only the metrics Layer 1 kept, and spend the combo budget on metrics
//! that actually *interact* instead of on a blind cross-product.
//!
//! ## Why families
//!
//! Metrics interact strongly **within** a family (they are different views of one
//! underlying quantity — lifetime vs rolling vs since-entry price, say) and largely
//! compose **across** families. The families are registry data
//! ([`MetricFamily`](hunter_engine::metrics::MetricFamily)), not a lab-side map, so a
//! group added later lands in a family with no edit here — the extensibility contract
//! (plan §5, decision D3).
//!
//! ## What runs
//!
//! 1. **A grid per family**, over the *narrowed* 2–3 value ranges Layer 1 emitted
//!    ([`Verdict::Keep`]), each axis keeping its `off` sentinel so the grid still
//!    reads with-vs-without per member. Small by construction; bounded explicitly by
//!    [`FamilyLimits`], and every member a bound drops is reported.
//! 2. **A pairwise interaction check**: fix family A at its own best combo, re-sweep
//!    family B on top. If B's best picks are unchanged the families are independent
//!    and their grids compose; if B's best moves, they interact.
//! 3. **Joint grids (L2b)**: connected components of undirected `Interacting` pairs
//!    are product-gridded under the same [`FamilyLimits`] — the measurement that used
//!    to be advisory-only now actually runs, and those winners feed Layer 3 + the
//!    sweep seed.
//!
//! Phases 1–3 ride the [`additive`](super::additive) scan mode (one shared
//! precompute per phase). Layer 2 models are built from layer 1 winners; layer 3
//! models from the interacting components.

use anyhow::Result;
use hunter_engine::metrics::evaluator::Operator;
use hunter_engine::metrics::{group_spec, MetricFamily};

use crate::sweep::aggregate::ComboMetrics;
use crate::sweep::corpus::Corpus;
use crate::sweep::generic::axes::{AxesModel, AxesRequest, AxisSpec, ResolvedAxis, WindowField};
use crate::sweep::generic::Pricing;
use crate::sweep::progress::SweepObserver;

use hunter_engine::metrics::Ts;

use super::additive::AdditiveStrategy;
use super::candidates::{ScreenConfig, ScreenMetric};
use super::objective::{discovery_score, ComboStats, DiscoveryWeights, ScoreOutcome};
use super::screen::{classify, MetricResponse, ResponsePoint, ScreenBaseline, ScreenReport, Verdict};

// ───────────────────────────── bounds ──────────────────────────────────────

/// Explicit bounds on how big a family grid may get. Layer 1 can shortlist more
/// members than a grid can afford (`values^members` grows fast), so the excess is
/// **dropped by lowest Layer-1 lift and reported** — never silently truncated.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FamilyLimits {
    /// Max axes in one family grid.
    pub max_axes: usize,
    /// Max combos in one family grid, after the axis cap.
    pub max_combos: usize,
    /// How many Layer-1 rejects the synergy rescue may re-screen. `0` disables the
    /// rescue pass entirely.
    pub rescue_cap: usize,
}

impl Default for FamilyLimits {
    fn default() -> Self {
        // 4 axes × (off + 3 narrowed values) = 256 worst case — well inside the
        // "small family grids" budget of plan §6 and orders below `MAX_COMBOS`.
        // 8 rescue attempts × a ~6-value menu is one more additive pass of ~48 combos.
        Self { max_axes: 4, max_combos: 1_024, rescue_cap: 8 }
    }
}

/// One shortlisted `(metric, operator)` carried into a family (or joint) grid.
#[derive(Clone, Debug, PartialEq)]
pub struct FamilyMember {
    pub metric: ScreenMetric,
    pub operator: Operator,
    /// The narrowed candidate values from Layer 1 (the `off` sentinel is added by the
    /// axis, not stored here).
    pub values: Vec<f64>,
    /// Marginal value over its own `off` — the ranking + capping key. For a rescued
    /// member this is the lift measured *given the pinned winner*, which is the only
    /// number under which it has any lift at all.
    pub lift: f64,
    /// True when Layer 1 dropped this metric and the synergy rescue brought it back.
    /// Carried so a report never presents a conditional edge as a standalone one.
    pub rescued: bool,
}

impl FamilyMember {
    fn axis(&self) -> AxisSpec {
        AxisSpec {
            kind: "metric".to_string(),
            side: Some(self.metric.side),
            group: Some(group_spec(self.metric.group).name.to_string()),
            metric: Some(self.metric.metric.name().to_string()),
            operator: Some(self.operator),
            window: self.metric.window.map(|w| WindowField::Span(w.label())),
            slice: None,
            values: std::iter::once(None).chain(self.values.iter().copied().map(Some)).collect(),
        }
    }

    /// A single-valued axis pinning this member at `value` — how a family is "held
    /// fixed" during the interaction check. `None` ⇒ the member was off in the best
    /// combo, so it contributes no axis at all.
    fn pinned_axis(&self, value: Option<f64>) -> Option<AxisSpec> {
        let v = value?;
        let mut a = self.axis();
        a.values = vec![Some(v)];
        Some(a)
    }
}

/// Why a shortlisted member did not make it into its family's grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropReason {
    /// Over [`FamilyLimits::max_axes`].
    AxisCap,
    /// The grid would have exceeded [`FamilyLimits::max_combos`].
    ComboCap,
}

// ───────────────────────────── results ─────────────────────────────────────

/// The winning combo of one grid.
#[derive(Clone, Debug, PartialEq)]
pub struct BestCombo {
    /// Flat combo index within its own sub-model.
    pub idx: usize,
    /// The value each **metric** axis picked, in model-axis order (`None` = `off`).
    /// This is what the interaction check compares.
    pub picks: Vec<Option<f64>>,
    pub score: f64,
    pub n_fired: u64,
    pub n_closed: u64,
    /// Canonical `RuleParams` JSON — the promote / `simulate_one_combo` handoff.
    pub params_json: serde_json::Value,
}

/// One family's grid outcome.
#[derive(Clone, Debug)]
pub struct FamilyResult {
    pub family: MetricFamily,
    /// Members actually gridded, best Layer-1 lift first (also the axis order).
    pub members: Vec<FamilyMember>,
    /// Members a bound removed, with the reason — no silent caps.
    pub dropped: Vec<(FamilyMember, DropReason)>,
    pub combos: usize,
    /// `None` when every combo was min-N gated or nothing fired.
    pub best: Option<BestCombo>,
    /// Combos the min-N gate removed in this grid.
    pub n_gated: usize,
}

/// Whether two families had to be gridded jointly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InteractionVerdict {
    /// B's best picks are the same with A pinned as without — the two compose, so
    /// keep the grids separate.
    Independent,
    /// B's best picks moved once A was pinned — grid them jointly.
    Interacting,
    /// B had no rankable combo under A, so nothing can be concluded.
    Inconclusive,
}

/// One ordered `(A pinned, B swept)` measurement.
#[derive(Clone, Debug)]
pub struct Interaction {
    pub pinned: MetricFamily,
    pub swept: MetricFamily,
    /// B's best picks standing alone (from its own family grid).
    pub alone: Vec<Option<f64>>,
    /// B's best picks with A pinned at A's best.
    pub given: Vec<Option<f64>>,
    pub score_alone: f64,
    /// `None` when nothing under A was rankable.
    pub score_given: Option<f64>,
    pub verdict: InteractionVerdict,
}

/// One joint-cluster grid outcome (L2b) — the product over an interacting
/// connected component of families.
#[derive(Clone, Debug)]
pub struct JointResult {
    /// Families in this connected component (stable: registry order of first appearance).
    pub families: Vec<MetricFamily>,
    /// Members actually gridded (union of component families, lift-desc, then capped).
    pub members: Vec<FamilyMember>,
    /// Members a bound removed, with the reason — no silent caps.
    pub dropped: Vec<(FamilyMember, DropReason)>,
    pub combos: usize,
    /// `None` when every combo was min-N gated or nothing fired.
    pub best: Option<BestCombo>,
    pub n_gated: usize,
}

/// One Layer-1 reject re-screened against a pinned winner — the synergy rescue.
///
/// Layer 1 is univariate by construction: it can only ever see a metric's *standalone*
/// lift, so the second half of a pair that only works together never reaches a grid.
/// This is that blind spot's repair, and it is deliberately conditional — a rescued
/// metric earns its axis **given** the pin, never on its own.
#[derive(Clone, Debug)]
pub struct Rescue {
    pub metric: ScreenMetric,
    pub operator: Operator,
    /// The family whose winner was pinned while this metric was re-swept.
    pub pinned: MetricFamily,
    /// The pinned-alone score — the `off` pick of the rescue's own curve, and the
    /// baseline its lift is measured against.
    pub pinned_score: f64,
    /// Whether the re-screen produced a keep-grade curve, and if not, why. Reported
    /// either way: an attempted-and-refused rescue is a finding, not a non-event.
    pub verdict: Verdict,
}

impl Rescue {
    /// The lift this metric showed given the pin, when the rescue succeeded.
    pub fn lift(&self) -> Option<f64> {
        match self.verdict {
            Verdict::Keep { lift, .. } => Some(lift),
            _ => None,
        }
    }
}

/// The Layer-2 deliverable: per-family winners, the interaction map, and joint grids.
#[derive(Clone, Debug)]
pub struct FamilyReport {
    pub cohort_tokens: usize,
    pub baseline: ScreenBaseline,
    pub limits: FamilyLimits,
    pub families: Vec<FamilyResult>,
    pub interactions: Vec<Interaction>,
    /// Joint grids over interacting connected components (empty when none interact).
    pub joints: Vec<JointResult>,
    /// Every synergy-rescue attempt, successful or not.
    pub rescues: Vec<Rescue>,
    /// Combos scanned across all phases (the honest budget).
    pub combos_scanned: usize,
}

impl FamilyReport {
    /// The family pairs that must be gridded jointly.
    pub fn interacting_pairs(&self) -> impl Iterator<Item = (MetricFamily, MetricFamily)> + '_ {
        self.interactions
            .iter()
            .filter(|i| i.verdict == InteractionVerdict::Interacting)
            .map(|i| (i.pinned, i.swept))
    }
}

/// Undirected connected components of `Interacting` pairs. Singleton families and
/// Independent/Inconclusive edges are ignored — only edges that force a joint grid.
pub fn interacting_components(interactions: &[Interaction]) -> Vec<Vec<MetricFamily>> {
    let mut nodes: Vec<MetricFamily> = Vec::new();
    let mut edges: Vec<(MetricFamily, MetricFamily)> = Vec::new();
    for i in interactions {
        if i.verdict != InteractionVerdict::Interacting {
            continue;
        }
        // Undirected: store once with sorted endpoints so A↔B collapses.
        let (a, b) = if i.pinned.as_str() <= i.swept.as_str() {
            (i.pinned, i.swept)
        } else {
            (i.swept, i.pinned)
        };
        if a == b {
            continue;
        }
        if !nodes.contains(&a) {
            nodes.push(a);
        }
        if !nodes.contains(&b) {
            nodes.push(b);
        }
        if !edges.contains(&(a, b)) {
            edges.push((a, b));
        }
    }
    if nodes.is_empty() {
        return Vec::new();
    }

    // BFS components.
    let mut seen = vec![false; nodes.len()];
    let mut out: Vec<Vec<MetricFamily>> = Vec::new();
    for start in 0..nodes.len() {
        if seen[start] {
            continue;
        }
        let mut stack = vec![start];
        seen[start] = true;
        let mut comp = Vec::new();
        while let Some(i) = stack.pop() {
            comp.push(nodes[i]);
            for &(a, b) in &edges {
                let (ai, bi) = (
                    nodes.iter().position(|n| *n == a).unwrap(),
                    nodes.iter().position(|n| *n == b).unwrap(),
                );
                if ai == i && !seen[bi] {
                    seen[bi] = true;
                    stack.push(bi);
                } else if bi == i && !seen[ai] {
                    seen[ai] = true;
                    stack.push(ai);
                }
            }
        }
        if comp.len() >= 2 {
            comp.sort_by_key(|f| f.as_str());
            out.push(comp);
        }
    }
    out.sort_by(|a, b| a[0].as_str().cmp(b[0].as_str()));
    out
}

/// Plan one joint grid over the union of `component` families' members.
pub fn plan_joint(
    families: &[FamilyResult],
    component: &[MetricFamily],
    limits: FamilyLimits,
) -> JointResult {
    let mut members: Vec<FamilyMember> = Vec::new();
    for fam in component {
        if let Some(f) = families.iter().find(|f| f.family == *fam) {
            members.extend(f.members.iter().cloned());
        }
    }
    let (members, dropped) = cap_members(members, limits);
    let combos = grid_combos(&members);
    JointResult {
        families: component.to_vec(),
        members,
        dropped,
        combos,
        best: None,
        n_gated: 0,
    }
}

// ───────────────────────────── the driver ──────────────────────────────────

/// Group Layer 1's shortlist into families, honouring [`FamilyLimits`].
pub fn plan_families(report: &ScreenReport, limits: FamilyLimits) -> Vec<FamilyResult> {
    let mut by_family: Vec<(MetricFamily, Vec<FamilyMember>)> = Vec::new();
    for r in report.shortlisted() {
        let Verdict::Keep { lift, ref narrowed, .. } = r.verdict else { continue };
        if narrowed.is_empty() {
            continue;
        }
        let member = FamilyMember {
            metric: r.metric,
            operator: r.operator,
            values: narrowed.clone(),
            lift,
            rescued: false,
        };
        let fam = group_spec(r.metric.group).family;
        match by_family.iter_mut().find(|(f, _)| *f == fam) {
            Some((_, v)) => v.push(member),
            None => by_family.push((fam, vec![member])),
        }
    }

    by_family
        .into_iter()
        .map(|(family, members)| {
            let (members, dropped) = cap_members(members, limits);
            let combos = grid_combos(&members);
            FamilyResult { family, members, dropped, combos, best: None, n_gated: 0 }
        })
        .collect()
}

/// Order members by lift and enforce [`FamilyLimits`], reporting every drop.
///
/// The sort is not defensive dressing: it decides the axis order and therefore *which*
/// member a cap removes, so it must never depend on the caller's input order. One
/// implementation for family grids, joint grids, and the post-rescue re-grid — three
/// call sites that must cap identically or a rescued member could survive in one grid
/// and vanish from another.
fn cap_members(
    mut members: Vec<FamilyMember>,
    limits: FamilyLimits,
) -> (Vec<FamilyMember>, Vec<(FamilyMember, DropReason)>) {
    members.sort_by(|a, b| b.lift.partial_cmp(&a.lift).unwrap_or(std::cmp::Ordering::Equal));
    let mut dropped = Vec::new();
    while members.len() > limits.max_axes.max(1) {
        dropped.push((members.pop().expect("non-empty"), DropReason::AxisCap));
    }
    while members.len() > 1 && grid_combos(&members) > limits.max_combos.max(1) {
        dropped.push((members.pop().expect("non-empty"), DropReason::ComboCap));
    }
    (members, dropped)
}

/// Combos a family grid would produce: `Π (off + narrowed)` over its members. The
/// baseline TP/SL axes are single-valued and contribute a factor of 1.
fn grid_combos(members: &[FamilyMember]) -> usize {
    members.iter().fold(1usize, |n, m| n.saturating_mul(m.values.len() + 1))
}

/// Run Layer 2 over a completed Layer-1 [`ScreenReport`].
///
/// Layer 1 grids every family in one additive pass; phase 2 runs the pairwise
/// interaction checks in a second. Synchronous on the caller's rayon pool, same as
/// [`run_screen`](super::screen::run_screen).
#[allow(clippy::too_many_arguments)]
pub fn run_family_layer(
    corpus: &Corpus,
    cfg: &ScreenConfig,
    report: &ScreenReport,
    limits: FamilyLimits,
    pricing: Pricing,
    as_of: Ts,
    weights: DiscoveryWeights,
    observer: &dyn SweepObserver,
) -> Result<FamilyReport> {
    let baseline = report.baseline;
    baseline_ok(&baseline)?;
    let matched = corpus.token_count() as u64;
    let mut families = plan_families(report, limits);
    families.retain(|f| !f.members.is_empty());

    let mut combos_scanned = 0usize;
    if families.is_empty() {
        return Ok(FamilyReport {
            cohort_tokens: corpus.token_count(),
            baseline,
            limits,
            families,
            interactions: Vec::new(),
            joints: Vec::new(),
            rescues: Vec::new(),
            combos_scanned,
        });
    }

    // ── phase 1: one grid per family, all in one additive pass ──────────────
    let models: Vec<AxesModel> = families
        .iter()
        .map(|f| family_model(&f.members, &baseline))
        .collect::<Result<_, String>>()
        .map_err(|e| anyhow::anyhow!("family grid axes: {e}"))?;
    let grids = AdditiveStrategy::new(models, pricing, as_of, cfg.flow_patterns.as_ref());
    combos_scanned += grids.combos().len();
    let rows = grids.run(corpus, observer)?;
    for (i, fam) in families.iter_mut().enumerate() {
        let (best, gated) = best_of(&grids, i, &rows[i], matched, weights);
        fam.best = best;
        fam.n_gated = gated;
    }

    // ── phase 1b: synergy rescue ────────────────────────────────────────────
    // Layer 1 could only measure standalone lift. Pin the strongest winner so far and
    // re-screen the metrics it rejected: anything that turns keep-grade *given* the
    // pin is a conditional edge Layer 1 is structurally unable to see.
    let rescues = run_rescue(
        corpus,
        cfg,
        report,
        &families,
        limits,
        pricing,
        as_of,
        weights,
        observer,
        &mut combos_scanned,
    )?;

    // Fold successful rescues into their families and re-grid only what changed.
    let touched = adopt_rescues(&mut families, &rescues, limits);
    if !touched.is_empty() {
        let models: Vec<AxesModel> = touched
            .iter()
            .map(|i| family_model(&families[*i].members, &baseline))
            .collect::<Result<_, String>>()
            .map_err(|e| anyhow::anyhow!("rescued family grid axes: {e}"))?;
        let regrid = AdditiveStrategy::new(models, pricing, as_of, cfg.flow_patterns.as_ref());
        combos_scanned += regrid.combos().len();
        let rows = regrid.run(corpus, observer)?;
        for (slot, fam_i) in touched.iter().enumerate() {
            let (best, gated) = best_of(&regrid, slot, &rows[slot], matched, weights);
            families[*fam_i].best = best;
            families[*fam_i].n_gated = gated;
        }
    }

    // ── phase 2: pairwise interaction checks ────────────────────────────────
    // Only families that actually produced a winner can pin or be compared against.
    let live: Vec<usize> = (0..families.len()).filter(|i| families[*i].best.is_some()).collect();
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    for &a in &live {
        for &b in &live {
            if a != b {
                pairs.push((a, b));
            }
        }
    }

    let mut interactions = Vec::new();
    if !pairs.is_empty() {
        let check_models: Vec<AxesModel> = pairs
            .iter()
            .map(|(a, b)| {
                interaction_model(
                    &families[*a].members,
                    families[*a].best.as_ref().expect("live"),
                    &families[*b].members,
                    &baseline,
                )
            })
            .collect::<Result<_, String>>()
            .map_err(|e| anyhow::anyhow!("interaction axes: {e}"))?;
        let checks = AdditiveStrategy::new(check_models, pricing, as_of, cfg.flow_patterns.as_ref());
        combos_scanned += checks.combos().len();
        let check_rows = checks.run(corpus, observer)?;

        interactions.reserve(pairs.len());
        for (i, (a, b)) in pairs.iter().enumerate() {
            let b_alone = families[*b].best.as_ref().expect("live");
            let (best_given, _) = best_of(&checks, i, &check_rows[i], matched, weights);
            // The pinned family's axes come first in the model (see `interaction_model`),
            // so B's picks are the tail — compare like for like.
            let given: Vec<Option<f64>> = best_given
                .as_ref()
                .map(|c| c.picks[c.picks.len() - b_alone.picks.len()..].to_vec())
                .unwrap_or_default();
            let verdict = match &best_given {
                None => InteractionVerdict::Inconclusive,
                Some(_) if given == b_alone.picks => InteractionVerdict::Independent,
                Some(_) => InteractionVerdict::Interacting,
            };
            interactions.push(Interaction {
                pinned: families[*a].family,
                swept: families[*b].family,
                alone: b_alone.picks.clone(),
                given,
                score_alone: b_alone.score,
                score_given: best_given.as_ref().map(|c| c.score),
                verdict,
            });
        }
    }

    // ── phase 3 (L2b): joint grids over interacting connected components ────
    let components = interacting_components(&interactions);
    let mut joints: Vec<JointResult> = components
        .iter()
        .map(|c| plan_joint(&families, c, limits))
        .filter(|j| !j.members.is_empty())
        .collect();

    if !joints.is_empty() {
        let joint_models: Vec<AxesModel> = joints
            .iter()
            .map(|j| family_model(&j.members, &baseline))
            .collect::<Result<_, String>>()
            .map_err(|e| anyhow::anyhow!("joint grid axes: {e}"))?;
        let joint_grids =
            AdditiveStrategy::new(joint_models, pricing, as_of, cfg.flow_patterns.as_ref());
        combos_scanned += joint_grids.combos().len();
        let joint_rows = joint_grids.run(corpus, observer)?;
        for (i, joint) in joints.iter_mut().enumerate() {
            let (best, gated) = best_of(&joint_grids, i, &joint_rows[i], matched, weights);
            joint.best = best;
            joint.n_gated = gated;
        }
    }

    Ok(FamilyReport {
        cohort_tokens: corpus.token_count(),
        baseline,
        limits,
        families,
        interactions,
        joints,
        rescues,
        combos_scanned,
    })
}

// ───────────────────────────── synergy rescue ──────────────────────────────

/// Re-screen Layer 1's rejects with the strongest family winner pinned.
///
/// The pin is the *single* strongest winner rather than every family in turn: the
/// point is to give a conditional edge one honest chance to appear, and pinning each
/// family separately would multiply the pass by the family count for a strictly weaker
/// signal (a metric that only lifts under a weak pin is not worth an axis).
///
/// Candidates are Layer-1 drops that still carry a measurable curve —
/// `DropNoEdge` / `DropSpike` / `DropNegative`. `DropThin` and `DropNoBaseline` are
/// excluded on purpose: they failed for lack of data, and conditioning on a pin can
/// only ever remove more trades.
#[allow(clippy::too_many_arguments)]
fn run_rescue(
    corpus: &Corpus,
    cfg: &ScreenConfig,
    report: &ScreenReport,
    families: &[FamilyResult],
    limits: FamilyLimits,
    pricing: Pricing,
    as_of: Ts,
    weights: DiscoveryWeights,
    observer: &dyn SweepObserver,
    combos_scanned: &mut usize,
) -> Result<Vec<Rescue>> {
    if limits.rescue_cap == 0 {
        return Ok(Vec::new());
    }
    let Some(pin_i) = families
        .iter()
        .enumerate()
        .filter_map(|(i, f)| f.best.as_ref().map(|b| (i, b.score)))
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i)
    else {
        return Ok(Vec::new());
    };
    let pinned_family = &families[pin_i];
    let pinned_best = pinned_family.best.as_ref().expect("selected on best.is_some()");
    // A winner whose every member is `off` pins nothing — the "rescue" would just
    // re-run Layer 1 verbatim.
    if pinned_best.picks.iter().all(Option::is_none) {
        return Ok(Vec::new());
    }

    let mut candidates: Vec<&MetricResponse> = report
        .responses
        .iter()
        .filter(|r| {
            matches!(
                r.verdict,
                Verdict::DropNoEdge { .. } | Verdict::DropSpike { .. } | Verdict::DropNegative { .. }
            )
        })
        // Needs `off` plus at least two values, or there is no curve to classify.
        .filter(|r| r.curve.len() >= 3)
        // Never re-screen something the pin already holds fixed.
        .filter(|r| !pinned_family.members.iter().any(|m| m.metric == r.metric))
        .collect();
    candidates.sort_by(|a, b| {
        let sa = a.best_pick_score().unwrap_or(f64::NEG_INFINITY);
        let sb = b.best_pick_score().unwrap_or(f64::NEG_INFINITY);
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });
    candidates.truncate(limits.rescue_cap);
    if candidates.is_empty() {
        return Ok(Vec::new());
    }

    let models: Vec<AxesModel> = candidates
        .iter()
        .map(|r| rescue_model(pinned_family, pinned_best, r, &report.baseline))
        .collect::<Result<_, String>>()
        .map_err(|e| anyhow::anyhow!("rescue axes: {e}"))?;
    let strategy = AdditiveStrategy::new(models, pricing, as_of, cfg.flow_patterns.as_ref());
    *combos_scanned += strategy.combos().len();
    let rows = strategy.run(corpus, observer)?;

    let matched = corpus.token_count() as u64;
    let mut out = Vec::with_capacity(candidates.len());
    for (r, rows) in candidates.iter().zip(&rows) {
        let curve: Vec<ResponsePoint> = rows
            .iter()
            .zip(r.menu_values())
            .map(|(m, value)| {
                let outcome = discovery_score(ComboStats::from_combo_metrics(m), matched, weights);
                ResponsePoint::from_row(value, m, outcome)
            })
            .collect();
        // Pick 0 is the pin standing alone — the with-vs-without baseline for a
        // conditional edge, exactly as `off` is for a standalone one.
        let pinned_score = curve
            .first()
            .and_then(|p| p.outcome.score())
            .unwrap_or(f64::NAN);
        out.push(Rescue {
            metric: r.metric,
            operator: r.operator,
            pinned: pinned_family.family,
            pinned_score,
            verdict: classify(&curve, report.thresholds),
        });
    }
    Ok(out)
}

/// Fold every successful rescue into its registry family, re-capping the families that
/// changed. Returns their indices — the grids that must be re-run.
fn adopt_rescues(
    families: &mut Vec<FamilyResult>,
    rescues: &[Rescue],
    limits: FamilyLimits,
) -> Vec<usize> {
    let mut touched: Vec<usize> = Vec::new();
    for res in rescues {
        let Verdict::Keep { lift, ref narrowed, .. } = res.verdict else { continue };
        if narrowed.is_empty() {
            continue;
        }
        let fam = group_spec(res.metric.group).family;
        let idx = match families.iter().position(|f| f.family == fam) {
            Some(i) => i,
            None => {
                // A rescue can be the *only* member of its family — Layer 1 kept
                // nothing there, so `plan_families` never created the entry.
                families.push(FamilyResult {
                    family: fam,
                    members: Vec::new(),
                    dropped: Vec::new(),
                    combos: 0,
                    best: None,
                    n_gated: 0,
                });
                families.len() - 1
            }
        };
        if families[idx]
            .members
            .iter()
            .any(|m| m.metric == res.metric && m.operator == res.operator)
        {
            continue;
        }
        families[idx].members.push(FamilyMember {
            metric: res.metric,
            operator: res.operator,
            values: narrowed.clone(),
            lift,
            rescued: true,
        });
        if !touched.contains(&idx) {
            touched.push(idx);
        }
    }
    for &i in &touched {
        let members = std::mem::take(&mut families[i].members);
        let (kept, newly_dropped) = cap_members(members, limits);
        families[i].members = kept;
        families[i].dropped.extend(newly_dropped);
        families[i].combos = grid_combos(&families[i].members);
    }
    touched
}

/// The pinned winner held fixed, one Layer-1 reject swept over its full menu on top.
fn rescue_model(
    pinned: &FamilyResult,
    pinned_best: &BestCombo,
    r: &MetricResponse,
    baseline: &ScreenBaseline,
) -> Result<AxesModel, String> {
    let mut axes: Vec<AxisSpec> = Vec::new();
    for (m, v) in pinned.members.iter().zip(&pinned_best.picks) {
        if let Some(a) = m.pinned_axis(*v) {
            axes.push(a);
        }
    }
    axes.push(AxisSpec {
        kind: "metric".to_string(),
        side: Some(r.metric.side),
        group: Some(group_spec(r.metric.group).name.to_string()),
        metric: Some(r.metric.metric.name().to_string()),
        operator: Some(r.operator),
        window: r.metric.window.map(|w| WindowField::Span(w.label())),
        slice: None,
        values: r.menu_values(),
    });
    axes.extend(baseline_axes(baseline));
    AxesModel::resolve(&AxesRequest { axes })
}

fn baseline_ok(b: &ScreenBaseline) -> Result<()> {
    if b.take_profit_pct.is_none() && b.stop_loss_pct.is_none() {
        anyhow::bail!("Layer 2 inherits the screen's baseline, which carries no TP/SL");
    }
    Ok(())
}

/// One family's grid: every member's axis (with `off`) plus the screen's baseline.
fn family_model(members: &[FamilyMember], baseline: &ScreenBaseline) -> Result<AxesModel, String> {
    let mut axes: Vec<AxisSpec> = members.iter().map(FamilyMember::axis).collect();
    axes.extend(baseline_axes(baseline));
    AxesModel::resolve(&AxesRequest { axes })
}

/// Family A pinned at its best combo, family B swept on top.
fn interaction_model(
    pinned_members: &[FamilyMember],
    pinned_best: &BestCombo,
    swept_members: &[FamilyMember],
    baseline: &ScreenBaseline,
) -> Result<AxesModel, String> {
    let mut axes: Vec<AxisSpec> = Vec::new();
    for (m, v) in pinned_members.iter().zip(&pinned_best.picks) {
        if let Some(a) = m.pinned_axis(*v) {
            axes.push(a);
        }
    }
    axes.extend(swept_members.iter().map(FamilyMember::axis));
    axes.extend(baseline_axes(baseline));
    AxesModel::resolve(&AxesRequest { axes })
}

fn baseline_axes(b: &ScreenBaseline) -> Vec<AxisSpec> {
    let mut out = Vec::with_capacity(2);
    let mut push = |kind: &str, v: Option<f64>| {
        if let Some(v) = v {
            out.push(AxisSpec {
                kind: kind.to_string(),
                side: None,
                group: None,
                metric: None,
                operator: None,
                window: None,
                slice: None,
                values: vec![Some(v)],
            });
        }
    };
    push("take_profit", b.take_profit_pct);
    push("stop_loss", b.stop_loss_pct);
    out
}

/// The best-scoring combo of one segment's rows, plus the min-N gate's kill count.
fn best_of(
    strategy: &AdditiveStrategy,
    segment: usize,
    rows: &[ComboMetrics],
    matched: u64,
    weights: DiscoveryWeights,
) -> (Option<BestCombo>, usize) {
    let mut gated = 0usize;
    let mut best: Option<(usize, f64, &ComboMetrics)> = None;
    for (idx, m) in rows.iter().enumerate() {
        match discovery_score(ComboStats::from_combo_metrics(m), matched, weights) {
            ScoreOutcome::Ranked(s) => {
                if best.is_none_or(|(_, bs, _)| s > bs) {
                    best = Some((idx, s, m));
                }
            }
            ScoreOutcome::BelowMinClosed { .. } => gated += 1,
            ScoreOutcome::NoFire => {}
        }
    }
    let combo = best.map(|(idx, score, m)| BestCombo {
        idx,
        picks: metric_picks(strategy.model(segment), idx),
        score,
        n_fired: m.n_fired,
        n_closed: m.n_closed,
        params_json: strategy
            .params_json_of(super::additive::AdditiveCombo { segment: segment as u32, pick: idx as u32 }),
    });
    (combo, gated)
}

/// The value each **metric** axis picked for combo `idx`, in model-axis order. TP/SL
/// axes are excluded: they are the fixed baseline, identical in every combo, so
/// including them would make two otherwise-equal pick vectors compare equal anyway
/// while obscuring what actually moved.
fn metric_picks(model: &AxesModel, idx: usize) -> Vec<Option<f64>> {
    let picks = model.combo_picks(idx);
    model
        .axes
        .iter()
        .zip(&picks)
        .filter(|(a, _)| matches!(a, ResolvedAxis::Metric { .. }))
        .map(|(a, p)| a.value_at(*p).flatten())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use hunter_engine::metrics::{MetricGroupId, MetricId};

    use super::super::candidates::{screen_plan, ScreenMetric};
    use super::super::fixtures::{corpus, pricing};
    use super::super::screen::{
        run_screen, MetricResponse, ResponsePoint, ScreenThresholds,
    };
    use crate::sweep::generic::axes::AxisSide;
    use crate::sweep::progress::NoopObserver;

    fn baseline() -> ScreenBaseline {
        ScreenBaseline { take_profit_pct: Some(30.0), stop_loss_pct: Some(15.0) }
    }

    fn screen_metric(metric: MetricId, side: AxisSide) -> ScreenMetric {
        screen_plan(&ScreenConfig::default())
            .metrics
            .iter()
            .find(|m| m.metric == metric && m.side == side)
            .copied()
            .expect("metric screened")
    }

    fn kept(metric: MetricId, side: AxisSide, narrowed: Vec<f64>, lift: f64) -> MetricResponse {
        MetricResponse {
            metric: screen_metric(metric, side),
            operator: Operator::Gte,
            curve: vec![ResponsePoint {
                value: None,
                outcome: ScoreOutcome::Ranked(1.0),
                n_fired: 10,
                n_closed: 10,
                win_rate: 0.5,
                median_pnl_pct: 1.0,
                total_pnl_sol: 0.1,
            }],
            baseline: Some(1.0),
            verdict: Verdict::Keep {
                best_value: narrowed[0],
                lift,
                plateau: 1.0,
                narrowed,
            },
        }
    }

    fn report_of(responses: Vec<MetricResponse>) -> ScreenReport {
        let mut shortlist: Vec<usize> = (0..responses.len()).collect();
        shortlist.sort_by(|a, b| {
            let la = responses[*a].lift().unwrap_or(f64::NEG_INFINITY);
            let lb = responses[*b].lift().unwrap_or(f64::NEG_INFINITY);
            lb.partial_cmp(&la).unwrap_or(std::cmp::Ordering::Equal)
        });
        ScreenReport {
            cohort_tokens: 10,
            baseline: baseline(),
            baseline_stats: None,
            weights: DiscoveryWeights::default(),
            effective_min_closed: DiscoveryWeights::default().effective_min_closed(10),
            thresholds: ScreenThresholds::default(),
            responses,
            shortlist,
            skipped: Vec::new(),
            gaps: Vec::new(),
            errors: Vec::new(),
            percentiles: Default::default(),
            n_gated: 0,
            combos_scanned: 0,
        }
    }

    /// Families come off the registry, not a lab-side map — `m_state` and
    /// `m_price_*` must land in different grids without this module naming them.
    #[test]
    fn shortlist_is_grouped_by_registry_family() {
        let r = report_of(vec![
            kept(MetricId::Time, AxisSide::Entry, vec![5.0, 30.0], 3.0),
            kept(MetricId::Liquidity, AxisSide::Entry, vec![40.0, 50.0], 2.0),
            kept(MetricId::Trail, AxisSide::Entry, vec![10.0, 20.0], 1.0),
        ]);
        let plan = plan_families(&r, FamilyLimits::default());
        assert_eq!(plan.len(), 2, "snapshot and price are distinct families");
        let liq_age = plan.iter().find(|f| f.family == MetricFamily::State).unwrap();
        assert_eq!(liq_age.members.len(), 2, "time + liquidity share m_state");
        // Axis order is lift-descending, so a cap drops the weakest member.
        assert!(liq_age.members[0].lift >= liq_age.members[1].lift);
        let price = plan.iter().find(|f| f.family == MetricFamily::Price).unwrap();
        assert_eq!(price.members.len(), 1);
        assert_eq!(price.members[0].metric.metric, MetricId::Trail);
        // (off + 2)² = 9 combos for the two-member family.
        assert_eq!(liq_age.combos, 9);
    }

    /// Bounds drop the weakest members and say so — never a silent truncation.
    #[test]
    fn caps_drop_weakest_members_with_a_reason() {
        let r = report_of(vec![
            kept(MetricId::Time, AxisSide::Entry, vec![5.0, 30.0], 9.0),
            kept(MetricId::Liquidity, AxisSide::Entry, vec![40.0, 50.0], 1.0),
        ]);
        let plan = plan_families(&r, FamilyLimits { max_axes: 1, max_combos: 1_024, rescue_cap: 0 });
        let fam = &plan[0];
        assert_eq!(fam.members.len(), 1);
        assert_eq!(fam.members[0].metric.metric, MetricId::Time, "highest lift survives");
        assert_eq!(fam.dropped.len(), 1);
        assert_eq!(fam.dropped[0].1, DropReason::AxisCap);
        assert_eq!(fam.dropped[0].0.metric.metric, MetricId::Liquidity);

        // The combo cap bites second, on the same lift order.
        let plan = plan_families(&r, FamilyLimits { max_axes: 4, max_combos: 4, rescue_cap: 0 });
        assert_eq!(plan[0].members.len(), 1);
        assert_eq!(plan[0].dropped[0].1, DropReason::ComboCap);
    }

    /// End-to-end over the synthetic cohort. The shortlist is authored rather than
    /// taken from a live screen so the grid and the interaction check are actually
    /// exercised: whether *this* toy corpus happens to yield a `Keep` is a property of
    /// the fixture, not of Layer 2, and the chained path is covered separately by
    /// [`chains_off_a_live_screen`].
    #[test]
    fn family_layer_grids_and_measures_interaction() {
        let c = corpus(12);
        let cfg = ScreenConfig::default();
        let weights = DiscoveryWeights { min_closed: 1, ..DiscoveryWeights::default() };
        let screen = report_of(vec![
            kept(MetricId::Time, AxisSide::Entry, vec![5.0, 30.0], 3.0),
            kept(MetricId::Liquidity, AxisSide::Entry, vec![40.0, 45.0], 2.0),
            kept(MetricId::Trail, AxisSide::Entry, vec![5.0, 15.0], 1.0),
        ]);

        let out = run_family_layer(
            &c,
            &cfg,
            &screen,
            FamilyLimits::default(),
            pricing(),
            Utc::now(),
            weights,
            &NoopObserver,
        )
        .unwrap();

        assert_eq!(out.cohort_tokens, 12);
        assert_eq!(out.families.len(), 2, "m_state and m_price_lifetime are two families");
        // Every gridded family is one of the registry's, and its combo count is the
        // additive product of its own members only.
        for f in &out.families {
            assert!(!f.members.is_empty());
            assert_eq!(f.combos, f.members.iter().fold(1, |n, m| n * (m.values.len() + 1)));
            assert!(f.combos <= out.limits.max_combos);
            assert!(f.members.len() <= out.limits.max_axes);
        }
        // Interaction checks are ordered pairs over families that produced a winner.
        let live = out.families.iter().filter(|f| f.best.is_some()).count();
        assert_eq!(out.interactions.len(), live * live.saturating_sub(1));
        for i in &out.interactions {
            assert_ne!(i.pinned, i.swept);
            if i.verdict != InteractionVerdict::Inconclusive {
                // Like-for-like comparison: one pick per swept member.
                let swept = out.families.iter().find(|f| f.family == i.swept).unwrap();
                assert_eq!(i.given.len(), swept.members.len());
                assert_eq!(i.alone.len(), swept.members.len());
                assert_eq!(
                    i.verdict == InteractionVerdict::Independent,
                    i.given == i.alone,
                );
            }
        }
        // The winner of each family is promotable straight to a rule.
        for f in &out.families {
            if let Some(b) = &f.best {
                hunter_engine::rule_params::RuleParams::parse(&b.params_json)
                    .expect("family winner is promotable");
                assert_eq!(b.picks.len(), f.members.len());
            }
        }
        // The whole layer stays inside the additive budget.
        assert!(out.combos_scanned > 0);
        assert!(
            out.combos_scanned < 10_000,
            "Layer 2 must stay small: {}",
            out.combos_scanned
        );
        // Joints only appear for Interacting components; each is promotable when crowned.
        for j in &out.joints {
            assert!(j.families.len() >= 2);
            assert!(!j.members.is_empty());
            if let Some(b) = &j.best {
                hunter_engine::rule_params::RuleParams::parse(&b.params_json)
                    .expect("joint winner is promotable");
            }
        }
        // Position metrics are exit-only, so any that survived sit in the price family.
        for f in &out.families {
            for m in &f.members {
                if m.metric.group == MetricGroupId::Position {
                    assert_eq!(f.family, MetricFamily::Price);
                    assert_eq!(m.metric.side, AxisSide::Exit);
                }
            }
        }
    }

    /// The layers chain: a live Layer-1 report feeds Layer 2 without an adapter, and
    /// an empty shortlist is a valid (empty) Layer-2 result, not an error.
    #[test]
    fn chains_off_a_live_screen() {
        let c = corpus(12);
        let cfg = ScreenConfig::default();
        let weights = DiscoveryWeights { min_closed: 1, ..DiscoveryWeights::default() };
        let screen = run_screen(
            &c,
            &cfg,
            baseline(),
            pricing(),
            Utc::now(),
            weights,
            ScreenThresholds::default(),
            &NoopObserver,
        )
        .unwrap();
        let out = run_family_layer(
            &c,
            &cfg,
            &screen,
            FamilyLimits::default(),
            pricing(),
            Utc::now(),
            weights,
            &NoopObserver,
        )
        .unwrap();
        // Every gridded member is either something the screen kept, or a rescue that
        // earned its axis under the pin — never anything else.
        for f in &out.families {
            for m in &f.members {
                let kept = screen
                    .shortlisted()
                    .any(|r| r.metric == m.metric && r.operator == m.operator);
                let rescued = out.rescues.iter().any(|res| {
                    res.metric == m.metric && res.operator == m.operator && res.verdict.is_keep()
                });
                assert_eq!(m.rescued, !kept, "a member's `rescued` flag must match its provenance");
                assert!(kept || rescued, "{:?} came from nowhere", m.metric.metric);
            }
        }
        // A rescue is only ever attempted on a metric Layer 1 dropped.
        for res in &out.rescues {
            assert!(!screen
                .shortlisted()
                .any(|r| r.metric == res.metric && r.operator == res.operator));
        }
        assert_eq!(out.families.is_empty(), out.combos_scanned == 0);
    }

    /// The rescue closes Layer 1's univariate blind spot — and stays conditional: a
    /// metric it brings back is flagged `rescued`, and disabling the pass removes it.
    #[test]
    fn synergy_rescue_is_conditional_and_capped() {
        let c = corpus(12);
        let cfg = ScreenConfig::default();
        let weights = DiscoveryWeights { min_closed: 1, ..DiscoveryWeights::default() };
        let screen = run_screen(
            &c,
            &cfg,
            baseline(),
            pricing(),
            Utc::now(),
            weights,
            ScreenThresholds::default(),
            &NoopObserver,
        )
        .unwrap();

        let run = |limits| {
            run_family_layer(&c, &cfg, &screen, limits, pricing(), Utc::now(), weights, &NoopObserver)
                .unwrap()
        };
        let off = run(FamilyLimits { rescue_cap: 0, ..FamilyLimits::default() });
        assert!(off.rescues.is_empty(), "rescue_cap 0 disables the pass outright");
        assert!(off.families.iter().all(|f| f.members.iter().all(|m| !m.rescued)));

        let on = run(FamilyLimits::default());
        assert!(on.rescues.len() <= FamilyLimits::default().rescue_cap, "the cap is enforced");
        // Every attempt carries its pinned reference, successful or not.
        for res in &on.rescues {
            assert!(on.families.iter().any(|f| f.family == res.pinned));
            assert_eq!(res.lift().is_some(), res.verdict.is_keep());
        }
        // The pass can only ever add axes — it never removes a Layer-1 keep.
        for f in &off.families {
            let same = on.families.iter().find(|g| g.family == f.family).expect("family survives");
            for m in f.members.iter().filter(|m| !m.rescued) {
                assert!(
                    same.members.iter().any(|g| g.metric == m.metric && g.operator == m.operator)
                        || same.dropped.iter().any(|(g, _)| g.metric == m.metric),
                    "a rescue displaced a Layer-1 keep without reporting the drop",
                );
            }
        }
    }

    #[test]
    fn interacting_components_are_undirected_connected() {
        let mk = |pinned, swept, verdict| Interaction {
            pinned,
            swept,
            alone: vec![],
            given: vec![],
            score_alone: 1.0,
            score_given: Some(1.0),
            verdict,
        };
        let interactions = vec![
            mk(MetricFamily::Price, MetricFamily::Flow, InteractionVerdict::Interacting),
            mk(MetricFamily::Flow, MetricFamily::Price, InteractionVerdict::Interacting),
            mk(MetricFamily::Flow, MetricFamily::FlowIx, InteractionVerdict::Interacting),
            mk(
                MetricFamily::State,
                MetricFamily::Price,
                InteractionVerdict::Independent,
            ),
        ];
        let comps = interacting_components(&interactions);
        assert_eq!(comps.len(), 1);
        assert_eq!(
            comps[0],
            vec![MetricFamily::Flow, MetricFamily::FlowIx, MetricFamily::Price]
        );
    }

    #[test]
    fn plan_joint_caps_drop_weakest_with_reason() {
        let members_a = vec![FamilyMember {
            metric: screen_metric(MetricId::Time, AxisSide::Entry),
            operator: Operator::Gte,
            values: vec![5.0, 30.0],
            lift: 9.0,
            rescued: false,
        }];
        let members_b = vec![FamilyMember {
            metric: screen_metric(MetricId::Trail, AxisSide::Entry),
            operator: Operator::Gte,
            values: vec![10.0, 20.0],
            lift: 1.0,
            rescued: false,
        }];
        let families = vec![
            FamilyResult {
                family: MetricFamily::State,
                members: members_a,
                dropped: vec![],
                combos: 3,
                best: None,
                n_gated: 0,
            },
            FamilyResult {
                family: MetricFamily::Price,
                members: members_b,
                dropped: vec![],
                combos: 3,
                best: None,
                n_gated: 0,
            },
        ];
        let joint = plan_joint(
            &families,
            &[MetricFamily::State, MetricFamily::Price],
            FamilyLimits { max_axes: 1, max_combos: 1_024, rescue_cap: 0 },
        );
        assert_eq!(joint.members.len(), 1);
        assert_eq!(joint.members[0].metric.metric, MetricId::Time);
        assert_eq!(joint.dropped.len(), 1);
        assert_eq!(joint.dropped[0].1, DropReason::AxisCap);
        assert_eq!(joint.families.len(), 2);
    }
}
