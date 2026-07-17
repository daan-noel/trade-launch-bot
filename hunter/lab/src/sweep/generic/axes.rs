//! Generic sweep **axes** — the redesigned engine's replacement for the three
//! per-strategy `*Axes` structs (plan §5.4).
//!
//! An axis is one swept dimension. Each combo picks exactly one value from every
//! axis; the picked values assemble one [`RuleParams`] (the "WHEN it trades"
//! JSONB a rule stores). The precompute-then-scan sweep reads these `RuleParams`
//! through the **same** `hunter_engine` evaluator the live engine uses, so a
//! swept combo and a promoted rule trade identically by construction.
//!
//! Axis kinds (the wire `kind` tag, default `"metric"`):
//! * `metric` — a `(side, group, metric, operator[, window])` condition; each
//!   value becomes a `{operator, value}` condition on that metric.
//! * `take_profit` / `stop_loss` — each value sets the rule's TP / SL %.
//!
//! Group / metric are named (the registry enums aren't serde) and resolved
//! against [`hunter_engine::metrics`] so a typo is a hard error, never a silent
//! no-op — the same contract rule-save validation enforces.

use hunter_engine::metrics::evaluator::{Condition, Operator};
use hunter_engine::metrics::series::SeriesColumn;
use hunter_engine::metrics::{group_by_name, MetricGroupId, MetricId, MetricKind};
use hunter_engine::rule_params::{GroupConditions, RuleParams, SideConditions};
use serde::{Deserialize, Serialize};

/// Which side of the rule an axis conditions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AxisSide {
    Entry,
    Exit,
}

/// The raw wire form of one axis (from the request `axes.axes[]` array).
#[derive(Clone, Debug, Deserialize)]
pub struct AxisSpec {
    /// `"metric"` (default), `"take_profit"`, or `"stop_loss"`.
    #[serde(default = "default_kind")]
    pub kind: String,
    /// Metric axes only — which side the condition applies to.
    #[serde(default)]
    pub side: Option<AxisSide>,
    /// Metric axes only — the registry group name (e.g. `"m_snapshot"`).
    #[serde(default)]
    pub group: Option<String>,
    /// Metric axes only — the registry metric name (e.g. `"time"`).
    #[serde(default)]
    pub metric: Option<String>,
    /// Metric axes only — the comparison operator.
    #[serde(default)]
    pub operator: Option<Operator>,
    /// Dynamic-metric axes only — the trailing window (seconds).
    #[serde(default)]
    pub window: Option<f64>,
    /// The swept values. Must be non-empty; deduped + sorted on resolve.
    pub values: Vec<f64>,
}

fn default_kind() -> String {
    "metric".to_string()
}

/// The full request body's `axes` field.
#[derive(Clone, Debug, Deserialize, Default)]
pub struct AxesRequest {
    #[serde(default)]
    pub axes: Vec<AxisSpec>,
}

/// A validated, registry-resolved axis.
#[derive(Clone, Debug)]
pub enum ResolvedAxis {
    /// A metric condition axis: each value → `{operator, value}` on the metric.
    Metric {
        side: AxisSide,
        group: MetricGroupId,
        metric: MetricId,
        operator: Operator,
        /// Present iff the metric's group is dynamic (`m_time_window`).
        window: Option<f64>,
        values: Vec<f64>,
    },
    /// Take-profit %.
    TakeProfit { values: Vec<f64> },
    /// Stop-loss %.
    StopLoss { values: Vec<f64> },
}

impl ResolvedAxis {
    /// The number of values this axis contributes to the combo product.
    fn len(&self) -> usize {
        match self {
            ResolvedAxis::Metric { values, .. } => values.len(),
            ResolvedAxis::TakeProfit { values } | ResolvedAxis::StopLoss { values } => values.len(),
        }
    }

    fn value_at(&self, idx: usize) -> f64 {
        match self {
            ResolvedAxis::Metric { values, .. } => values[idx],
            ResolvedAxis::TakeProfit { values } | ResolvedAxis::StopLoss { values } => values[idx],
        }
    }

