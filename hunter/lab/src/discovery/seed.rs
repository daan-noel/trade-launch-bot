//! **Sweep seed** — the discovery → grouped-sweep handoff.
//!
//! Discovery's primary deliverable is not a promoted rule: it is a ready
//! [`AxisSpec`](crate::sweep::generic::axes::AxisSpec) grid for the generic
//! sweep, built from Layer-1 Keep metrics (narrowed menus), TP/SL ladders
//! around the run baseline, and cluster notes from the Layer-2 interaction map
//! (incl. joint grids). Near-miss drops land in [`SweepSeed::optional_axes`]
//! (off by default in the UI) — see [`near_miss_axis`] for which drops qualify.

use hunter_engine::metrics::group_spec;
use hunter_engine::metrics::MetricFamily;
use serde::Serialize;

use crate::sweep::generic::axes::{AxisSide, AxisSpec};

use super::family::{FamilyReport, InteractionVerdict, JointResult};
use super::screen::{MetricResponse, ScreenBaseline, ScreenReport, Verdict};

/// Canonical TP % rungs (see `docs/plans/sweep/axis-value-candidates.md`).
const TP_LADDER: &[f64] = &[20.0, 30.0, 60.0, 100.0, 150.0];
/// Canonical SL % rungs.
const SL_LADDER: &[f64] = &[10.0, 15.0, 25.0, 40.0];

/// One family cluster for the UI badges / notes.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct SeedCluster {
    /// Registry family names in this cluster.
    pub families: Vec<String>,
    /// `true` when the families were measured as interacting (joint-gridded).
    pub interacting: bool,
    /// `side·group.metric` identities of the axes that belong to this cluster.
    pub members: Vec<String>,
}

/// The discovery → sweep handoff payload.
#[derive(Clone, Debug, Serialize)]
pub struct SweepSeed {
    /// Axes the sweep form should load (Keep metrics + expanded TP/SL).
    pub axes: Vec<AxisSpec>,
    /// Near-miss metrics (`DropNoEdge` with best score > 0) — UI toggles off by default.
    pub optional_axes: Vec<AxisSpec>,
    /// Family clusters (interacting components + independent singletons).
    pub clusters: Vec<SeedCluster>,
    /// Product of value counts over [`Self::axes`] (same idea as the FE comboCount).
    pub combo_estimate: usize,
    /// Human-readable notes (independent vs joint, drops, caps).
    pub notes: Vec<String>,
}

/// Build the sweep seed from the Layer-1/2 reports (pure projection — no re-score).
pub fn build_sweep_seed(screen: &ScreenReport, family: &FamilyReport) -> SweepSeed {
    let mut axes: Vec<AxisSpec> = Vec::new();
    let mut notes: Vec<String> = Vec::new();

    // Metric axes: every Keep, off + narrowed.
    for r in screen.shortlisted() {
        if let Some(spec) = keep_axis(r) {
            axes.push(spec);
        }
    }

    // TP/SL: baseline ± one ladder rung (multi-value so the sweep isn't locked).
    axes.extend(tpsl_seed_axes(&screen.baseline, &mut notes));

    let clusters = build_clusters(family);
    for c in &clusters {
        if c.interacting {
            notes.push(format!(
                "joint cluster [{}]: grid these families together",
                c.families.join(", ")
            ));
        } else if c.families.len() == 1 {
            notes.push(format!(
                "independent family {}: may drop from the joint grid for budget",
                c.families[0]
            ));
        }
    }
    for j in &family.joints {
        for (m, reason) in &j.dropped {
            notes.push(format!(
                "joint grid dropped {}·{}.{} ({reason:?})",
                side_str(m.metric.side),
                group_spec(m.metric.group).name,
                m.metric.metric.name()
            ));
        }
        if j.best.is_some() {
            notes.push(format!(
                "joint winner over [{}] ({} combos)",
                j.families.iter().map(|f| f.as_str()).collect::<Vec<_>>().join(", "),
                j.combos
            ));
        }
    }

    // Near-miss axes, counted by *why* they missed: the three classes call for three
    // different reads, so one flat count would hide which one the reader has.
    let mut optional_axes: Vec<AxisSpec> = Vec::new();
    let (mut leads, mut spikes) = (0usize, 0usize);
    for r in &screen.responses {
        let Some(axis) = near_miss_axis(r) else { continue };
        match r.verdict {
            Verdict::DropNegative { .. } => leads += 1,
            Verdict::DropSpike { .. } => spikes += 1,
            _ => {}
        }
        optional_axes.push(axis);
    }
    if !optional_axes.is_empty() {
        let flat = optional_axes.len() - leads - spikes;
        notes.push(format!(
            "{} near-miss axis(es) available as optional: {flat} flat-but-positive, {leads} that \
             lifted a losing baseline (those are worth the TP/SL ladder), {spikes} single spike(s) \
             whose neighbours collapsed — a spike is UNSTABLE, keep it only if the sweep \
             reproduces it under a different bracket",
            optional_axes.len(),
        ));
    }

    let combo_estimate = axes.iter().fold(1usize, |n, a| n.saturating_mul(a.values.len().max(1)));

    SweepSeed { axes, optional_axes, clusters, combo_estimate, notes }
}

