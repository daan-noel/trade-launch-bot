//! **Automated candidate generation** (plan §2.1) — the registry-driven
//! replacement for the hand-derived anchor table in
//! [`docs/plans/sweep/axis-value-candidates.md`].
//!
//! Today a human runs throwaway DuckDB percentile queries and records the anchors
//! in that doc; a metric added later has no menu until someone repeats the ritual.
//! That is the pipeline's single biggest extensibility hole (plan §0, gap #1). This
//! module closes it: for a cohort it
//!
//! 1. enumerates every screenable `(side, group, metric[, window])` straight off
//!    [`REGISTRY`] ([`screen_plan`]) — a new metric is included automatically,
//!    and anything excluded is reported with a [`SkipReason`], never dropped
//!    silently;
//! 2. measures each metric's own distribution over the cohort
//!    ([`collect_percentiles`]) into a percentile ladder;
//! 3. spaces a candidate menu off that ladder, rounded to the metric's `unit`,
//!    with the `off` sentinel first ([`build_menus`]) — ready to hand to the
//!    sweep as an [`AxisSpec`].
//!
//! ## Why the percentiles come from `MetricSeries`, not DuckDB SQL
//!
//! The plan sketched `metric_percentiles` as an `approx_quantile` in
//! [`lake::duck`](crate::lake::duck). It isn't: only `time`/`liquidity` are raw
//! lake columns — `trail`, `stall`, the rolling-window flows and the price-window
//! extrema are *engine* quantities. Expressing them as DuckDB window functions
//! would be a **second implementation of metric semantics** that can silently drift
//! from [`hunter_engine`] (the SSOT rule), and the anchors would then describe
//! values the screen never actually gates on. Instead the ladder is measured
//! through the engine's own [`MetricSeries`] compute — the exact numbers the Layer-1
//! scan reads — and the same per-token precompute the screen needs anyway (plan
//! §6.1: precompute reuse is the dominant lever).
//!
//! Values are sampled at **trade moments** (trades folded with no synthetic ticks),
//! matching what the hand-derived anchor table measured.
//!
//! [`docs/plans/sweep/axis-value-candidates.md`]: ../../../docs/plans/sweep/axis-value-candidates.md
//! [`REGISTRY`]: hunter_engine::metrics::REGISTRY

use hunter_engine::metrics::evaluator::Operator;
use hunter_engine::metrics::flow_split::FlowPatterns;
use hunter_engine::metrics::series::{MetricSeries, SeriesColumn};
use hunter_engine::metrics::{
    group_spec, is_flow_metric, MetricGroupId, MetricId, MetricKind, MetricScope, Unit, REGISTRY,
};
use trading_core::strategies::kernel::exact_quantile_f64;

use crate::sweep::corpus::CorpusToken;
use crate::sweep::generic::axes::{AxisSide, AxisSpec, SWEEP_FLOW_FP};
use crate::sweep::projection::to_trade_lite;

/// The percentile ladder every screenable metric is measured on. Mirrors the
/// columns of the hand-written anchor table so a generated row is comparable with
/// the recorded ones.
pub const PERCENTILE_LADDER: [f64; 8] = [0.05, 0.10, 0.25, 0.50, 0.75, 0.90, 0.95, 0.99];

/// The ladder rungs a candidate menu is spaced on (plan §2.1: `p10, p25, p50, p75,
/// p90` plus the `off` sentinel). A strict subset of [`PERCENTILE_LADDER`].
pub const MENU_PERCENTILES: [f64; 5] = [0.10, 0.25, 0.50, 0.75, 0.90];

/// Declared menus for **position-scoped** metrics (`m_position`). They anchor on
/// *your* entry fill, so no token-independent distribution exists to measure — the
/// one explicit annotation the extensibility contract predicts (plan §5). Values
/// are the trailing-stop / holding-time anchors from `axis-value-candidates.md`.
///
/// Every `MetricScope::Position` metric must appear here or in
/// [`POSITION_EXCLUDED`]; `position_metrics_all_declared` pins that, so adding one
/// to the registry fails loudly instead of going unscreened.
const POSITION_MENUS: &[(MetricId, &[f64])] = &[
    (MetricId::Retrace, &[5.0, 10.0, 15.0, 25.0, 40.0]),
    (MetricId::Bounce, &[5.0, 10.0, 15.0, 25.0, 40.0]),
    (MetricId::Held, &[30.0, 60.0, 120.0, 300.0, 600.0]),
];

/// Position metrics the screen deliberately does not sweep as a metric axis.
/// `pnl` **is** the baseline TP/SL (they desugar into it — see the engine's
/// `arm.rs`), which every screening combo already carries; screening it again
/// would just re-sweep the baseline.
const POSITION_EXCLUDED: &[MetricId] = &[MetricId::Pnl];

// ───────────────────────────── configuration ───────────────────────────────