    /// True for an axis that shapes the ENTRY side — used to keep entry axes as
    /// the high-order combo digits so same-entry combos stay contiguous (the
    /// engine's per-token entry cache then recomputes the entry once per block).
    fn is_entry(&self) -> bool {
        matches!(self, ResolvedAxis::Metric { side: AxisSide::Entry, .. })
    }

    /// The precompute column this axis reads (metric axes only).
    fn column(&self) -> Option<SeriesColumn> {
        match self {
            ResolvedAxis::Metric { metric, window, .. } => Some(match window {
                Some(w) => SeriesColumn::Window(*metric, *w),
                None => SeriesColumn::Static(*metric),
            }),
            _ => None,
        }
    }
}

/// A resolved, validated axes model: the ordered axes plus derived combo math.
/// Entry axes are ordered before exit/TP/SL axes so a grid walk keeps each
/// distinct entry contiguous.
#[derive(Clone, Debug)]
pub struct AxesModel {
    /// Axes in combo-significance order (index 0 = most significant). Entry
    /// axes first (slowest-varying) so the exit sub-grid varies within one entry.
    pub axes: Vec<ResolvedAxis>,
}

impl AxesModel {
    /// Resolve + validate the wire specs against the metric registry.
    pub fn resolve(req: &AxesRequest) -> Result<Self, String> {
        if req.axes.is_empty() {
            return Err("at least one axis is required".to_string());
        }
        let mut resolved: Vec<ResolvedAxis> = Vec::with_capacity(req.axes.len());
        for (i, spec) in req.axes.iter().enumerate() {
            resolved.push(resolve_one(spec).map_err(|e| format!("axis {i}: {e}"))?);
        }
        // One window per (side, m_time_window group): RuleParams carries a single
        // `window_size_sec` per group per side, so time-window axes on one side
        // must agree. Reject a mixed set rather than silently dropping one.
        check_shared_windows(&resolved)?;
        // Entry axes first (high-order); the rest keep their relative order.
        resolved.sort_by_key(|a| !a.is_entry());
        Ok(Self { axes: resolved })
    }

    /// Total combos = product of every axis's value count.
    pub fn combo_count(&self) -> usize {
        self.axes.iter().map(|a| a.len()).product()
    }

    /// The distinct precompute columns every combo could read — the union fed to
    /// `MetricSeries` so one replay pass serves every combo.
    pub fn columns(&self) -> Vec<SeriesColumn> {
        let mut cols: Vec<SeriesColumn> = Vec::new();
        for a in &self.axes {
            if let Some(c) = a.column() {
                if !cols.contains(&c) {
                    cols.push(c);
                }
            }
        }
        cols
    }

    /// The `window_size_sec` used by the entry side's `m_time_window` group (if
    /// any) — the number of high-order entry axes' first window. Used only by the
    /// entry-cache key packing; correctness comes from the assembled RuleParams.
    pub fn entry_axis_count(&self) -> usize {
        self.axes.iter().filter(|a| a.is_entry()).count()
    }

    /// Assemble the `RuleParams` for combo `idx` by mixed-radix decoding (axis 0
    /// most significant). Panics only if `idx >= combo_count` (caller-guarded).
    pub fn combo_params(&self, idx: usize) -> RuleParams {
        let mut rem = idx;
        // Decode from the least-significant axis (last) upward so the entry axes
        // (front) are the slowest-varying digits.
        let mut picks = vec![0usize; self.axes.len()];
        for (a_idx, axis) in self.axes.iter().enumerate().rev() {
            let radix = axis.len().max(1);
            picks[a_idx] = rem % radix;
            rem /= radix;
        }
        self.assemble(&picks)
    }

    /// The packed entry-axis pick indices for combo `idx` — the engine's entry
    /// cache key (two combos with equal packing share an entry resolution).
    pub fn entry_key(&self, idx: usize) -> u64 {
        let mut rem = idx;
        let mut picks = vec![0usize; self.axes.len()];
        for (a_idx, axis) in self.axes.iter().enumerate().rev() {
            let radix = axis.len().max(1);
            picks[a_idx] = rem % radix;
            rem /= radix;
        }
        // Pack only the entry-axis picks (the high-order front) into a key.
        let mut key = 0u64;
        for (a_idx, axis) in self.axes.iter().enumerate() {
            if axis.is_entry() {
                key = key.wrapping_mul(axis.len() as u64 + 1).wrapping_add(picks[a_idx] as u64 + 1);
            }
        }
        key
    }