fn keep_axis(r: &MetricResponse) -> Option<AxisSpec> {
    let Verdict::Keep { ref narrowed, .. } = r.verdict else {
        return None;
    };
    if narrowed.is_empty() {
        return None;
    }
    Some(metric_axis(r, narrowed))
}

/// Near-miss axes worth offering as optional sweep rows:
///
/// * `DropNoEdge` whose best ranked pick is strictly positive — it just missed the
///   lift bar, and a different TP/SL may clear it.
/// * `DropSpike` on the same terms. The peak's neighbours collapsed, so this is the
///   least trustworthy of the three — but re-pricing it across the whole TP/SL ladder
///   is exactly the test a spike needs, and withholding it leaves a spike-dominated
///   run with a TP/SL-only handoff, which reads as "nothing was found" when what
///   happened is "nothing was found *at this bracket*". The seed note flags it.
/// * `DropNegative` at **any** sign — the metric measurably lifted its baseline and
///   only failed because that baseline loses money here. Filtering it on `> 0` would
///   discard the one class of drop the sweep is most likely to convert, since the
///   sweep re-tries it across the whole TP/SL ladder.
fn near_miss_axis(r: &MetricResponse) -> Option<AxisSpec> {
    match r.verdict {
        Verdict::DropNegative { .. } => negative_lead_axis(r),
        Verdict::DropNoEdge { .. } | Verdict::DropSpike { .. } => positive_pick_axis(r),
        _ => None,
    }
}

/// The up-to-3 best **positive-scoring** values of a curve. `None` when no value
/// scored above zero — there is nothing there for a sweep to re-price.
fn positive_pick_axis(r: &MetricResponse) -> Option<AxisSpec> {
    let scored: Vec<(f64, f64)> = r
        .curve
        .iter()
        .filter_map(|p| {
            let v = p.value?;
            let s = p.outcome.score()?;
            (s > 0.0).then_some((v, s))
        })
        .collect();
    top_scoring_axis(r, scored)
}

/// The best-scoring values of a `DropNegative` curve, sign ignored — the metric's own
/// ranking, carried into a sweep that will re-price it under other brackets.
fn negative_lead_axis(r: &MetricResponse) -> Option<AxisSpec> {
    let scored: Vec<(f64, f64)> = r
        .curve
        .iter()
        .filter_map(|p| Some((p.value?, p.outcome.score()?)))
        .collect();
    top_scoring_axis(r, scored)
}

/// The 3 best-scoring `(value, score)` picks as an axis, values ascending and deduped
/// — an axis's order is its menu's order, never the ranking's.
fn top_scoring_axis(r: &MetricResponse, mut scored: Vec<(f64, f64)>) -> Option<AxisSpec> {
    if scored.is_empty() {
        return None;
    }
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(3);
    let mut values: Vec<f64> = scored.into_iter().map(|(v, _)| v).collect();
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    values.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
    Some(metric_axis(r, &values))
}

fn metric_axis(r: &MetricResponse, values: &[f64]) -> AxisSpec {
    AxisSpec {
        kind: "metric".to_string(),
        side: Some(r.metric.side),
        group: Some(group_spec(r.metric.group).name.to_string()),
        metric: Some(r.metric.metric.name().to_string()),
        operator: Some(r.operator),
        window: r.metric.window,
        values: std::iter::once(None)
            .chain(values.iter().copied().map(Some))
            .collect(),
    }
}