/// Which comparison directions the screen tries per metric.
///
/// The default is **both**: the plan treats the operator as an *output* of the
/// screen ("keep, with the suggested operator" — §2.2), and the anchor table shows
/// most metrics earning a gate in either direction depending on side (`liquidity >`
/// floor vs `liquidity <` pre-migration cap). Measuring beats guessing, and it is
/// additive (2 × 5 values), not multiplicative.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DirectionPolicy {
    #[default]
    Both,
    Above,
    Below,
}

impl DirectionPolicy {
    /// The operators to screen, primary first. `>=` is primary everywhere: an
    /// upper-bound gate is only ever the complement of the same cutpoint.
    pub fn operators(self) -> &'static [Operator] {
        match self {
            DirectionPolicy::Both => &[Operator::Gte, Operator::Lt],
            DirectionPolicy::Above => &[Operator::Gte],
            DirectionPolicy::Below => &[Operator::Lt],
        }
    }
}

/// Run-scoped knobs for the screen's candidate generation.
#[derive(Clone, Debug)]
pub struct ScreenConfig {
    /// `window_size_sec` every dynamic group is screened at on the **entry** side.
    /// Windows are compared across runs, not swept within one (plan §2.1).
    pub entry_window_sec: f64,
    /// `window_size_sec` for dynamic groups on the **exit** side.
    pub exit_window_sec: f64,
    /// Compiled `volume_ix_patterns` for the run. `None` ⇒ `m_flow_split*` metrics
    /// are skipped ([`SkipReason::FlowPatternsMissing`]) — their values are
    /// pattern-dependent, so a corpus-wide menu would be meaningless.
    pub flow_patterns: Option<FlowPatterns>,
    /// Per-metric ceiling on retained samples (see [`Reservoir`]). Bounds the
    /// percentile pass's RAM at `columns × cap × 8` bytes regardless of corpus size.
    pub sample_cap: usize,
    pub directions: DirectionPolicy,
}

impl Default for ScreenConfig {
    fn default() -> Self {
        Self {
            entry_window_sec: 30.0,
            exit_window_sec: 10.0,
            flow_patterns: None,
            // 200k samples per metric ⇒ ~1.6 MB/column; the p10..p90 rungs the menu
            // reads are stable long before this.
            sample_cap: 200_000,
            directions: DirectionPolicy::default(),
        }
    }
}

// ───────────────────────────── the screen plan ─────────────────────────────

/// Where a metric's candidate values come from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ValueSource {
    /// Measured from the metric's own distribution over the cohort, through this
    /// precompute column.
    Series(SeriesColumn),
    /// Declared up front ([`POSITION_MENUS`]) — no cohort distribution exists.
    Declared(&'static [f64]),
}

/// One `(side, metric)` the screen will sweep alone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenMetric {
    pub side: AxisSide,
    pub group: MetricGroupId,
    pub metric: MetricId,
    /// `Some` iff the group is [`MetricKind::Dynamic`] — the side's window.
    pub window: Option<f64>,
    pub source: ValueSource,
}

impl ScreenMetric {
    /// The precompute column this metric reads (`None` for a declared menu).
    pub fn column(&self) -> Option<SeriesColumn> {
        match self.source {
            ValueSource::Series(c) => Some(c),
            ValueSource::Declared(_) => None,
        }
    }
}

/// Why a registry metric is not screened on a side. Reported, never silently
/// dropped (the no-silent-caps rule).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipReason {
    /// `m_flow_split*` without `volume_ix_patterns` — values are pattern-dependent.
    FlowPatternsMissing,
    /// Position-scoped groups have no value before entry (`axes.rs` rejects the
    /// entry axis outright).
    PositionIsExitOnly,
    /// `m_position.pnl` is the baseline TP/SL the screen already carries.
    BaselineTpSl,
    /// Position-scoped with no entry in [`POSITION_MENUS`] — a new metric that
    /// needs its declared menu (guarded by `position_metrics_all_declared`).
    NoDeclaredMenu,
}

/// A registry metric the screen left out, with the reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Skipped {
    pub side: AxisSide,
    pub group: MetricGroupId,
    pub metric: MetricId,
    pub reason: SkipReason,
}

/// The screenable metrics for a run, plus everything excluded and why.
#[derive(Clone, Debug, Default)]
pub struct ScreenPlan {
    pub metrics: Vec<ScreenMetric>,
    pub skipped: Vec<Skipped>,
}

impl ScreenPlan {
    /// The distinct precompute columns the percentile pass must record — one
    /// [`MetricSeries`] per token over this union serves every metric (plan §6.1).
    pub fn columns(&self) -> Vec<SeriesColumn> {
        let mut cols: Vec<SeriesColumn> = Vec::new();
        for c in self.metrics.iter().filter_map(ScreenMetric::column) {
            if !cols.contains(&c) {
                cols.push(c);
            }
        }
        cols
    }
}

