//! **Automated candidate generation** (plan §2.1) — the registry-driven candidate menus
//! the screen sweeps, in place of hand-derived anchor tables.
//!
//! For a cohort it
//!
//! 1. enumerates every screenable `(side, read)` straight off the registry
//!    ([`screen_plan`]): each metric over each tag and span it accepts. A new metric is
//!    included automatically, and anything excluded is reported with a [`SkipReason`],
//!    never dropped silently;
//! 2. measures each read's own distribution over the cohort ([`collect_percentiles`])
//!    into a percentile ladder;
//! 3. spaces a candidate menu off that ladder, rounded to the metric's unit, with the
//!    `off` pick first ([`build_menus`]) — ready to hand to the sweep as an
//!    [`AxisSpec`].
//!
//! ## Why the percentiles come from `MetricSeries`, not DuckDB SQL
//!
//! Only `age_sec` / `liquidity_sol` are raw lake columns; every trailing window, clock
//! and tag split is an *engine* quantity. A DuckDB version of them would be a second
//! implementation of metric semantics that can drift from [`hunter_engine`], and the
//! anchors would describe values the screen never gates on. So the ladder is measured
//! through the engine's own [`MetricSeries`] compute — the exact numbers the scan reads.
//!
//! Values are sampled at **trade moments** (trades folded with no synthetic ticks).

use hunter_engine::metrics::evaluator::Operator;
use hunter_engine::metrics::registry::METRICS;
use hunter_engine::metrics::series::{MetricSeries, SeriesColumn};
use hunter_engine::metrics::tags::config::CompiledTag;
use hunter_engine::metrics::{chart_reads, Family, Metric, MetricRef, MetricSpec, TagUse, Unit, WindowSpec};
use trading_core::strategies::kernel::exact_quantile_f64;

use crate::sweep::corpus::CorpusToken;
use crate::sweep::generic::axes::{AxisSide, AxisSpec, SWEEP_FLOW_FP};
use crate::sweep::generic::strategy::register_tags;
use crate::sweep::projection::to_trade_lite;

/// The percentile ladder every screenable read is measured on.
pub const PERCENTILE_LADDER: [f64; 8] = [0.05, 0.10, 0.25, 0.50, 0.75, 0.90, 0.95, 0.99];

/// The ladder rungs a candidate menu is spaced on (plan §2.1: `p10, p25, p50, p75,
/// p90` plus `off`). A strict subset of [`PERCENTILE_LADDER`].
pub const MENU_PERCENTILES: [f64; 5] = [0.10, 0.25, 0.50, 0.75, 0.90];

/// Declared menus for **position** metrics. They read *your* entry fill, so no
/// coin-side distribution exists to measure. Every position metric must appear here or
/// in [`POSITION_EXCLUDED`]; `position_metrics_all_declared` pins that.
const POSITION_MENUS: &[(Metric, &[f64])] = &[
    (Metric::RetracePct, &[5.0, 10.0, 15.0, 25.0, 40.0]),
    (Metric::BouncePct, &[5.0, 10.0, 15.0, 25.0, 40.0]),
    (Metric::HeldSec, &[30.0, 60.0, 120.0, 300.0, 600.0]),
];

/// Position metrics the screen does not sweep: `pnl_pct` IS the baseline TP/SL every
/// screening combo already carries; `stage_sec` equals `held_sec` in a one-stage combo;
/// `room_taken_pct` is fixed at the fill (it describes the entry, not a moment to sell).
const POSITION_EXCLUDED: &[Metric] = &[Metric::PnlPct, Metric::StageSec, Metric::RoomTakenPct];

// ───────────────────────────── configuration ───────────────────────────────

/// Which comparison directions the screen tries per read.
///
/// The default is **both**: the operator is an *output* of the screen ("keep, with the
/// suggested operator"), and most metrics earn a gate in either direction depending on
/// side. Measuring beats guessing, and it is additive (2 × 5 values).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DirectionPolicy {
    #[default]
    Both,
    Above,
    Below,
}