    fn assemble(&self, picks: &[usize]) -> RuleParams {
        let mut rp = RuleParams { take_profit: None, stop_loss: None, entry: None, exit: None };
        for (axis, &pick) in self.axes.iter().zip(picks) {
            let val = axis.value_at(pick);
            match axis {
                ResolvedAxis::TakeProfit { .. } => rp.take_profit = Some(val),
                ResolvedAxis::StopLoss { .. } => rp.stop_loss = Some(val),
                ResolvedAxis::Metric { side, group, metric, operator, window, .. } => {
                    let side_slot = match side {
                        AxisSide::Entry => &mut rp.entry,
                        AxisSide::Exit => &mut rp.exit,
                    };
                    let sc = side_slot.get_or_insert_with(SideConditions::default);
                    let gc = sc.0.entry(*group).or_insert_with(GroupConditions::default);
                    if let Some(w) = window {
                        gc.strict.insert("window_size_sec".to_string(), *w);
                    }
                    gc.metrics
                        .entry(*metric)
                        .or_default()
                        .push(Condition { operator: *operator, value: val });
                }
            }
        }
        rp
    }
}

/// Resolve + validate a single axis spec against the registry.
fn resolve_one(spec: &AxisSpec) -> Result<ResolvedAxis, String> {
    let mut values = spec.values.clone();
    if values.is_empty() {
        return Err("`values` must be non-empty".to_string());
    }
    if values.iter().any(|v| !v.is_finite()) {
        return Err("`values` must all be finite".to_string());
    }
    // Dedup + sort for a stable, minimal grid.
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    values.dedup_by(|a, b| (*a - *b).abs() < f64::EPSILON);

    match spec.kind.as_str() {
        "take_profit" | "stop_loss" => {
            if values.iter().any(|v| *v <= 0.0) {
                return Err("TP/SL values must be > 0".to_string());
            }
            Ok(if spec.kind == "take_profit" {
                ResolvedAxis::TakeProfit { values }
            } else {
                ResolvedAxis::StopLoss { values }
            })
        }
        "metric" => {
            let side = spec.side.ok_or("metric axis needs `side`")?;
            let group_name = spec.group.as_deref().ok_or("metric axis needs `group`")?;
            let metric_name = spec.metric.as_deref().ok_or("metric axis needs `metric`")?;
            let operator = spec.operator.ok_or("metric axis needs `operator`")?;
            let group = group_by_name(group_name)
                .ok_or_else(|| format!("unknown metric group `{group_name}`"))?;
            let mspec = group
                .metric_by_name(metric_name)
                .ok_or_else(|| format!("metric `{metric_name}` not in group `{group_name}`"))?;
            let window = match group.kind {
                MetricKind::Dynamic => Some(match spec.window {
                    Some(w) if w.is_finite() && w > 0.0 => w,
                    _ => {
                        return Err(format!(
                            "group `{group_name}` is dynamic — `window` (> 0) is required"
                        ))
                    }
                }),
                MetricKind::Static => None, // a window on a static metric is ignored
            };
            Ok(ResolvedAxis::Metric {
                side,
                group: group.id,
                metric: mspec.id,
                operator,
                window,
                values,
            })
        }
        other => Err(format!("unknown axis kind `{other}`")),
    }
}