/// Enumerate every screenable metric off [`REGISTRY`] for `cfg`.
///
/// Registry-driven end to end: `kind` picks the side's window, `scope` forces
/// exit-only, and a fingerprint-configured group needs its patterns. Adding a
/// metric to the registry surfaces it here with no edit.
pub fn screen_plan(cfg: &ScreenConfig) -> ScreenPlan {
    let mut plan = ScreenPlan::default();
    for group in REGISTRY {
        for side in [AxisSide::Entry, AxisSide::Exit] {
            let window = match group.kind {
                MetricKind::Dynamic => Some(match side {
                    AxisSide::Entry => cfg.entry_window_sec,
                    AxisSide::Exit => cfg.exit_window_sec,
                }),
                MetricKind::Static => None,
            };
            for m in group.metrics {
                let skip = |reason| Skipped { side, group: group.id, metric: m.id, reason };
                // Position-scoped: exit-only, and its values are declared.
                if group.scope == MetricScope::Position {
                    if side == AxisSide::Entry {
                        plan.skipped.push(skip(SkipReason::PositionIsExitOnly));
                        continue;
                    }
                    if POSITION_EXCLUDED.contains(&m.id) {
                        plan.skipped.push(skip(SkipReason::BaselineTpSl));
                        continue;
                    }
                    match POSITION_MENUS.iter().find(|(id, _)| *id == m.id) {
                        Some((_, menu)) => plan.metrics.push(ScreenMetric {
                            side,
                            group: group.id,
                            metric: m.id,
                            window,
                            source: ValueSource::Declared(menu),
                        }),
                        None => plan.skipped.push(skip(SkipReason::NoDeclaredMenu)),
                    }
                    continue;
                }
                // Fingerprint-scoped flow needs the run's compiled patterns.
                let column = if is_flow_metric(m.id) {
                    if cfg.flow_patterns.is_none() {
                        plan.skipped.push(skip(SkipReason::FlowPatternsMissing));
                        continue;
                    }
                    SeriesColumn::Flow(m.id, window, SWEEP_FLOW_FP)
                } else {
                    match window {
                        Some(w) => SeriesColumn::Window(m.id, w),
                        None => SeriesColumn::Static(m.id),
                    }
                };
                plan.metrics.push(ScreenMetric {
                    side,
                    group: group.id,
                    metric: m.id,
                    window,
                    source: ValueSource::Series(column),
                });
            }
        }
    }
    plan
}

// ───────────────────────────── percentile pass ─────────────────────────────

/// A deterministic, memory-bounded sample of one column's values.
///
/// Keeps every `keep_every`-th finite value; when the buffer hits `cap` it drops
/// every other survivor and doubles the stride, so the retained set stays a uniform
/// (order-spaced, not random) sample of everything seen at a bounded cost. Chosen
/// over reservoir sampling because it needs no RNG — the same corpus yields the same
/// anchors on every run, which a published candidate menu depends on.
#[derive(Clone, Debug)]
struct Reservoir {
    cap: usize,
    keep_every: u64,
    /// Count of finite values observed (also the stride cursor).
    n_finite: u64,
    /// Values the metric could not resolve (`NaN` before its first input).
    n_nonfinite: u64,
    buf: Vec<f64>,
}

impl Reservoir {
    fn new(cap: usize) -> Self {
        Self { cap: cap.max(2), keep_every: 1, n_finite: 0, n_nonfinite: 0, buf: Vec::new() }
    }

    fn push(&mut self, v: f64) {
        if !v.is_finite() {
            self.n_nonfinite += 1;
            return;
        }
        let keep = self.n_finite.is_multiple_of(self.keep_every);
        self.n_finite += 1;
        if !keep {
            return;
        }
        self.buf.push(v);
        if self.buf.len() >= self.cap {
            let mut i = 0usize;
            self.buf.retain(|_| {
                let keep = i.is_multiple_of(2);
                i += 1;
                keep
            });
            self.keep_every = self.keep_every.saturating_mul(2);
        }
    }

    fn finish(mut self, column: SeriesColumn) -> MetricPercentiles {
        self.buf.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mut ladder = [f64::NAN; PERCENTILE_LADDER.len()];
        if !self.buf.is_empty() {
            for (slot, q) in ladder.iter_mut().zip(PERCENTILE_LADDER) {
                *slot = exact_quantile_f64(&self.buf, q);
            }
        }
        MetricPercentiles {
            column,
            n_finite: self.n_finite,
            n_nonfinite: self.n_nonfinite,
            n_sampled: self.buf.len(),
            ladder,
        }
    }
}