impl DirectionPolicy {
    /// The operators to screen, primary first.
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
    /// The span every windowed read is screened at on the **entry** side. Spans are
    /// compared across runs, not swept within one (plan §2.1).
    pub entry_window: WindowSpec,
    /// The span for windowed reads on the **exit** side.
    pub exit_window: WindowSpec,
    /// The nested slice a two-window read is screened over, as a fraction of the side's
    /// span — relative, because a slice only means anything inside its window and must
    /// stay in the window's unit.
    pub slice_fraction: f64,
    /// The run's tags. A read that needs a tag is screened once per tag (and its
    /// negation); with none, such reads are skipped ([`SkipReason::TagsMissing`]).
    pub tags: Vec<CompiledTag>,
    /// Per-read ceiling on retained samples (see [`Reservoir`]).
    pub sample_cap: usize,
    pub directions: DirectionPolicy,
}

impl Default for ScreenConfig {
    fn default() -> Self {
        Self {
            entry_window: WindowSpec::secs(30.0),
            exit_window: WindowSpec::secs(10.0),
            // A tenth of the window: wide enough that the ratio is not one print,
            // narrow enough that "recent" still means recent.
            slice_fraction: 0.1,
            tags: Vec::new(),
            // 200k samples per read ≈ 1.6 MB/column; the p10..p90 rungs are stable
            // long before this.
            sample_cap: 200_000,
            directions: DirectionPolicy::default(),
        }
    }
}

// ───────────────────────────── the screen plan ─────────────────────────────

/// Where a read's candidate values come from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ValueSource {
    /// Measured from the read's own distribution over the cohort, through this column.
    Series(SeriesColumn),
    /// Declared up front ([`POSITION_MENUS`]) — no cohort distribution exists.
    Declared(&'static [f64]),
}

/// One `(side, read)` the screen sweeps alone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenMetric {
    pub side: AxisSide,
    pub r: MetricRef,
    pub source: ValueSource,
}

impl ScreenMetric {
    /// The column this read measures (`None` for a declared menu).
    pub fn column(&self) -> Option<SeriesColumn> {
        match self.source {
            ValueSource::Series(c) => Some(c),
            ValueSource::Declared(_) => None,
        }
    }

    /// The read's family — what the family discovery groups by.
    pub fn family(&self) -> Family {
        self.r.metric.family()
    }

    /// `entry·m_flow.buy_sol @!volume [30s]` — how a report names it.
    pub fn name(&self) -> String {
        format!("{}·{}", side_str(self.side), self.r.label())
    }
}

/// `entry` / `exit`.
pub fn side_str(side: AxisSide) -> &'static str {
    match side {
        AxisSide::Entry => "entry",
        AxisSide::Exit => "exit",
    }
}

/// Why a registry metric is not screened on a side. Reported, never silently dropped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipReason {
    /// The metric reads only a tag and the run has none.
    TagsMissing,
    /// A position metric has no value before entry (`axes.rs` refuses the entry axis).
    PositionIsExitOnly,
    /// A position metric the screen does not sweep ([`POSITION_EXCLUDED`]).
    BaselineOrFixed,
    /// A position metric with no [`POSITION_MENUS`] entry — a new metric that needs its
    /// declared menu (guarded by `position_metrics_all_declared`).
    NoDeclaredMenu,
    /// A since-age read: its start age is a choice the screen's span vocabulary cannot
    /// make, and screening one arbitrary age would name a different metric.
    AnchorNotAScreenParam,
}

/// A registry metric the screen left out on a side, with the reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Skipped {
    pub side: AxisSide,
    pub metric: Metric,
    pub reason: SkipReason,
}

/// The screenable reads for a run, plus everything excluded and why.
#[derive(Clone, Debug, Default)]
pub struct ScreenPlan {
    pub metrics: Vec<ScreenMetric>,
    pub skipped: Vec<Skipped>,
}