/// Enforce one `window_size_sec` per (side, `m_time_window`) — RuleParams stores a
/// single window per group per side.
fn check_shared_windows(axes: &[ResolvedAxis]) -> Result<(), String> {
    for want_side in [AxisSide::Entry, AxisSide::Exit] {
        let mut window: Option<f64> = None;
        for a in axes {
            if let ResolvedAxis::Metric { side, group: MetricGroupId::TimeWindow, window: Some(w), .. } = a {
                if *side != want_side {
                    continue;
                }
                match window {
                    Some(prev) if (prev - *w).abs() > f64::EPSILON => {
                        return Err(format!(
                            "conflicting m_time_window windows on the {want_side:?} side ({prev} vs {w}) — one window per side"
                        ))
                    }
                    _ => window = Some(*w),
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metric_axis(kind_side: AxisSide, group: &str, metric: &str, op: &str, window: Option<f64>, vals: Vec<f64>) -> AxisSpec {
        AxisSpec {
            kind: "metric".to_string(),
            side: Some(kind_side),
            group: Some(group.to_string()),
            metric: Some(metric.to_string()),
            operator: Some(serde_json::from_str(&format!("\"{op}\"")).unwrap()),
            window,
            values: vals,
        }
    }

    fn tp(vals: Vec<f64>) -> AxisSpec {
        AxisSpec { kind: "take_profit".to_string(), side: None, group: None, metric: None, operator: None, window: None, values: vals }
    }

    #[test]
    fn combo_count_is_product_and_columns_dedup() {
        let req = AxesRequest {
            axes: vec![
                metric_axis(AxisSide::Entry, "m_snapshot", "time", ">", None, vec![5.0, 10.0, 15.0]),
                metric_axis(AxisSide::Entry, "m_time_window", "net_flow", ">", Some(10.0), vec![0.0, 2.5]),
                tp(vec![50.0, 100.0, 200.0]),
            ],
        };
        let m = AxesModel::resolve(&req).unwrap();
        assert_eq!(m.combo_count(), 3 * 2 * 3);
        // time (static) + net_flow@10 (window) = 2 columns.
        assert_eq!(m.columns().len(), 2);
    }

    #[test]
    fn combo_params_assembles_conditions_tp_sl() {
        let req = AxesRequest {
            axes: vec![
                metric_axis(AxisSide::Entry, "m_snapshot", "time", ">", None, vec![5.0, 10.0]),
                tp(vec![100.0]),
            ],
        };
        let m = AxesModel::resolve(&req).unwrap();
        // combo 0 → time>5, TP 100 ; combo 1 → time>10, TP 100 (entry is high-order)
        let p0 = m.combo_params(0);
        let p1 = m.combo_params(1);
        assert_eq!(p0.take_profit, Some(100.0));
        let entry0 = p0.entry.as_ref().unwrap();
        let conds0 = &entry0.0[&MetricGroupId::Snapshot].metrics[&MetricId::Time];
        assert_eq!(conds0[0].value, 5.0);
        let entry1 = p1.entry.as_ref().unwrap();
        let conds1 = &entry1.0[&MetricGroupId::Snapshot].metrics[&MetricId::Time];
        assert_eq!(conds1[0].value, 10.0);
        // The assembled params must survive the canonical parse (promotable).
        RuleParams::parse(&p0.to_value()).unwrap();
    }

    #[test]
    fn dynamic_metric_requires_window() {
        let req = AxesRequest {
            axes: vec![metric_axis(AxisSide::Entry, "m_time_window", "buy", ">", None, vec![1.0])],
        };
        assert!(AxesModel::resolve(&req).is_err());
    }

    #[test]
    fn unknown_group_rejected() {
        let req = AxesRequest {
            axes: vec![metric_axis(AxisSide::Entry, "m_bogus", "x", ">", None, vec![1.0])],
        };
        assert!(AxesModel::resolve(&req).is_err());
    }

    #[test]
    fn conflicting_windows_rejected() {
        let req = AxesRequest {
            axes: vec![
                metric_axis(AxisSide::Entry, "m_time_window", "buy", ">", Some(10.0), vec![1.0]),
                metric_axis(AxisSide::Entry, "m_time_window", "sell", ">", Some(20.0), vec![1.0]),
            ],
        };
        assert!(AxesModel::resolve(&req).is_err());
    }

    #[test]
    fn entry_axes_ordered_before_exit() {
        let req = AxesRequest {
            axes: vec![
                tp(vec![100.0, 200.0]),
                metric_axis(AxisSide::Entry, "m_snapshot", "time", ">", None, vec![5.0, 10.0]),
            ],
        };
        let m = AxesModel::resolve(&req).unwrap();
        assert!(m.axes[0].is_entry(), "entry axis must sort first");
    }
}