fn tpsl_seed_axes(baseline: &ScreenBaseline, notes: &mut Vec<String>) -> Vec<AxisSpec> {
    let mut out = Vec::with_capacity(2);
    if let Some(tp) = baseline.take_profit_pct {
        let vals = expand_ladder(TP_LADDER, tp);
        notes.push(format!("TP seed menu around {tp}: {vals:?}"));
        out.push(AxisSpec {
            kind: "take_profit".to_string(),
            side: None,
            group: None,
            metric: None,
            operator: None,
            window: None,
            values: vals.into_iter().map(Some).collect(),
        });
    }
    if let Some(sl) = baseline.stop_loss_pct {
        let vals = expand_ladder(SL_LADDER, sl);
        notes.push(format!("SL seed menu around {sl}: {vals:?}"));
        out.push(AxisSpec {
            kind: "stop_loss".to_string(),
            side: None,
            group: None,
            metric: None,
            operator: None,
            window: None,
            values: vals.into_iter().map(Some).collect(),
        });
    }
    out
}

/// Baseline ± one ladder rung (clamped to ladder ends); always includes baseline
/// itself even when it is off-ladder.
pub fn expand_ladder(ladder: &[f64], baseline: f64) -> Vec<f64> {
    if ladder.is_empty() {
        return vec![baseline];
    }
    let mut best_i = 0usize;
    let mut best_d = (ladder[0] - baseline).abs();
    for (i, &v) in ladder.iter().enumerate().skip(1) {
        let d = (v - baseline).abs();
        if d < best_d {
            best_d = d;
            best_i = i;
        }
    }
    let lo = best_i.saturating_sub(1);
    let hi = (best_i + 1).min(ladder.len() - 1);
    let mut out: Vec<f64> = ladder[lo..=hi].to_vec();
    if !out.iter().any(|v| (*v - baseline).abs() < 1e-12) {
        out.push(baseline);
    }
    out.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    out.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
    out
}

fn build_clusters(family: &FamilyReport) -> Vec<SeedCluster> {
    let mut clusters: Vec<SeedCluster> = Vec::new();

    // Interacting joint results first.
    let joint_fams: std::collections::HashSet<MetricFamily> = family
        .joints
        .iter()
        .flat_map(|j| j.families.iter().copied())
        .collect();

    for j in &family.joints {
        clusters.push(cluster_from_joint(j));
    }

    // Independent (or inconclusive-only) families not absorbed into a joint.
    for f in &family.families {
        if joint_fams.contains(&f.family) {
            continue;
        }
        // Was this family part of any Interacting edge? If so but no joint ran
        // (shouldn't happen), still flag interacting from the map.
        let interacting = family.interactions.iter().any(|i| {
            i.verdict == InteractionVerdict::Interacting
                && (i.pinned == f.family || i.swept == f.family)
        });
        clusters.push(SeedCluster {
            families: vec![f.family.as_str().to_string()],
            interacting,
            members: f
                .members
                .iter()
                .map(|m| {
                    format!(
                        "{}·{}.{}",
                        side_str(m.metric.side),
                        group_spec(m.metric.group).name,
                        m.metric.metric.name()
                    )
                })
                .collect(),
        });
    }
    clusters
}

fn cluster_from_joint(j: &JointResult) -> SeedCluster {
    SeedCluster {
        families: j.families.iter().map(|f| f.as_str().to_string()).collect(),
        interacting: true,
        members: j
            .members
            .iter()
            .map(|m| {
                format!(
                    "{}·{}.{}",
                    side_str(m.metric.side),
                    group_spec(m.metric.group).name,
                    m.metric.metric.name()
                )
            })
            .collect(),
    }
}