/// One metric's measured distribution over the cohort.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MetricPercentiles {
    pub column: SeriesColumn,
    /// Finite values observed (the honest denominator behind the ladder).
    pub n_finite: u64,
    /// Events where the metric had no value yet (`NaN`) — excluded from the ladder.
    pub n_nonfinite: u64,
    /// Values actually retained for the quantile (≤ `sample_cap`).
    pub n_sampled: usize,
    /// Values at [`PERCENTILE_LADDER`], ascending. All `NaN` when nothing was seen.
    pub ladder: [f64; PERCENTILE_LADDER.len()],
}

impl MetricPercentiles {
    /// The measured value at ladder rung `q` (`None` if `q` isn't a rung).
    pub fn at(&self, q: f64) -> Option<f64> {
        PERCENTILE_LADDER
            .iter()
            .position(|p| (p - q).abs() < 1e-9)
            .map(|i| self.ladder[i])
    }
}

/// Measured distributions for a run's screenable columns.
#[derive(Clone, Debug, Default)]
pub struct PercentileTable(Vec<MetricPercentiles>);

impl PercentileTable {
    pub fn get(&self, column: SeriesColumn) -> Option<&MetricPercentiles> {
        self.0.iter().find(|p| p.column == column)
    }

    pub fn rows(&self) -> &[MetricPercentiles] {
        &self.0
    }
}

/// Measure every column in `plan` over `tokens` — **one pass, one series per
/// token** over the column union (plan §6.1). Values are sampled at trade moments
/// (no synthetic ticks): a tick can only re-read a decayed window or an advanced
/// clock, and weighting the ladder by wall-clock silence rather than by activity
/// would drag every menu toward dead-token values.
pub fn collect_percentiles(
    tokens: &[CorpusToken],
    plan: &ScreenPlan,
    cfg: &ScreenConfig,
) -> PercentileTable {
    let columns = plan.columns();
    if columns.is_empty() {
        return PercentileTable::default();
    }
    // Flow/price windows every dynamic column reads — registered once per series so
    // the whole trade history feeds them (mirrors `build_series_with_flow`).
    let windows: Vec<f64> = columns
        .iter()
        .filter_map(|c| match c {
            SeriesColumn::Flow(_, Some(w), _) | SeriesColumn::Window(_, w) => Some(*w),
            _ => None,
        })
        .collect();

    let mut res: Vec<Reservoir> = columns.iter().map(|_| Reservoir::new(cfg.sample_cap)).collect();
    for token in tokens {
        if token.trades.is_empty() {
            continue;
        }
        let mut series = MetricSeries::new(token.created_at, columns.clone());
        if let Some(patterns) = &cfg.flow_patterns {
            series.ensure_flow(SWEEP_FLOW_FP, patterns, &windows);
        }
        for t in token.trades.iter() {
            series.push_trade(to_trade_lite(t));
        }
        for (i, col) in columns.iter().enumerate() {
            let Some(ci) = series.col_index(*col) else { continue };
            for row in 0..series.n_rows() {
                res[i].push(series.value_at(row, ci));
            }
        }
    }
    PercentileTable(
        res.into_iter().zip(columns).map(|(r, col)| r.finish(col)).collect(),
    )
}

// ───────────────────────────── candidate menus ─────────────────────────────

/// A generated menu for one screened metric.
#[derive(Clone, Debug, PartialEq)]
pub struct MetricCandidates {
    pub metric: ScreenMetric,
    /// Operators to screen, primary first ([`DirectionPolicy`]).
    pub operators: Vec<Operator>,
    /// `off` (`None`) first, then the rounded anchors ascending — the exact shape
    /// [`AxisSpec::values`] takes.
    pub values: Vec<Option<f64>>,
    /// `(quantile, measured value)` behind each menu entry — the audit trail the
    /// hand-written anchor table used to carry. Empty for a declared menu.
    pub anchors: Vec<(f64, f64)>,
}

impl MetricCandidates {
    /// This menu as a sweep axis for `operator` — the handoff into
    /// [`AxesModel`](crate::sweep::generic::axes::AxesModel), so a generated menu
    /// is swept through exactly the path a hand-authored one is.
    pub fn axis_spec(&self, operator: Operator) -> AxisSpec {
        AxisSpec {
            kind: "metric".to_string(),
            side: Some(self.metric.side),
            group: Some(group_spec(self.metric.group).name.to_string()),
            metric: Some(self.metric.metric.name().to_string()),
            operator: Some(operator),
            window: self.metric.window,
            values: self.values.clone(),
        }
    }
}

/// Why a screened metric produced no usable menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuGap {
    /// The metric never resolved to a finite value on this cohort.
    NoSamples,
    /// Its p10..p90 collapse to fewer than two distinct rounded values — the metric
    /// is ~constant here, so every candidate would gate identically.
    Degenerate { distinct: usize },
}