impl ScreenPlan {
    /// The distinct columns the percentile pass records — one [`MetricSeries`] per
    /// token over this union serves every read.
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

/// The side's span for a metric, and the slice nested in it for a two-window read.
fn side_spans(spec: &MetricSpec, side: AxisSide, cfg: &ScreenConfig) -> Vec<WindowSpec> {
    let w = match side {
        AxisSide::Entry => cfg.entry_window,
        AxisSide::Exit => cfg.exit_window,
    };
    if spec.spans.slice {
        let slice = WindowSpec { size: (w.size * cfg.slice_fraction).max(1.0), lag: 0.0, unit: w.unit };
        vec![w, slice]
    } else {
        vec![w]
    }
}

/// Enumerate every screenable read off the registry for `cfg`: each metric's
/// [`chart_reads`] over the side's span and the run's tags, position metrics on the
/// exit side with their declared menus. A metric added to the registry surfaces here
/// with no edit.
pub fn screen_plan(cfg: &ScreenConfig) -> ScreenPlan {
    let mut plan = ScreenPlan::default();
    let trade: Vec<&str> = cfg.tags.iter().map(|t| t.name).collect();
    let template: Vec<&str> = cfg.tags.iter().filter(|t| t.patterns.templates().is_some()).map(|t| t.name).collect();
    for spec in METRICS {
        for side in [AxisSide::Entry, AxisSide::Exit] {
            let skip = |reason| Skipped { side, metric: spec.id, reason };
            if spec.family == Family::Position {
                if side == AxisSide::Entry {
                    plan.skipped.push(skip(SkipReason::PositionIsExitOnly));
                } else if POSITION_EXCLUDED.contains(&spec.id) {
                    plan.skipped.push(skip(SkipReason::BaselineOrFixed));
                } else {
                    match POSITION_MENUS.iter().find(|(id, _)| *id == spec.id) {
                        Some((_, menu)) => plan.metrics.push(ScreenMetric {
                            side,
                            r: MetricRef::life(spec.id),
                            source: ValueSource::Declared(menu),
                        }),
                        None => plan.skipped.push(skip(SkipReason::NoDeclaredMenu)),
                    }
                }
                continue;
            }
            let reads: Vec<MetricRef> = chart_reads(spec, &trade, &template, &side_spans(spec, side, cfg))
                .into_iter()
                .filter(|r| r.span.since_age.is_none())
                .collect();
            if reads.is_empty() {
                let reason = if spec.spans.since_age && !spec.spans.life && !spec.spans.window {
                    SkipReason::AnchorNotAScreenParam
                } else if spec.tags == TagUse::Required {
                    SkipReason::TagsMissing
                } else {
                    // Every metric has a life or a window read; reaching here means the
                    // registry grew a span kind this plan does not know.
                    SkipReason::AnchorNotAScreenParam
                };
                plan.skipped.push(skip(reason));
                continue;
            }
            for r in reads {
                let column = SeriesColumn { r, fp: r.is_fingerprint_scoped().then_some(SWEEP_FLOW_FP) };
                plan.metrics.push(ScreenMetric { side, r, source: ValueSource::Series(column) });
            }
        }
    }
    plan
}

// ───────────────────────────── percentile pass ─────────────────────────────

/// A deterministic, memory-bounded sample of one column's values.
///
/// Keeps every `keep_every`-th finite value; when the buffer hits `cap` it drops every
/// other survivor and doubles the stride, so the retained set stays a uniform sample
/// at a bounded cost. No RNG — the same corpus yields the same anchors on every run.
#[derive(Clone, Debug)]
struct Reservoir {
    cap: usize,
    keep_every: u64,
    n_finite: u64,
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
        MetricPercentiles { column, n_finite: self.n_finite, n_nonfinite: self.n_nonfinite, n_sampled: self.buf.len(), ladder }
    }
}

/// One read's measured distribution over the cohort.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MetricPercentiles {
    pub column: SeriesColumn,
    /// Finite values observed (the honest denominator behind the ladder).
    pub n_finite: u64,
    /// Events where the read had no value yet (`NaN`) — excluded from the ladder.
    pub n_nonfinite: u64,
    /// Values retained for the quantile (≤ `sample_cap`).
    pub n_sampled: usize,
    /// Values at [`PERCENTILE_LADDER`], ascending. All `NaN` when nothing was seen.
    pub ladder: [f64; PERCENTILE_LADDER.len()],
}

