//! **Layer 2 — the family grid + interaction check** (plan §3): combine only the
//! metrics Layer 1 kept, and spend the combo budget on metrics that actually
//! *interact* instead of on a blind cross-product.
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
//!    and their grids compose; if B's best moves, they interact and Layer 3 should
//!    grid them jointly. `O(families²)` tiny sweeps, not one exponential grid — the
//!    measurement that replaces the guess.
//!
//! Both phases ride the [`additive`](super::additive) scan mode, so phase 1 is **one**
//! sweep over one shared precompute regardless of family count, and so is phase 2.
//! Two passes total, and they must be two: the interaction models are built *from*
//! phase 1's winners.

use anyhow::Result;
use hunter_engine::metrics::evaluator::Operator;
use hunter_engine::metrics::{group_spec, MetricFamily};

use crate::sweep::aggregate::ComboMetrics;
use crate::sweep::corpus::Corpus;
use crate::sweep::generic::axes::{AxesModel, AxesRequest, AxisSpec, ResolvedAxis};
use crate::sweep::generic::Pricing;
use crate::sweep::progress::SweepObserver;

use hunter_engine::metrics::Ts;

use super::additive::AdditiveStrategy;
use super::candidates::{ScreenConfig, ScreenMetric};
use super::objective::{discovery_score, ComboStats, DiscoveryWeights, ScoreOutcome};
use super::screen::{ScreenBaseline, ScreenReport, Verdict};

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
}

impl Default for FamilyLimits {
    fn default() -> Self {
        // 4 axes × (off + 3 narrowed values) = 256 worst case — well inside the
        // "small family grids" budget of plan §6 and orders below `MAX_COMBOS`.
        Self { max_axes: 4, max_combos: 1_024 }
    }
}

/// One shortlisted `(metric, operator)` carried into a family grid.
#[derive(Clone, Debug, PartialEq)]
pub struct FamilyMember {
    pub metric: ScreenMetric,
    pub operator: Operator,
    /// The narrowed candidate values from Layer 1 (the `off` sentinel is added by the
    /// axis, not stored here).
    pub values: Vec<f64>,
    /// Layer-1 marginal value over its own `off` — the ranking + capping key.
    pub lift: f64,
}

impl FamilyMember {
    fn axis(&self) -> AxisSpec {
        AxisSpec {
            kind: "metric".to_string(),
            side: Some(self.metric.side),
            group: Some(group_spec(self.metric.group).name.to_string()),
            metric: Some(self.metric.metric.name().to_string()),
            operator: Some(self.operator),
            window: self.metric.window,
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

/// The Layer-2 deliverable: the per-family winners plus the interaction map.
#[derive(Clone, Debug)]
pub struct FamilyReport {
    pub cohort_tokens: usize,
    pub baseline: ScreenBaseline,
    pub limits: FamilyLimits,
    pub families: Vec<FamilyResult>,
    pub interactions: Vec<Interaction>,
    /// Combos scanned across both phases (the honest budget).
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
        };
        let fam = group_spec(r.metric.group).family;
        match by_family.iter_mut().find(|(f, _)| *f == fam) {
            Some((_, v)) => v.push(member),
            None => by_family.push((fam, vec![member])),
        }
    }

    by_family
        .into_iter()
        .map(|(family, mut members)| {
            // The shortlist is already lift-ordered, but sort defensively so the axis
            // order (and therefore which member a cap drops) never depends on it.
            members.sort_by(|a, b| b.lift.partial_cmp(&a.lift).unwrap_or(std::cmp::Ordering::Equal));
            let mut dropped = Vec::new();
            while members.len() > limits.max_axes.max(1) {
                dropped.push((members.pop().expect("non-empty"), DropReason::AxisCap));
            }
            while members.len() > 1 && grid_combos(&members) > limits.max_combos.max(1) {
                dropped.push((members.pop().expect("non-empty"), DropReason::ComboCap));
            }
            let combos = grid_combos(&members);
            FamilyResult { family, members, dropped, combos, best: None, n_gated: 0 }
        })
        .collect()
}

/// Combos a family grid would produce: `Π (off + narrowed)` over its members. The
/// baseline TP/SL axes are single-valued and contribute a factor of 1.
fn grid_combos(members: &[FamilyMember]) -> usize {
    members.iter().fold(1usize, |n, m| n.saturating_mul(m.values.len() + 1))
}

/// Run Layer 2 over a completed Layer-1 [`ScreenReport`].
///
/// Phase 1 grids every family in one additive pass; phase 2 runs the pairwise
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
    if pairs.is_empty() {
        return Ok(FamilyReport {
            cohort_tokens: corpus.token_count(),
            baseline,
            limits,
            families,
            interactions: Vec::new(),
            combos_scanned,
        });
    }

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

    let mut interactions = Vec::with_capacity(pairs.len());
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

    Ok(FamilyReport {
        cohort_tokens: corpus.token_count(),
        baseline,
        limits,
        families,
        interactions,
        combos_scanned,
    })
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
            weights: DiscoveryWeights::default(),
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

    /// Families come off the registry, not a lab-side map — `m_snapshot` and
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
        let liq_age = plan.iter().find(|f| f.family == MetricFamily::LiquidityAge).unwrap();
        assert_eq!(liq_age.members.len(), 2, "time + liquidity share m_snapshot");
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
        let plan = plan_families(&r, FamilyLimits { max_axes: 1, max_combos: 1_024 });
        let fam = &plan[0];
        assert_eq!(fam.members.len(), 1);
        assert_eq!(fam.members[0].metric.metric, MetricId::Time, "highest lift survives");
        assert_eq!(fam.dropped.len(), 1);
        assert_eq!(fam.dropped[0].1, DropReason::AxisCap);
        assert_eq!(fam.dropped[0].0.metric.metric, MetricId::Liquidity);

        // The combo cap bites second, on the same lift order.
        let plan = plan_families(&r, FamilyLimits { max_axes: 4, max_combos: 4 });
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
        assert_eq!(out.families.len(), 2, "m_snapshot and m_price_lifetime are two families");
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
        // Whatever the screen kept, Layer 2 only ever grids kept metrics.
        for f in &out.families {
            for m in &f.members {
                assert!(screen
                    .shortlisted()
                    .any(|r| r.metric == m.metric && r.operator == m.operator));
            }
        }
        assert_eq!(out.families.is_empty(), out.combos_scanned == 0);
    }
}