/// Menus for a run, plus the metrics that yielded none and why.
#[derive(Clone, Debug, Default)]
pub struct MenuPlan {
    pub menus: Vec<MetricCandidates>,
    pub gaps: Vec<(ScreenMetric, MenuGap)>,
}

/// Generate the candidate menu for one screened metric.
pub fn candidate_menu(
    metric: ScreenMetric,
    table: &PercentileTable,
    cfg: &ScreenConfig,
) -> Result<MetricCandidates, MenuGap> {
    let operators = cfg.directions.operators().to_vec();
    let (values, anchors) = match metric.source {
        ValueSource::Declared(menu) => {
            (menu.iter().map(|v| Some(*v)).collect::<Vec<_>>(), Vec::new())
        }
        ValueSource::Series(col) => {
            let p = table.get(col).ok_or(MenuGap::NoSamples)?;
            if p.n_sampled == 0 {
                return Err(MenuGap::NoSamples);
            }
            let unit = hunter_engine::metrics::metric_spec(metric.metric).unit;
            let mut values: Vec<Option<f64>> = Vec::with_capacity(MENU_PERCENTILES.len());
            let mut anchors: Vec<(f64, f64)> = Vec::with_capacity(MENU_PERCENTILES.len());
            for q in MENU_PERCENTILES {
                let raw = p.at(q).expect("MENU_PERCENTILES ⊂ PERCENTILE_LADDER");
                if !raw.is_finite() {
                    continue;
                }
                let rounded = round_for_unit(raw, unit);
                // Dedup after rounding: adjacent rungs often collapse onto one
                // gate, and two identical values would only duplicate a combo.
                if values.contains(&Some(rounded)) {
                    continue;
                }
                values.push(Some(rounded));
                anchors.push((q, raw));
            }
            if values.len() < 2 {
                return Err(MenuGap::Degenerate { distinct: values.len() });
            }
            (values, anchors)
        }
    };
    // `off` first (pick 0) so a combo's marginal value is read straight off the
    // ranked table (the with-vs-without sentinel — `axes.rs`).
    let mut with_off: Vec<Option<f64>> = Vec::with_capacity(values.len() + 1);
    with_off.push(None);
    with_off.extend(values);
    Ok(MetricCandidates { metric, operators, values: with_off, anchors })
}

/// Generate menus for every metric in `plan`.
pub fn build_menus(plan: &ScreenPlan, table: &PercentileTable, cfg: &ScreenConfig) -> MenuPlan {
    let mut out = MenuPlan::default();
    for m in &plan.metrics {
        match candidate_menu(*m, table, cfg) {
            Ok(menu) => out.menus.push(menu),
            Err(gap) => out.gaps.push((*m, gap)),
        }
    }
    out
}