fn side_str(side: AxisSide) -> &'static str {
    match side {
        AxisSide::Entry => "entry",
        AxisSide::Exit => "exit",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hunter_engine::metrics::evaluator::Operator;
    use hunter_engine::metrics::{MetricId};

    use super::super::candidates::{screen_plan, ScreenConfig, ScreenMetric};
    use super::super::family::{
        FamilyLimits, FamilyMember, FamilyReport, FamilyResult, Interaction, InteractionVerdict,
    };
    use super::super::objective::{DiscoveryWeights, ScoreOutcome};
    use super::super::screen::{
        MetricResponse, ResponsePoint, ScreenBaseline, ScreenReport, ScreenThresholds, Verdict,
    };
    use crate::sweep::generic::axes::AxisSide;

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

    fn keep(metric: MetricId, side: AxisSide, narrowed: Vec<f64>, lift: f64) -> MetricResponse {
        MetricResponse {
            metric: screen_metric(metric, side),
            operator: Operator::Gte,
            curve: vec![point(None, 1.0)],
            baseline: Some(1.0),
            verdict: Verdict::Keep {
                best_value: narrowed[0],
                lift,
                plateau: 1.0,
                narrowed,
            },
        }
    }

    fn point(value: Option<f64>, score: f64) -> ResponsePoint {
        ResponsePoint {
            value,
            outcome: ScoreOutcome::Ranked(score),
            n_fired: 10,
            n_closed: 10,
            win_rate: 0.5,
            median_pnl_pct: score,
            total_pnl_sol: score,
        }
    }

    fn drop_no_edge_positive(metric: MetricId, side: AxisSide) -> MetricResponse {
        MetricResponse {
            metric: screen_metric(metric, side),
            operator: Operator::Gte,
            curve: vec![point(None, 0.5), point(Some(10.0), 0.6)],
            baseline: Some(0.5),
            verdict: Verdict::DropNoEdge { best_lift: 0.1 },
        }
    }

    /// A spike shaped like a real one: the peak's neighbours give the lift straight
    /// back, and one of them is outright destructive.
    fn drop_spike_positive(metric: MetricId, side: AxisSide) -> MetricResponse {
        MetricResponse {
            metric: screen_metric(metric, side),
            operator: Operator::Gte,
            curve: vec![
                point(None, 4.36),
                point(Some(1.0), -0.26),
                point(Some(8.0), 4.80),
                point(Some(25.0), 4.31),
            ],
            baseline: Some(4.36),
            verdict: Verdict::DropSpike { best_value: 8.0, lift: 0.44, plateau: -0.11 },
        }
    }

    fn screen_of(responses: Vec<MetricResponse>) -> ScreenReport {
        let shortlist: Vec<usize> = responses
            .iter()
            .enumerate()
            .filter(|(_, r)| r.verdict.is_keep())
            .map(|(i, _)| i)
            .collect();
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

    fn empty_family(screen: &ScreenReport) -> FamilyReport {
        let members: Vec<FamilyMember> = screen
            .shortlisted()
            .filter_map(|r| match &r.verdict {
                Verdict::Keep { narrowed, lift, .. } => Some(FamilyMember {
                    metric: r.metric,
                    operator: r.operator,
                    values: narrowed.clone(),
                    lift: *lift,
                    rescued: false,
                }),
                _ => None,
            })
            .collect();
        let family = if members.is_empty() {
            MetricFamily::Price
        } else {
            group_spec(members[0].metric.group).family
        };
        FamilyReport {
            cohort_tokens: 10,
            baseline: baseline(),
            limits: FamilyLimits::default(),
            families: if members.is_empty() {
                vec![]
            } else {
                vec![FamilyResult {
                    family,
                    combos: members.iter().fold(1, |n, m| n * (m.values.len() + 1)),
                    members,
                    dropped: vec![],
                    best: None,
                    n_gated: 0,
                }]
            },
            interactions: vec![],
            joints: vec![],
            rescues: vec![],
            combos_scanned: 0,
        }
    }

    #[test]
    fn expand_ladder_includes_neighbours_and_baseline() {
        assert_eq!(expand_ladder(TP_LADDER, 30.0), vec![20.0, 30.0, 60.0]);
        assert_eq!(expand_ladder(TP_LADDER, 20.0), vec![20.0, 30.0]);
        assert_eq!(expand_ladder(TP_LADDER, 150.0), vec![100.0, 150.0]);
        // Off-ladder baseline is injected; equal distance prefers the lower rung,
        // so 45 → neighbours of 30 (=20,30,60) plus 45.
        assert_eq!(expand_ladder(TP_LADDER, 45.0), vec![20.0, 30.0, 45.0, 60.0]);
        assert_eq!(expand_ladder(SL_LADDER, 15.0), vec![10.0, 15.0, 25.0]);
    }

    #[test]
    fn seed_axes_match_keep_plus_tpsl_menus() {
        let screen = screen_of(vec![
            keep(MetricId::Time, AxisSide::Entry, vec![5.0, 30.0], 3.0),
            drop_no_edge_positive(MetricId::Liquidity, AxisSide::Entry),
        ]);
        let family = empty_family(&screen);
        let seed = build_sweep_seed(&screen, &family);

        let metrics: Vec<_> = seed
            .axes
            .iter()
            .filter(|a| a.kind == "metric")
            .collect();
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].metric.as_deref(), Some("time"));
        assert_eq!(
            metrics[0].values,
            vec![None, Some(5.0), Some(30.0)]
        );

        let tp = seed.axes.iter().find(|a| a.kind == "take_profit").unwrap();
        assert_eq!(tp.values, vec![Some(20.0), Some(30.0), Some(60.0)]);
        let sl = seed.axes.iter().find(|a| a.kind == "stop_loss").unwrap();
        assert_eq!(sl.values, vec![Some(10.0), Some(15.0), Some(25.0)]);

        assert_eq!(seed.optional_axes.len(), 1);
        assert_eq!(seed.optional_axes[0].metric.as_deref(), Some("liquidity"));
        assert!(seed.combo_estimate >= 3 * 3 * 3);
        assert!(!seed.notes.is_empty());
    }

    /// A spike is the least trustworthy near-miss, but withholding it hands a
    /// spike-dominated run a TP/SL-only seed — which reads as "nothing was found"
    /// rather than "nothing was found at this bracket".
    #[test]
    fn positive_spike_is_offered_as_an_optional_axis() {
        let screen = screen_of(vec![drop_spike_positive(MetricId::Liquidity, AxisSide::Entry)]);
        let family = empty_family(&screen);
        let seed = build_sweep_seed(&screen, &family);

        assert!(
            seed.axes.iter().all(|a| a.kind != "metric"),
            "an empty shortlist still seeds no metric axis"
        );
        assert_eq!(seed.optional_axes.len(), 1);
        // `off` plus the two positive-scoring picks, ascending — the destructive value
        // is not worth a sweep row.
        assert_eq!(seed.optional_axes[0].values, vec![None, Some(8.0), Some(25.0)]);
        assert!(
            seed.notes.iter().any(|n| n.contains("spike")),
            "the note must flag the spike as unstable: {:?}",
            seed.notes
        );
    }

    /// A spike with nothing positive on its curve is still not an axis — there is no
    /// value there for the sweep to re-price.
    #[test]
    fn spike_with_no_positive_pick_is_not_offered() {
        let mut r = drop_spike_positive(MetricId::Liquidity, AxisSide::Entry);
        r.curve = vec![point(None, 4.36), point(Some(1.0), -0.26), point(Some(8.0), -0.10)];
        let screen = screen_of(vec![r]);
        let family = empty_family(&screen);
        assert!(build_sweep_seed(&screen, &family).optional_axes.is_empty());
    }

    #[test]
    fn interacting_cluster_notes_joint() {
        let screen = screen_of(vec![
            keep(MetricId::Time, AxisSide::Entry, vec![5.0], 3.0),
            keep(MetricId::Trail, AxisSide::Entry, vec![10.0], 2.0),
        ]);
        let mut family = empty_family(&screen);
        // Force two families + an interacting edge + a joint stub.
        let liq = FamilyMember {
            metric: screen_metric(MetricId::Time, AxisSide::Entry),
            operator: Operator::Gte,
            values: vec![5.0],
            lift: 3.0,
            rescued: false,
        };
        let price = FamilyMember {
            metric: screen_metric(MetricId::Trail, AxisSide::Entry),
            operator: Operator::Gte,
            values: vec![10.0],
            lift: 2.0,
            rescued: false,
        };
        family.families = vec![
            FamilyResult {
                family: MetricFamily::LiquidityAge,
                members: vec![liq.clone()],
                dropped: vec![],
                combos: 2,
                best: None,
                n_gated: 0,
            },
            FamilyResult {
                family: MetricFamily::Price,
                members: vec![price.clone()],
                dropped: vec![],
                combos: 2,
                best: None,
                n_gated: 0,
            },
        ];
        family.interactions = vec![Interaction {
            pinned: MetricFamily::LiquidityAge,
            swept: MetricFamily::Price,
            alone: vec![Some(10.0)],
            given: vec![Some(5.0)],
            score_alone: 1.0,
            score_given: Some(1.2),
            verdict: InteractionVerdict::Interacting,
        }];
        family.joints = vec![super::super::family::JointResult {
            families: vec![MetricFamily::LiquidityAge, MetricFamily::Price],
            members: vec![liq, price],
            dropped: vec![],
            combos: 4,
            best: None,
            n_gated: 0,
        }];

        let seed = build_sweep_seed(&screen, &family);
        assert!(seed.clusters.iter().any(|c| c.interacting && c.families.len() == 2));
        assert!(seed.notes.iter().any(|n| n.contains("joint cluster")));
    }
}