impl MetricPercentiles {
    /// The measured value at ladder rung `q` (`None` if `q` isn't a rung).
    pub fn at(&self, q: f64) -> Option<f64> {
        PERCENTILE_LADDER.iter().position(|p| (p - q).abs() < 1e-9).map(|i| self.ladder[i])
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

/// Measure every column in `plan` over `tokens` — one pass, one series per token over
/// the column union. Sampled at trade moments (no synthetic ticks): weighting the
/// ladder by wall-clock silence rather than activity would drag every menu toward
/// dead-coin values.
pub fn collect_percentiles(tokens: &[CorpusToken], plan: &ScreenPlan, cfg: &ScreenConfig) -> PercentileTable {
    let columns = plan.columns();
    if columns.is_empty() {
        return PercentileTable::default();
    }
    let mut res: Vec<Reservoir> = columns.iter().map(|_| Reservoir::new(cfg.sample_cap)).collect();
    for token in tokens {
        if token.trades.is_empty() {
            continue;
        }
        let mut series = MetricSeries::new(token.created_at, columns.clone());
        register_tags(&mut series, &cfg.tags);
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
    PercentileTable(res.into_iter().zip(columns).map(|(r, col)| r.finish(col)).collect())
}

// ───────────────────────────── candidate menus ─────────────────────────────

/// A generated menu for one screened read.
#[derive(Clone, Debug, PartialEq)]
pub struct MetricCandidates {
    pub metric: ScreenMetric,
    /// Operators to screen, primary first ([`DirectionPolicy`]).
    pub operators: Vec<Operator>,
    /// `off` (`None`) first, then the rounded anchors ascending — [`AxisSpec::values`].
    pub values: Vec<Option<f64>>,
    /// `(quantile, measured value)` behind each menu entry. Empty for a declared menu.
    pub anchors: Vec<(f64, f64)>,
}

impl MetricCandidates {
    /// This menu as a sweep axis for `operator` — swept through exactly the path a
    /// hand-authored axis is.
    pub fn axis_spec(&self, operator: Operator) -> AxisSpec {
        axis_spec_of(self.metric, operator, self.values.clone())
    }
}

/// The sweep axis for one screened read.
pub fn axis_spec_of(m: ScreenMetric, operator: Operator, values: Vec<Option<f64>>) -> AxisSpec {
    AxisSpec {
        kind: "metric".to_string(),
        side: Some(m.side),
        metric: Some(m.r.metric.spec().path()),
        tag: m.r.tag.map(|t| t.text()),
        span: m.r.span.span_text(),
        slice: m.r.span.slice_text(),
        operator: Some(operator),
        values,
    }
}

/// Why a screened read produced no usable menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuGap {
    /// The read never resolved to a finite value on this cohort.
    NoSamples,
    /// Its p10..p90 collapse to fewer than two distinct rounded values.
    Degenerate { distinct: usize },
}

/// Menus for a run, plus the reads that yielded none and why.
#[derive(Clone, Debug, Default)]
pub struct MenuPlan {
    pub menus: Vec<MetricCandidates>,
    pub gaps: Vec<(ScreenMetric, MenuGap)>,
}

/// Generate the candidate menu for one screened read.
pub fn candidate_menu(metric: ScreenMetric, table: &PercentileTable, cfg: &ScreenConfig) -> Result<MetricCandidates, MenuGap> {
    let operators = cfg.directions.operators().to_vec();
    let (values, anchors) = match metric.source {
        ValueSource::Declared(menu) => (menu.iter().map(|v| Some(*v)).collect::<Vec<_>>(), Vec::new()),
        ValueSource::Series(col) => {
            let p = table.get(col).ok_or(MenuGap::NoSamples)?;
            if p.n_sampled == 0 {
                return Err(MenuGap::NoSamples);
            }
            let unit = metric.r.metric.spec().unit;
            let mut values: Vec<Option<f64>> = Vec::with_capacity(MENU_PERCENTILES.len());
            let mut anchors: Vec<(f64, f64)> = Vec::with_capacity(MENU_PERCENTILES.len());
            for q in MENU_PERCENTILES {
                let raw = p.at(q).expect("MENU_PERCENTILES ⊂ PERCENTILE_LADDER");
                if !raw.is_finite() {
                    continue;
                }
                let rounded = round_for_unit(raw, unit);
                // Adjacent rungs often collapse onto one gate after rounding.
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
    // `off` first (pick 0), so a combo's marginal value reads straight off the table.
    let mut with_off: Vec<Option<f64>> = Vec::with_capacity(values.len() + 1);
    with_off.push(None);
    with_off.extend(values);
    Ok(MetricCandidates { metric, operators, values: with_off, anchors })
}

/// Generate menus for every read in `plan`.
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

/// Round a measured anchor to a gate a human would author, by the metric's unit. The
/// step widens with magnitude so a menu stays legible across orders of magnitude; the
/// sign is kept (flows are legitimately negative).
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
        // A tally has no sub-unit: every step stays an integer.
        Unit::Count => {
            if mag < 20.0 {
                1.0
            } else if mag < 100.0 {
                5.0
            } else {
                25.0
            }
        }
        // 0 or 1: the only gate is the flag itself.
        Unit::Flag => 1.0,
        Unit::Lamports => {
            if mag < 10_000.0 {
                100.0
            } else if mag < 1_000_000.0 {
                10_000.0
            } else if mag < 100_000_000.0 {
                1_000_000.0
            } else {
                10_000_000.0
            }
        }
    };
    let r = (v / step).round() * step;
    // Kill float dust (`0.30000000000000004`) and `-0.0`.
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
    use hunter_engine::metrics::tags::config::compile_tags;
    use std::sync::Arc;

    fn cfg_with_tags() -> ScreenConfig {
        ScreenConfig {
            tags: compile_tags(&serde_json::json!({
                "volume": { "match": { "ix_shape": [["Buy"]] } },
                "working": { "match": { "ix_template": ["Pump.Fun|200000|0|1|0|0"] } }
            })),
            ..Default::default()
        }
    }

    fn life(m: Metric) -> SeriesColumn {
        SeriesColumn::of(MetricRef::life(m))
    }

    fn windowed(m: Metric, w: WindowSpec) -> SeriesColumn {
        SeriesColumn::of(MetricRef::life(m).with_span(hunter_engine::metrics::Span::window(w)))
    }

    /// The extensibility contract: every registry metric, on every side, is screened or
    /// carries a reason. A metric added to the registry can never be silently unscreened.
    #[test]
    fn every_registry_metric_is_screened_or_reported() {
        for cfg in [ScreenConfig::default(), cfg_with_tags()] {
            let plan = screen_plan(&cfg);
            for m in METRICS {
                for side in [AxisSide::Entry, AxisSide::Exit] {
                    let screened = plan.metrics.iter().any(|s| s.r.metric == m.id && s.side == side);
                    let reported = plan.skipped.iter().any(|s| s.metric == m.id && s.side == side);
                    assert!(screened ^ reported, "{} ({side:?}) must be screened xor reported", m.path());
                }
            }
        }
    }

    #[test]
    fn tagged_reads_need_the_runs_tags() {
        let bare = screen_plan(&ScreenConfig::default());
        assert!(bare.skipped.iter().any(|s| s.metric == Metric::ProfitSol && s.reason == SkipReason::TagsMissing));
        assert!(!bare.metrics.iter().any(|m| m.r.is_fingerprint_scoped()));
        let with = screen_plan(&cfg_with_tags());
        let tagged: Vec<_> = with.metrics.iter().filter(|m| m.r.is_fingerprint_scoped()).collect();
        assert!(tagged.iter().any(|m| m.r.label() == "m_flow.buy_sol @!volume [30s]"));
        assert!(tagged.iter().all(|m| matches!(m.column(), Some(c) if c.fp == Some(SWEEP_FLOW_FP))));
    }

    #[test]
    fn position_metrics_are_exit_only_with_declared_menus() {
        let plan = screen_plan(&ScreenConfig::default());
        let pos: Vec<_> = plan.metrics.iter().filter(|m| m.r.is_position()).collect();
        assert!(!pos.is_empty());
        assert!(pos.iter().all(|m| m.side == AxisSide::Exit && matches!(m.source, ValueSource::Declared(_))));
        assert!(pos.iter().all(|m| m.column().is_none()), "a position read has no coin column");
        assert!(plan.skipped.iter().any(|s| s.metric == Metric::PnlPct && s.reason == SkipReason::BaselineOrFixed));
    }

    /// A new position metric must come with its declared menu or an explicit exclusion.
    #[test]
    fn position_metrics_all_declared() {
        for m in METRICS.iter().filter(|m| m.family == Family::Position) {
            let declared = POSITION_MENUS.iter().any(|(id, _)| *id == m.id);
            let excluded = POSITION_EXCLUDED.contains(&m.id);
            assert!(declared ^ excluded, "{} needs a POSITION_MENUS or a POSITION_EXCLUDED entry (not both)", m.path());
        }
        for (id, menu) in POSITION_MENUS {
            assert_eq!(id.family(), Family::Position);
            assert!(menu.len() >= 2);
        }
    }

    #[test]
    fn windowed_reads_take_the_side_span_and_life_reads_share_one_column() {
        let plan = screen_plan(&ScreenConfig::default());
        let has = |side, label: &str| plan.metrics.iter().any(|m| m.side == side && m.r.label() == label);
        assert!(has(AxisSide::Entry, "m_flow.net_sol [30s]"));
        assert!(has(AxisSide::Exit, "m_flow.net_sol [10s]"));
        assert!(has(AxisSide::Entry, "m_flow.net_sol"), "a life read is screened too");
        assert!(has(AxisSide::Entry, "m_flow.slice_trade_share_pct [30s, slice 3s]"));
        let cols = plan.columns();
        assert_eq!(cols.iter().filter(|c| **c == life(Metric::AgeSec)).count(), 1);
    }

    #[test]
    fn menu_percentiles_are_ladder_rungs() {
        for q in MENU_PERCENTILES {
            assert!(PERCENTILE_LADDER.iter().any(|p| (p - q).abs() < 1e-9), "{q} is not a ladder rung");
        }
    }

    #[test]
    fn reservoir_is_exact_below_cap_and_bounded_above_it() {
        let mut r = Reservoir::new(1_000);
        for i in 1..=100 {
            r.push(i as f64);
        }
        r.push(f64::NAN);
        let p = r.clone().finish(life(Metric::AgeSec));
        assert_eq!((p.n_finite, p.n_nonfinite, p.n_sampled), (100, 1, 100));
        assert_eq!(p.at(0.5), Some(51.0));
        assert_eq!(p.at(0.9), Some(90.0));
        assert_eq!(p.at(0.42), None);
        let cap = 64;
        let mut big = Reservoir::new(cap);
        for i in 1..=10_000 {
            big.push(i as f64);
        }
        let p = big.finish(life(Metric::AgeSec));
        assert_eq!(p.n_finite, 10_000);
        assert!(p.n_sampled <= cap);
        assert!((p.at(0.5).unwrap() - 5_000.0).abs() < 500.0);
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
        assert_eq!(round_for_unit(0.6, Unit::Flag), 1.0);
        assert_eq!(round_for_unit(123_456.0, Unit::Lamports), 120_000.0);
        assert!(round_for_unit(-0.001, Unit::Sol).is_sign_positive());
    }

    fn table_of(column: SeriesColumn, ladder: [f64; 8]) -> PercentileTable {
        PercentileTable(vec![MetricPercentiles { column, n_finite: 1_000, n_nonfinite: 0, n_sampled: 1_000, ladder }])
    }

    fn screened(side: AxisSide, col: SeriesColumn) -> ScreenMetric {
        ScreenMetric { side, r: col.r, source: ValueSource::Series(col) }
    }

    #[test]
    fn menu_is_off_first_rounded_deduped_and_ascending() {
        let col = life(Metric::TrailPct);
        let table = table_of(col, [0.0, 0.1, 6.4, 22.8, 43.4, 61.0, 69.0, 79.0]);
        let menu = candidate_menu(screened(AxisSide::Entry, col), &table, &ScreenConfig::default()).unwrap();
        assert_eq!(menu.values, vec![None, Some(0.1), Some(6.0), Some(25.0), Some(45.0), Some(60.0)]);
        assert_eq!(menu.anchors.len(), menu.values.len() - 1);
        assert_eq!(menu.operators, vec![Operator::Gte, Operator::Lt]);
    }

    #[test]
    fn degenerate_and_empty_reads_are_reported_not_dropped() {
        let col = life(Metric::LiquiditySol);
        let m = screened(AxisSide::Entry, col);
        let flat = table_of(col, [30.0; 8]);
        assert_eq!(candidate_menu(m, &flat, &ScreenConfig::default()), Err(MenuGap::Degenerate { distinct: 1 }));
        let empty = PercentileTable(vec![MetricPercentiles { column: col, n_finite: 0, n_nonfinite: 500, n_sampled: 0, ladder: [f64::NAN; 8] }]);
        assert_eq!(candidate_menu(m, &empty, &ScreenConfig::default()), Err(MenuGap::NoSamples));
        let menus = build_menus(&ScreenPlan { metrics: vec![m], skipped: Vec::new() }, &flat, &ScreenConfig::default());
        assert!(menus.menus.is_empty());
        assert_eq!(menus.gaps.len(), 1);
    }

    #[test]
    fn declared_position_menu_needs_no_percentiles() {
        let m = ScreenMetric { side: AxisSide::Exit, r: MetricRef::life(Metric::RetracePct), source: ValueSource::Declared(&[5.0, 10.0, 25.0]) };
        let menu = candidate_menu(m, &PercentileTable::default(), &ScreenConfig::default()).unwrap();
        assert_eq!(menu.values, vec![None, Some(5.0), Some(10.0), Some(25.0)]);
        assert!(menu.anchors.is_empty());
    }

    /// The handoff: a generated menu sweeps through the same axes model a hand-authored
    /// one does — the `off` pick and the span included.
    #[test]
    fn generated_menu_resolves_as_a_sweep_axis() {
        let col = windowed(Metric::NetSol, WindowSpec::secs(30.0));
        let table = table_of(col, [-10.3, -6.5, -2.3, 0.1, 1.8, 4.9, 7.3, 14.0]);
        let menu = candidate_menu(screened(AxisSide::Entry, col), &table, &ScreenConfig::default()).unwrap();
        let model = AxesModel::resolve(&AxesRequest { axes: vec![menu.axis_spec(menu.operators[0])] }).unwrap();
        assert_eq!(model.combo_count(), menu.values.len());
        assert_eq!(model.columns(), vec![col]);
        assert!(model.combo_params(0).enter.filters.is_empty(), "combo 0 is the off pick");
    }

    fn trade(secs: i64, price: f64, reserve: f64, is_buy: bool, sol: f64) -> CorpusTrade {
        CorpusTrade {
            flow: crate::sweep::projection::FlowKeys::default(),
            block_time: Utc.timestamp_opt(1_700_000_000, 0).unwrap() + Duration::seconds(secs),
            amount_sol: sol,
            token_amount: 1.0,
            price_per_token: price,
            reserve_sol: Some(reserve),
            // Consistent with the price: a fill prices off `reserve_sol / reserve_token`.
            reserve_token: Some(reserve / price),
            real_reserve_sol: Some(reserve),
            real_token_reserves: None,
            slot: secs as u64,
            tx_index: 0,
            leg_index: 0,
            is_buy,
            on_curve: true,
            venue_fee_bps: None,
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
            trades: Arc::new((0..10).map(|i| trade(i * 10, 1.0 + i as f64, 40.0 + i as f64, true, 1.0)).collect()),
            fp: Default::default(),
            identity: None,
            peak_after: None,
        };
        let cfg = ScreenConfig::default();
        let plan = screen_plan(&cfg);
        let table = collect_percentiles(std::slice::from_ref(&token), &plan, &cfg);
        assert_eq!(table.rows().len(), plan.columns().len());
        let age = table.get(life(Metric::AgeSec)).expect("age measured");
        assert_eq!(age.n_finite, 10);
        assert_eq!((age.at(0.05), age.at(0.5), age.at(0.99)), (Some(0.0), Some(50.0), Some(90.0)));
        let liq = table.get(life(Metric::LiquiditySol)).expect("liquidity measured");
        assert_eq!(liq.at(0.5), Some(45.0));
        let m = plan.metrics.iter().find(|m| m.r == MetricRef::life(Metric::LiquiditySol) && m.side == AxisSide::Entry).copied().unwrap();
        let menu = candidate_menu(m, &table, &cfg).expect("liquidity menu");
        assert_eq!(menu.values, vec![None, Some(41.0), Some(42.0), Some(45.0), Some(47.0), Some(48.0)]);
        // A rising price never trails its peak ⇒ constant 0 ⇒ degenerate, reported.
        let trail = plan.metrics.iter().find(|m| m.r == MetricRef::life(Metric::TrailPct) && m.side == AxisSide::Entry).copied().unwrap();
        assert_eq!(candidate_menu(trail, &table, &cfg), Err(MenuGap::Degenerate { distinct: 1 }));
    }
}