/// Round a measured anchor to a gate a human would author, by the metric's `unit`.
///
/// Percentiles land on values like `6.43176…`; a menu of those is unreadable and
/// implies a precision the sample doesn't support. The steps widen with magnitude
/// (the same spacing the hand-written menus use: `5/8/15/25` percent, `30/60/120/300`
/// seconds, `35/45/55/70` SOL) so a menu stays legible across three orders of
/// magnitude. Sign is preserved — flow metrics are legitimately negative.
pub fn round_for_unit(v: f64, unit: Unit) -> f64 {
    let mag = v.abs();
    let step = match unit {
        Unit::Seconds => {
            if mag < 10.0 {
                1.0
            } else if mag < 60.0 {
                5.0
            } else if mag < 600.0 {
                30.0
            } else {
                60.0
            }
        }
        Unit::Sol => {
            if mag < 1.0 {
                0.05
            } else if mag < 10.0 {
                0.5
            } else if mag < 100.0 {
                1.0
            } else {
                5.0
            }
        }
        Unit::Percent => {
            if mag < 1.0 {
                0.1
            } else if mag < 10.0 {
                1.0
            } else if mag < 100.0 {
                5.0
            } else {
                10.0
            }
        }
    };
    let r = (v / step).round() * step;
    // Kill float dust from the divide/multiply (`0.30000000000000004`) and `-0.0`.
    let r = (r * 1e6).round() / 1e6;
    if r == 0.0 {
        0.0
    } else {
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sweep::generic::axes::{AxesModel, AxesRequest};
    use crate::sweep::projection::CorpusTrade;
    use chrono::{Duration, TimeZone, Utc};
    use std::sync::Arc;

    fn cfg_with_patterns() -> ScreenConfig {
        ScreenConfig {
            flow_patterns: Some(FlowPatterns::from_label_sequences(&[vec!["Buy".to_string()]])),
            ..Default::default()
        }
    }

    /// The extensibility contract (plan §5): every registry metric, on every side,
    /// is either screened or carries a reason. A metric added to `REGISTRY` can
    /// never be silently unscreened.
    #[test]
    fn every_registry_metric_is_screened_or_reported() {
        for cfg in [ScreenConfig::default(), cfg_with_patterns()] {
            let plan = screen_plan(&cfg);
            for g in REGISTRY {
                for m in g.metrics {
                    for side in [AxisSide::Entry, AxisSide::Exit] {
                        let screened = plan
                            .metrics
                            .iter()
                            .any(|s| s.metric == m.id && s.group == g.id && s.side == side);
                        let reported = plan
                            .skipped
                            .iter()
                            .any(|s| s.metric == m.id && s.group == g.id && s.side == side);
                        assert!(
                            screened ^ reported,
                            "{}.{} ({side:?}) must be screened xor reported",
                            g.name,
                            m.name,
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn flow_split_needs_patterns() {
        let bare = screen_plan(&ScreenConfig::default());
        assert!(
            bare.skipped.iter().any(|s| s.group == MetricGroupId::FlowSplit
                && s.reason == SkipReason::FlowPatternsMissing),
            "flow-split must be skipped (with a reason) when no patterns are supplied",
        );
        assert!(!bare.metrics.iter().any(|m| is_flow_metric(m.metric)));

        let with = screen_plan(&cfg_with_patterns());
        assert!(with.metrics.iter().any(|m| m.group == MetricGroupId::FlowSplit));
        // Fingerprint-scoped columns carry the run's sweep fingerprint.
        assert!(with
            .metrics
            .iter()
            .filter(|m| is_flow_metric(m.metric))
            .all(|m| matches!(m.column(), Some(SeriesColumn::Flow(_, _, fp)) if fp == SWEEP_FLOW_FP)));
    }

    #[test]
    fn position_metrics_are_exit_only_with_declared_menus() {
        let plan = screen_plan(&ScreenConfig::default());
        let pos: Vec<_> =
            plan.metrics.iter().filter(|m| m.group == MetricGroupId::Position).collect();
        assert!(!pos.is_empty());
        assert!(pos.iter().all(|m| m.side == AxisSide::Exit), "m_position is exit-only");
        assert!(pos.iter().all(|m| matches!(m.source, ValueSource::Declared(_))));
        // Position metrics contribute no precompute column (mirrors `axes.rs`).
        assert!(pos.iter().all(|m| m.column().is_none()));
        // `pnl` is the baseline TP/SL, not a screened axis.
        assert!(plan.skipped.iter().any(|s| s.metric == MetricId::Pnl
            && s.side == AxisSide::Exit
            && s.reason == SkipReason::BaselineTpSl));
    }

    /// A new `m_position` metric must come with its declared menu (or an explicit
    /// exclusion) — otherwise it would silently never be screened.
    #[test]
    fn position_metrics_all_declared() {
        for g in REGISTRY.iter().filter(|g| g.scope == MetricScope::Position) {
            for m in g.metrics {
                let declared = POSITION_MENUS.iter().any(|(id, _)| *id == m.id);
                let excluded = POSITION_EXCLUDED.contains(&m.id);
                assert!(
                    declared ^ excluded,
                    "{}.{} needs a POSITION_MENUS entry or a POSITION_EXCLUDED entry (not both)",
                    g.name,
                    m.name,
                );
            }
        }
        // Both tables must reference live metrics, so neither can rot into a no-op.
        for (id, menu) in POSITION_MENUS {
            assert_eq!(hunter_engine::metrics::group_of(*id).scope, MetricScope::Position);
            assert!(menu.len() >= 2, "{} declared menu needs >= 2 values", id.name());
        }
        for id in POSITION_EXCLUDED {
            assert_eq!(hunter_engine::metrics::group_of(*id).scope, MetricScope::Position);
        }
    }

    #[test]
    fn dynamic_groups_take_the_side_window_and_dedupe_columns() {
        let cfg = ScreenConfig { entry_window_sec: 30.0, exit_window_sec: 10.0, ..Default::default() };
        let plan = screen_plan(&cfg);
        let win = |side, metric| {
            plan.metrics
                .iter()
                .find(|m| m.side == side && m.metric == metric)
                .and_then(|m| m.window)
        };
        assert_eq!(win(AxisSide::Entry, MetricId::NetFlow), Some(30.0));
        assert_eq!(win(AxisSide::Exit, MetricId::NetFlow), Some(10.0));
        // Static metrics carry no window and share ONE column across both sides.
        assert_eq!(win(AxisSide::Entry, MetricId::Time), None);
        let cols = plan.columns();
        assert_eq!(cols.iter().filter(|c| **c == SeriesColumn::Static(MetricId::Time)).count(), 1);
        assert!(cols.contains(&SeriesColumn::Window(MetricId::NetFlow, 30.0)));
        assert!(cols.contains(&SeriesColumn::Window(MetricId::NetFlow, 10.0)));
    }

    #[test]
    fn menu_percentiles_are_ladder_rungs() {
        for q in MENU_PERCENTILES {
            assert!(
                PERCENTILE_LADDER.iter().any(|p| (p - q).abs() < 1e-9),
                "{q} is not a ladder rung",
            );
        }
    }

    #[test]
    fn reservoir_is_exact_below_cap_and_bounded_above_it() {
        // Exact nearest-rank while everything fits.
        let mut r = Reservoir::new(1_000);
        for i in 1..=100 {
            r.push(i as f64);
        }
        r.push(f64::NAN);
        let p = r.clone().finish(SeriesColumn::Static(MetricId::Time));
        assert_eq!(p.n_finite, 100);
        assert_eq!(p.n_nonfinite, 1);
        assert_eq!(p.n_sampled, 100);
        // Nearest-rank over 1..=100: index `round(99 · q)`.
        assert_eq!(p.at(0.5), Some(51.0));
        assert_eq!(p.at(0.9), Some(90.0));
        assert_eq!(p.at(0.42), None);

        // Past the cap the buffer stays bounded and the ladder stays close.
        let cap = 64;
        let mut big = Reservoir::new(cap);
        for i in 1..=10_000 {
            big.push(i as f64);
        }
        let p = big.finish(SeriesColumn::Static(MetricId::Time));
        assert_eq!(p.n_finite, 10_000);
        assert!(p.n_sampled <= cap, "sample must stay bounded: {}", p.n_sampled);
        let median = p.at(0.5).unwrap();
        assert!((median - 5_000.0).abs() < 500.0, "decimated median drifted: {median}");
    }

    #[test]
    fn rounding_widens_with_magnitude_and_keeps_sign() {
        assert_eq!(round_for_unit(6.43, Unit::Percent), 6.0);
        assert_eq!(round_for_unit(22.8, Unit::Percent), 25.0);
        assert_eq!(round_for_unit(0.14, Unit::Percent), 0.1);
        assert_eq!(round_for_unit(-10.3, Unit::Sol), -10.0);
        assert_eq!(round_for_unit(52.6, Unit::Sol), 53.0);
        assert_eq!(round_for_unit(0.31, Unit::Sol), 0.3);
        assert_eq!(round_for_unit(98.0, Unit::Seconds), 90.0);
        assert_eq!(round_for_unit(1497.0, Unit::Seconds), 1500.0);
        // -0.0 never leaks into a menu (it would print as "-0").
        assert!(round_for_unit(-0.001, Unit::Sol).is_sign_positive());
    }

    /// Build a table by hand so the menu shaping is tested independently of a corpus.
    fn table_of(column: SeriesColumn, ladder: [f64; 8]) -> PercentileTable {
        PercentileTable(vec![MetricPercentiles {
            column,
            n_finite: 1_000,
            n_nonfinite: 0,
            n_sampled: 1_000,
            ladder,
        }])
    }

    #[test]
    fn menu_is_off_first_rounded_deduped_and_ascending() {
        let col = SeriesColumn::Static(MetricId::Trail);
        // p10..p90 of `trail` (HOT row of the anchor table).
        let table = table_of(col, [0.0, 0.1, 6.4, 22.8, 43.4, 61.0, 69.0, 79.0]);
        let m = ScreenMetric {
            side: AxisSide::Entry,
            group: MetricGroupId::PriceLifetime,
            metric: MetricId::Trail,
            window: None,
            source: ValueSource::Series(col),
        };
        let menu = candidate_menu(m, &table, &ScreenConfig::default()).unwrap();
        assert_eq!(menu.values[0], None, "`off` must be pick 0");
        assert_eq!(menu.values, vec![None, Some(0.1), Some(6.0), Some(25.0), Some(45.0), Some(60.0)]);
        // One anchor recorded per real value (off carries none).
        assert_eq!(menu.anchors.len(), menu.values.len() - 1);
        assert_eq!(menu.anchors[0].0, 0.10);
        assert_eq!(menu.operators, vec![Operator::Gte, Operator::Lt]);
    }

    #[test]
    fn degenerate_and_empty_metrics_are_reported_not_dropped() {
        let col = SeriesColumn::Static(MetricId::Liquidity);
        let m = ScreenMetric {
            side: AxisSide::Entry,
            group: MetricGroupId::Snapshot,
            metric: MetricId::Liquidity,
            window: None,
            source: ValueSource::Series(col),
        };
        // A constant metric: every rung rounds onto the same gate.
        let flat = table_of(col, [30.0; 8]);
        assert_eq!(
            candidate_menu(m, &flat, &ScreenConfig::default()),
            Err(MenuGap::Degenerate { distinct: 1 }),
        );
        // Never resolved on this cohort.
        let empty = PercentileTable(vec![MetricPercentiles {
            column: col,
            n_finite: 0,
            n_nonfinite: 500,
            n_sampled: 0,
            ladder: [f64::NAN; 8],
        }]);
        assert_eq!(candidate_menu(m, &empty, &ScreenConfig::default()), Err(MenuGap::NoSamples));
        // And a whole plan reports them instead of shrinking silently.
        let plan = ScreenPlan { metrics: vec![m], skipped: Vec::new() };
        let menus = build_menus(&plan, &flat, &ScreenConfig::default());
        assert!(menus.menus.is_empty());
        assert_eq!(menus.gaps.len(), 1);
    }

    #[test]
    fn declared_position_menu_needs_no_percentiles() {
        let m = ScreenMetric {
            side: AxisSide::Exit,
            group: MetricGroupId::Position,
            metric: MetricId::Retrace,
            window: None,
            source: ValueSource::Declared(&[5.0, 10.0, 25.0]),
        };
        let menu = candidate_menu(m, &PercentileTable::default(), &ScreenConfig::default()).unwrap();
        assert_eq!(menu.values, vec![None, Some(5.0), Some(10.0), Some(25.0)]);
        assert!(menu.anchors.is_empty());
    }

    /// The handoff: a generated menu must sweep through the *same* axes model a
    /// hand-authored one does — including the `off` sentinel and the window.
    #[test]
    fn generated_menu_resolves_as_a_sweep_axis() {
        let col = SeriesColumn::Window(MetricId::NetFlow, 30.0);
        let table = table_of(col, [-10.3, -6.5, -2.3, 0.1, 1.8, 4.9, 7.3, 14.0]);
        let m = ScreenMetric {
            side: AxisSide::Entry,
            group: MetricGroupId::FlowWindow,
            metric: MetricId::NetFlow,
            window: Some(30.0),
            source: ValueSource::Series(col),
        };
        let menu = candidate_menu(m, &table, &ScreenConfig::default()).unwrap();
        let spec = menu.axis_spec(menu.operators[0]);
        let model = AxesModel::resolve(&AxesRequest { axes: vec![spec] }).unwrap();
        assert_eq!(model.combo_count(), menu.values.len());
        assert_eq!(model.columns(), vec![col]);
        // Combo 0 is the off pick: no entry conditions at all.
        assert!(model.combo_params(0).entry.is_none());
    }

    // ── end-to-end over a synthetic corpus ─────────────────────────────────

    fn trade(secs: i64, price: f64, reserve: f64, is_buy: bool, sol: f64) -> CorpusTrade {
        CorpusTrade {
            flow: crate::sweep::projection::FlowKeys::default(),
            block_time: Utc.timestamp_opt(1_700_000_000, 0).unwrap() + Duration::seconds(secs),
            amount_sol: sol,
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

    #[test]
    fn collect_percentiles_measures_the_cohort() {
        let created = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let token = CorpusToken {
            mint: "mint".into(),
            symbol: "SYM".into(),
            created_at: created,
            trades: Arc::new(
                (0..10).map(|i| trade(i * 10, 1.0 + i as f64, 40.0 + i as f64, true, 1.0)).collect(),
            ),
            fp: Default::default(),
        identity: None,
        };
        let cfg = ScreenConfig::default();
        let plan = screen_plan(&cfg);
        let table = collect_percentiles(std::slice::from_ref(&token), &plan, &cfg);

        // Every screened column is measured.
        assert_eq!(table.rows().len(), plan.columns().len());
        // `time` runs 0..90s over the 10 trades — the median trade moment is ~45s.
        let time = table.get(SeriesColumn::Static(MetricId::Time)).expect("time measured");
        assert_eq!(time.n_finite, 10);
        assert_eq!(time.at(0.05), Some(0.0));
        assert_eq!(time.at(0.5), Some(50.0));
        assert_eq!(time.at(0.99), Some(90.0));
        // `liquidity` mirrors the reserve ramp, so its menu is a real ladder.
        let liq = table.get(SeriesColumn::Static(MetricId::Liquidity)).expect("liquidity measured");
        assert_eq!(liq.at(0.5), Some(45.0));
        let m = plan
            .metrics
            .iter()
            .find(|m| m.metric == MetricId::Liquidity && m.side == AxisSide::Entry)
            .copied()
            .expect("liquidity screened");
        let menu = candidate_menu(m, &table, &cfg).expect("liquidity menu");
        assert_eq!(menu.values, vec![None, Some(41.0), Some(42.0), Some(45.0), Some(47.0), Some(48.0)]);
        // A monotonically rising price never trails its peak ⇒ constant 0 ⇒ reported
        // as degenerate rather than sweeping five identical gates.
        let trail = plan
            .metrics
            .iter()
            .find(|m| m.metric == MetricId::Trail && m.side == AxisSide::Entry)
            .copied()
            .expect("trail screened");
        assert_eq!(
            candidate_menu(trail, &table, &cfg),
            Err(MenuGap::Degenerate { distinct: 1 }),
        );
    }
}
