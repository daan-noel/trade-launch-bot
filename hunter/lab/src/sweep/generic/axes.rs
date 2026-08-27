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
//!   value becomes a `{operator, value}` condition on that metric. A `null`
//!   value is the **off** sentinel: that combo simply omits the condition
//!   (sweeping with-vs-without in one grid). Off is metric-only.
//! * `take_profit` / `stop_loss` — each value sets the rule's TP / SL %.
//!
//! Group / metric are named (the registry enums aren't serde) and resolved
//! against [`hunter_engine::metrics`] so a typo is a hard error, never a silent
//! no-op — the same contract rule-save validation enforces.

use hunter_engine::metrics::evaluator::{coalesce_contributions, Condition, Operator};
use hunter_engine::metrics::series::SeriesColumn;
use hunter_engine::fingerprint::FingerprintId;
use hunter_engine::metrics::{
    group_by_name, group_spec, is_flow_metric, metric_spec, MetricGroupId, MetricId, MetricKind,
    MetricScope,
};
use uuid::Uuid;

/// Run-scoped fingerprint id for sweep flow state (compile_combo + SeriesColumn::Flow
/// share this so BoundCombo column indices line up). Promote materializes a real FP.
pub const SWEEP_FLOW_FP: FingerprintId = FingerprintId(Uuid::nil());
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
#[derive(Clone, Debug, Serialize, Deserialize)]
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
    /// The swept values. Must be non-empty; deduped + sorted on resolve. On a
    /// metric axis a `null` is the **off** sentinel (combo omits the condition);
    /// TP/SL axes reject it (absent TP/SL is authored by omitting the axis).
    pub values: Vec<Option<f64>>,
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
    /// A `None` value is the **off** pick — that combo carries no condition on
    /// this metric (sorted first, so pick 0 is always off when present).
    Metric {
        side: AxisSide,
        group: MetricGroupId,
        metric: MetricId,
        operator: Operator,
        /// Present iff the metric's group is dynamic (`m_flow_window`).
        window: Option<f64>,
        values: Vec<Option<f64>>,
    },
    /// Take-profit %.
    TakeProfit { values: Vec<f64> },
    /// Stop-loss %.
    StopLoss { values: Vec<f64> },
}

impl ResolvedAxis {
    /// The number of values this axis contributes to the combo product.
    pub fn value_count(&self) -> usize {
        match self {
            ResolvedAxis::Metric { values, .. } => values.len(),
            ResolvedAxis::TakeProfit { values } | ResolvedAxis::StopLoss { values } => values.len(),
        }
    }

    /// This axis's value at `pick` — `None` on a metric axis means the **off**
    /// sentinel, so the two `None`s are distinguished by the outer `Option`.
    pub fn value_at(&self, pick: usize) -> Option<Option<f64>> {
        match self {
            ResolvedAxis::Metric { values, .. } => values.get(pick).copied(),
            ResolvedAxis::TakeProfit { values } | ResolvedAxis::StopLoss { values } => {
                values.get(pick).copied().map(Some)
            }
        }
    }

    /// True for an axis that shapes the ENTRY side — used to keep entry axes as
    /// the high-order combo digits so same-entry combos stay contiguous (the
    /// engine's per-token entry cache then recomputes the entry once per block).
    fn is_entry(&self) -> bool {
        matches!(self, ResolvedAxis::Metric { side: AxisSide::Entry, .. })
    }

    /// The precompute column this axis reads (metric axes only). Position-scoped
    /// metrics (`m_position`) read no column: their value comes from the per-entry
    /// `PositionCtx` during the exit scan, not a token-independent series (a static
    /// column would only ever record `NaN`, since the track has no position state).
    fn column(&self) -> Option<SeriesColumn> {
        match self {
            ResolvedAxis::Metric { group, .. }
                if group_spec(*group).scope == MetricScope::Position =>
            {
                None
            }
            ResolvedAxis::Metric { metric, window, .. } => Some(if is_flow_metric(*metric) {
                SeriesColumn::Flow(*metric, window.map(hunter_engine::metrics::WindowSpec::secs), SWEEP_FLOW_FP)
            } else {
                match window {
                    Some(w) => SeriesColumn::window(*metric, hunter_engine::metrics::WindowSpec::secs(*w)),
                    None => SeriesColumn::Static(*metric),
                }
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
        // No shared-window constraint: `assemble` places each axis into the
        // `GroupConditions` instance matching its `window_size_sec`, so different
        // windows on the same (side, group) become distinct instances — exactly the
        // engine's multi-window-per-group model. Same-window axes merge; nothing
        // conflicts.
        // Entry axes first (high-order); the rest keep their relative order.
        resolved.sort_by_key(|a| !a.is_entry());
        Ok(Self { axes: resolved })
    }

    /// Total combos = product of every axis's value count. Uses checked
    /// multiplication — a wrapping `product()` can yield a bogus huge count that
    /// then tries to allocate petabytes when sampling a grid.
    pub fn combo_count(&self) -> usize {
        let mut n: usize = 1;
        for a in &self.axes {
            let len = a.value_count().max(1);
            match n.checked_mul(len) {
                Some(p) => n = p,
                None => return usize::MAX,
            }
        }
        n
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

    /// True when any axis references a volume-flow metric group.
    pub fn references_flow(&self) -> bool {
        self.axes.iter().any(|a| {
            matches!(
                a,
                ResolvedAxis::Metric {
                    group: MetricGroupId::FlowSplit | MetricGroupId::FlowSplitWindow,
                    ..
                }
            )
        })
    }

    /// Largest trailing window (`window_size_sec`) any metric axis reads, across both
    /// window families (`m_flow_window`/`m_flow_split_window` and `m_price_window`) —
    /// `0.0` if the swept rules read no windowed metrics. Sizes the sparse grid's
    /// decay region (plan §P2): past `last_trade + this`, every window flow is 0 and
    /// every rolling price extremum has aged out.
    pub fn max_window_secs(&self) -> f64 {
        self.axes
            .iter()
            .filter_map(|a| match a {
                ResolvedAxis::Metric { window: Some(w), .. } => Some(*w),
                _ => None,
            })
            .fold(0.0_f64, f64::max)
    }

    /// The largest swept condition value (+ the metric's `=`-tolerance) placed on
    /// `metric` across every axis and both sides — `0.0` if the metric isn't swept.
    /// For the monotone/static `time`/`stall` metrics this is the sparse grid's
    /// horizon: past it, no `metric` condition can change truth (plan §P2). The
    /// full `eq_tolerance` is a safe superset of every operator's settle point
    /// (including `=`'s upper tolerance edge and derived mono-bounds).
    pub fn metric_value_ceiling(&self, metric: MetricId) -> f64 {
        let mut max = 0.0_f64;
        let mut found = false;
        for a in &self.axes {
            if let ResolvedAxis::Metric { metric: m, values, .. } = a {
                if *m == metric {
                    for v in values.iter().flatten() {
                        max = max.max(*v);
                        found = true;
                    }
                }
            }
        }
        if found {
            max + hunter_engine::metrics::metric_spec(metric).eq_tolerance
        } else {
            0.0
        }
    }

    /// The `window_size_sec` used by the entry side's `m_flow_window` group (if
    /// any) — the number of high-order entry axes' first window. Used only by the
    /// entry-cache key packing; correctness comes from the assembled RuleParams.
    pub fn entry_axis_count(&self) -> usize {
        self.axes.iter().filter(|a| a.is_entry()).count()
    }

    /// The per-axis value indices combo `idx` picks — the mixed-radix decode every
    /// other combo accessor is defined in terms of (axis 0 most significant, so the
    /// entry axes at the front are the slowest-varying digits).
    ///
    /// SSOT: `combo_params` and `entry_key` read this decode rather than each
    /// carrying a copy of the loop. Discovery reads the picks directly (to compare
    /// which value a family's best combo landed on) — one decode, three readers.
    pub fn combo_picks(&self, idx: usize) -> Vec<usize> {
        let mut rem = idx;
        let mut picks = vec![0usize; self.axes.len()];
        for (a_idx, axis) in self.axes.iter().enumerate().rev() {
            let radix = axis.value_count().max(1);
            picks[a_idx] = rem % radix;
            rem /= radix;
        }
        picks
    }

    /// Assemble the `RuleParams` for combo `idx` by mixed-radix decoding (axis 0
    /// most significant). Panics only if `idx >= combo_count` (caller-guarded).
    pub fn combo_params(&self, idx: usize) -> RuleParams {
        self.assemble(&self.combo_picks(idx))
    }

    /// The packed entry-axis pick indices for combo `idx` — the engine's entry
    /// cache key (two combos with equal packing share an entry resolution).
    pub fn entry_key(&self, idx: usize) -> u64 {
        let picks = self.combo_picks(idx);
        // Pack only the entry-axis picks (the high-order front) into a key.
        let mut key = 0u64;
        for (a_idx, axis) in self.axes.iter().enumerate() {
            if axis.is_entry() {
                key = key
                    .wrapping_mul(axis.value_count() as u64 + 1)
                    .wrapping_add(picks[a_idx] as u64 + 1);
            }
        }
        key
    }

    fn assemble(&self, picks: &[usize]) -> RuleParams {
        // `reentry` / `exclusive` / `priority` are not sweepable axes — exclusivity is
        // a documented sweep divergence (docs/plans/sweep/sim-parity.md).
        let mut rp = RuleParams::default();
        for (axis, &pick) in self.axes.iter().zip(picks) {
            match axis {
                ResolvedAxis::TakeProfit { values } => rp.take_profit = Some(values[pick]),
                ResolvedAxis::StopLoss { values } => rp.stop_loss = Some(values[pick]),
                ResolvedAxis::Metric { side, group, metric, operator, window, values } => {
                    // The off pick: no condition, no group entry, no window —
                    // this combo behaves as if the axis were never authored.
                    let Some(val) = values[pick] else { continue };
                    let side_slot = match side {
                        AxisSide::Entry => &mut rp.entry,
                        AxisSide::Exit => &mut rp.exit,
                    };
                    let sc = side_slot.get_or_insert_with(SideConditions::default);
                    // One `GroupConditions` instance per distinct `window_size_sec`,
                    // mirroring the engine's multi-window-per-group model
                    // (`hunter_engine::rule_params`): axes sharing a window merge into
                    // the same instance, distinct windows each get their own. So
                    // `m_flow_window@30` and `m_flow_window@60` coexist, and a
                    // `m_price_window` window is independent of any flow window. A
                    // static group's axes all carry `window == None`, so they collapse
                    // to a single window-less instance. Created lazily on the first
                    // metric so an all-off group leaves no window-only instance behind.
                    let instances = sc.0.entry(*group).or_default();
                    let gc = match instances
                        .iter()
                        .position(|g| same_window(g.strict_param("window_size_sec"), *window))
                    {
                        Some(i) => &mut instances[i],
                        None => {
                            let mut g = GroupConditions::default();
                            if let Some(w) = window {
                                g.strict.insert("window_size_sec".to_string(), *w);
                            }
                            instances.push(g);
                            instances.last_mut().expect("just pushed")
                        }
                    };
                    // Stage each axis contribution as its own single-condition arm;
                    // coalesce below: AND when the combined list is satisfiable
                    // (range `> a, < b`), else one OR arm per contribution (`< a | > b`).
                    gc.metrics
                        .entry(*metric)
                        .or_default()
                        .push(vec![Condition { operator: *operator, value: val }]);
                }
            }
        }
        coalesce_side(&mut rp.entry);
        coalesce_side(&mut rp.exit);
        rp
    }
}

/// Flatten staged per-axis arms into the metric's DNF expr (same rule on entry
/// and exit): AND when satisfiable, else OR. That makes both orientations work:
/// * `> lows × < highs` → ranges when `a < b`, outside OR when crossed
/// * `< lows × > highs` → outside OR when crossed, ranges when the pair forms an interval
fn coalesce_side(side: &mut Option<SideConditions>) {
    let Some(side) = side else { return };
    for instances in side.0.values_mut() {
        for group in instances.iter_mut() {
            for (metric_id, arms) in group.metrics.iter_mut() {
                let flat: Vec<Condition> = arms.iter().flatten().copied().collect();
                let tol = metric_spec(*metric_id).eq_tolerance;
                *arms = coalesce_contributions(flat, tol);
            }
        }
    }
}

/// Resolve + validate a single axis spec against the registry.
fn resolve_one(spec: &AxisSpec) -> Result<ResolvedAxis, String> {
    if spec.values.is_empty() {
        return Err("`values` must be non-empty".to_string());
    }
    let has_off = spec.values.iter().any(Option::is_none);
    let mut numbers: Vec<f64> = spec.values.iter().flatten().copied().collect();
    if numbers.iter().any(|v| !v.is_finite()) {
        return Err("`values` must all be finite".to_string());
    }
    if numbers.is_empty() {
        // An all-off axis is the same as not authoring the axis — reject the no-op.
        return Err("`values` needs at least one number besides `off`".to_string());
    }
    // Dedup + sort for a stable, minimal grid.
    numbers.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    numbers.dedup_by(|a, b| (*a - *b).abs() < f64::EPSILON);

    match spec.kind.as_str() {
        "take_profit" | "stop_loss" => {
            if has_off {
                return Err(
                    "TP/SL axes cannot carry `off` — omit the axis to leave the guard off"
                        .to_string(),
                );
            }
            if numbers.iter().any(|v| *v <= 0.0) {
                return Err("TP/SL values must be > 0".to_string());
            }
            Ok(if spec.kind == "take_profit" {
                ResolvedAxis::TakeProfit { values: numbers }
            } else {
                ResolvedAxis::StopLoss { values: numbers }
            })
        }
        "metric" => {
            let side = spec.side.ok_or("metric axis needs `side`")?;
            let group_name = spec.group.as_deref().ok_or("metric axis needs `group`")?;
            let metric_name = spec.metric.as_deref().ok_or("metric axis needs `metric`")?;
            let operator = spec.operator.ok_or("metric axis needs `operator`")?;
            let group = group_by_name(group_name)
                .ok_or_else(|| format!("unknown metric group `{group_name}`"))?;
            // Position-scoped metrics (`m_position`) only exist while a position is
            // held — they read `NaN` before entry, so an entry condition on one can
            // never fire. Reject it on the entry side (mirrors the rule-save gate).
            if group.scope == MetricScope::Position && side == AxisSide::Entry {
                return Err(format!(
                    "group `{group_name}` is position-scoped (exit-only) — it has no value before entry; place it on the exit side"
                ));
            }
            let mspec = group
                .metric_by_name(metric_name)
                .ok_or_else(|| format!("metric `{metric_name}` not in group `{group_name}`"))?;
            // Creator-history metrics need the PG `tokens` row; the lake corpus a sweep
            // folds carries no creator column, so the value would be `NaN` for every
            // token and the axis would silently sweep a gate that never fires.
            if mspec.id.needs_creator_history() {
                return Err(format!(
                    "metric `{metric_name}` reads the token's CREATOR history, which the                      lake corpus does not carry - a sweep on it would score every cell on                      zero trades. Use `simulate`, which loads the creator from `tokens`."
                ));
            }
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
            // Off first (pick 0), then the numbers ascending.
            let mut values: Vec<Option<f64>> = Vec::with_capacity(numbers.len() + 1);
            if has_off {
                values.push(None);
            }
            values.extend(numbers.into_iter().map(Some));
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

/// Whether two group instances share a `window_size_sec` (so an axis merges into
/// an existing instance rather than opening a new one). Static-group axes carry
/// `None` and collapse to the single window-less instance. Matches the engine's
/// duplicate-window rule (`(a - b).abs() <= EPSILON`).
fn same_window(a: Option<f64>, b: Option<f64>) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => (x - y).abs() <= f64::EPSILON,
        (None, None) => true,
        _ => false,
    }
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
            values: vals.into_iter().map(Some).collect(),
        }
    }

    fn tp(vals: Vec<f64>) -> AxisSpec {
        AxisSpec {
            kind: "take_profit".to_string(),
            side: None,
            group: None,
            metric: None,
            operator: None,
            window: None,
            values: vals.into_iter().map(Some).collect(),
        }
    }

    #[test]
    fn combo_count_is_product_and_columns_dedup() {
        let req = AxesRequest {
            axes: vec![
                metric_axis(AxisSide::Entry, "m_snapshot", "time", ">", None, vec![5.0, 10.0, 15.0]),
                metric_axis(AxisSide::Entry, "m_flow_window", "net_flow", ">", Some(10.0), vec![0.0, 2.5]),
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
        let conds0 = &entry0.0[&MetricGroupId::Snapshot][0].metrics[&MetricId::Time];
        assert_eq!(conds0[0][0].value, 5.0);
        let entry1 = p1.entry.as_ref().unwrap();
        let conds1 = &entry1.0[&MetricGroupId::Snapshot][0].metrics[&MetricId::Time];
        assert_eq!(conds1[0][0].value, 10.0);
        // The assembled params must survive the canonical parse (promotable).
        RuleParams::parse(&p0.to_value()).unwrap();
    }

    #[test]
    fn off_pick_omits_the_condition() {
        let mut spec = metric_axis(AxisSide::Entry, "m_snapshot", "time", ">", None, vec![5.0]);
        spec.values.push(None); // off — must sort to pick 0
        let req = AxesRequest { axes: vec![spec, tp(vec![100.0])] };
        let m = AxesModel::resolve(&req).unwrap();
        assert_eq!(m.combo_count(), 2 * 1);
        // Combo 0 = the off pick: no entry conditions at all ⇒ enter on arm.
        let p0 = m.combo_params(0);
        assert!(p0.entry.is_none());
        assert!(p0.enter_on_arm());
        assert_eq!(p0.take_profit, Some(100.0));
        // Combo 1 = time > 5 as usual.
        let p1 = m.combo_params(1);
        let conds = &p1.entry.as_ref().unwrap().0[&MetricGroupId::Snapshot][0].metrics[&MetricId::Time];
        assert_eq!(conds[0][0].value, 5.0);
        // Both survive the canonical parse (promotable).
        RuleParams::parse(&p0.to_value()).unwrap();
        RuleParams::parse(&p1.to_value()).unwrap();
    }

    #[test]
    fn off_pick_on_dynamic_group_omits_window_too() {
        let mut spec =
            metric_axis(AxisSide::Entry, "m_flow_window", "net_flow", ">", Some(10.0), vec![2.5]);
        spec.values.insert(0, None);
        let req = AxesRequest { axes: vec![spec] };
        let m = AxesModel::resolve(&req).unwrap();
        // The off combo must not leave a metric-less group behind (validation
        // rejects a group carrying only window_size_sec).
        let p0 = m.combo_params(0);
        assert!(p0.entry.is_none());
        RuleParams::parse(&p0.to_value()).unwrap();
        RuleParams::parse(&m.combo_params(1).to_value()).unwrap();
    }

    #[test]
    fn all_off_axis_rejected() {
        let mut spec = metric_axis(AxisSide::Entry, "m_snapshot", "time", ">", None, vec![]);
        spec.values.push(None);
        let e = AxesModel::resolve(&AxesRequest { axes: vec![spec] }).unwrap_err();
        assert!(e.contains("besides `off`"), "{e}");
    }

    #[test]
    fn tp_sl_reject_off() {
        let mut spec = tp(vec![100.0]);
        spec.values.push(None);
        let e = AxesModel::resolve(&AxesRequest { axes: vec![spec] }).unwrap_err();
        assert!(e.contains("cannot carry `off`"), "{e}");
    }

    #[test]
    fn dynamic_metric_requires_window() {
        let req = AxesRequest {
            axes: vec![metric_axis(AxisSide::Entry, "m_flow_window", "buy", ">", None, vec![1.0])],
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
    fn position_group_rejected_on_entry_but_swept_on_exit() {
        // `m_position` only has a value while holding, so an entry condition on it
        // could never fire — reject the axis rather than sweep a dead grid.
        let entry = metric_axis(AxisSide::Entry, "m_position", "retrace", ">=", None, vec![3.0]);
        let e = AxesModel::resolve(&AxesRequest { axes: vec![entry] }).unwrap_err();
        assert!(e.contains("exit-only"), "{e}");

        // On the exit side it sweeps normally — but contributes NO precompute column
        // (the scan reads it from the per-entry PositionCtx, not a token series).
        let exit = metric_axis(AxisSide::Exit, "m_position", "retrace", ">=", None, vec![1.5, 3.0, 5.0, 10.0]);
        let m = AxesModel::resolve(&AxesRequest { axes: vec![exit] }).unwrap();
        assert_eq!(m.combo_count(), 4);
        assert!(m.columns().is_empty(), "position metrics must not become series columns");
        // The assembled params must survive the canonical parse (promotable to a rule).
        let p = m.combo_params(0);
        let arms = &p.exit.as_ref().unwrap().0[&MetricGroupId::Position][0].metrics[&MetricId::Retrace];
        assert_eq!(arms[0][0].value, 1.5);
        RuleParams::parse(&p.to_value()).unwrap();
    }

    #[test]
    fn price_window_axis_resolves_to_a_windowed_column() {
        // The dip trigger: `m_price_window(30).trail` — a dynamic group, so it takes a
        // window and precomputes as a Window column (routed to the price-extrema deque).
        let axis = metric_axis(
            AxisSide::Entry,
            "m_price_window",
            "trail",
            ">=",
            Some(30.0),
            vec![8.0, 15.0, 25.0],
        );
        let m = AxesModel::resolve(&AxesRequest { axes: vec![axis] }).unwrap();
        assert_eq!(m.combo_count(), 3);
        assert_eq!(m.columns(), vec![SeriesColumn::window(MetricId::WinTrail, hunter_engine::metrics::WindowSpec::secs(30.0))]);
        // The rolling high decays between trades, so it must size the sparse grid.
        assert_eq!(m.max_window_secs(), 30.0);
        RuleParams::parse(&m.combo_params(0).to_value()).unwrap();
    }

    #[test]
    fn same_group_distinct_windows_make_two_instances() {
        // Two `m_flow_window` axes with different windows on the same side are NOT a
        // conflict — they assemble into two `GroupConditions` instances (one per
        // window), exactly the engine's multi-window-per-group model. This is the
        // reported case: the exit side's `buy` at both 30 s and 60 s.
        let req = AxesRequest {
            axes: vec![
                metric_axis(AxisSide::Exit, "m_flow_window", "buy", "<", Some(30.0), vec![1.0]),
                metric_axis(AxisSide::Exit, "m_flow_window", "buy", "<", Some(60.0), vec![1.0]),
            ],
        };
        let m = AxesModel::resolve(&req).expect("distinct windows on one group must resolve");
        let p = m.combo_params(0);
        let insts = &p.exit.as_ref().unwrap().0[&MetricGroupId::FlowWindow];
        assert_eq!(insts.len(), 2, "one instance per distinct window");
        // Sorted ascending by window when serialized; assert both windows present.
        let windows: Vec<f64> =
            insts.iter().filter_map(|g| g.strict_param("window_size_sec")).collect();
        assert!(windows.contains(&30.0) && windows.contains(&60.0), "{windows:?}");
        // Each carries its own `buy` condition, and the whole thing is promotable.
        assert!(insts.iter().all(|g| g.metrics.contains_key(&MetricId::Buy)));
        RuleParams::parse(&p.to_value()).unwrap();
    }

    #[test]
    fn same_group_same_window_merges_into_one_instance() {
        // Two axes on the same (side, group, window) merge into ONE instance — the
        // engine rejects duplicate windows, so they must not become two.
        let req = AxesRequest {
            axes: vec![
                metric_axis(AxisSide::Entry, "m_flow_window", "buy", ">", Some(10.0), vec![1.0]),
                metric_axis(AxisSide::Entry, "m_flow_window", "sell", ">", Some(10.0), vec![1.0]),
            ],
        };
        let m = AxesModel::resolve(&req).unwrap();
        let p = m.combo_params(0);
        let insts = &p.entry.as_ref().unwrap().0[&MetricGroupId::FlowWindow];
        assert_eq!(insts.len(), 1, "same window ⇒ one instance");
        assert!(insts[0].metrics.contains_key(&MetricId::Buy));
        assert!(insts[0].metrics.contains_key(&MetricId::Sell));
        RuleParams::parse(&p.to_value()).unwrap();
    }

    #[test]
    fn distinct_groups_keep_independent_windows() {
        // The reported footgun: `m_price_window(5)` and `m_flow_window(3)` on the
        // same side are DIFFERENT groups, each with its own `window_size_sec`, so
        // the sizes are free to differ — this must resolve, not error.
        let req = AxesRequest {
            axes: vec![
                metric_axis(AxisSide::Entry, "m_price_window", "trail", ">", Some(5.0), vec![1.0, 3.0]),
                metric_axis(AxisSide::Entry, "m_flow_window", "gross_flow", "<", Some(3.0), vec![10.0, 15.0]),
            ],
        };
        let m = AxesModel::resolve(&req).expect("distinct groups may hold different windows");
        // Each group carries its own window in the assembled params.
        let p = m.combo_params(0);
        let entry = &p.entry.as_ref().unwrap().0;
        assert_eq!(entry[&MetricGroupId::PriceWindow][0].strict["window_size_sec"], 5.0);
        assert_eq!(entry[&MetricGroupId::FlowWindow][0].strict["window_size_sec"], 3.0);
        RuleParams::parse(&p.to_value()).unwrap();
    }

    #[test]
    fn same_group_two_windows_per_metric_promotable() {
        // A single metric (`trail`) swept at two `m_price_window` windows becomes two
        // instances, each with a `trail` condition — the engine accepts this.
        let req = AxesRequest {
            axes: vec![
                metric_axis(AxisSide::Entry, "m_price_window", "trail", ">", Some(5.0), vec![1.0]),
                metric_axis(AxisSide::Entry, "m_price_window", "trail", ">", Some(30.0), vec![1.0]),
            ],
        };
        let m = AxesModel::resolve(&req).expect("same metric at two windows must resolve");
        let p = m.combo_params(0);
        assert_eq!(p.entry.as_ref().unwrap().0[&MetricGroupId::PriceWindow].len(), 2);
        RuleParams::parse(&p.to_value()).unwrap();
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

    #[test]
    fn exit_range_orientation_ands_when_feasible() {
        // `> lows × < highs` — the range orientation: AND when a < b.
        let req = AxesRequest {
            axes: vec![
                metric_axis(AxisSide::Exit, "m_snapshot", "liquidity", ">", None, vec![0.0, 5.0]),
                metric_axis(AxisSide::Exit, "m_snapshot", "liquidity", "<", None, vec![40.0, 70.0]),
            ],
        };
        let m = AxesModel::resolve(&req).unwrap();
        assert_eq!(m.combo_count(), 4);
        for i in 0..m.combo_count() {
            let p = m.combo_params(i);
            let arms =
                &p.exit.as_ref().unwrap().0[&MetricGroupId::Snapshot][0].metrics[&MetricId::Liquidity];
            assert_eq!(arms.len(), 1, "combo {i}: feasible opposing bounds must AND");
            assert_eq!(arms[0].len(), 2);
            RuleParams::parse(&p.to_value()).unwrap();
        }
    }

    #[test]
    fn exit_outside_orientation_ors_when_crossed() {
        // `< low × > high` with crossed values → OR outside band.
        let req = AxesRequest {
            axes: vec![
                metric_axis(AxisSide::Exit, "m_snapshot", "liquidity", "<", None, vec![30.0]),
                metric_axis(AxisSide::Exit, "m_snapshot", "liquidity", ">", None, vec![70.0]),
            ],
        };
        let m = AxesModel::resolve(&req).unwrap();
        let p = m.combo_params(0);
        let arms = &p.exit.as_ref().unwrap().0[&MetricGroupId::Snapshot][0].metrics[&MetricId::Liquidity];
        assert_eq!(arms.len(), 2);
        assert_eq!(arms[0][0].operator, Operator::Lt);
        assert_eq!(arms[1][0].operator, Operator::Gt);
        RuleParams::parse(&p.to_value()).unwrap();
    }

    #[test]
    fn exit_outside_orientation_ands_when_pair_forms_interval() {
        // `< 10 × > 0` is a satisfiable interval, not an outside band.
        let req = AxesRequest {
            axes: vec![
                metric_axis(AxisSide::Exit, "m_snapshot", "liquidity", "<", None, vec![10.0]),
                metric_axis(AxisSide::Exit, "m_snapshot", "liquidity", ">", None, vec![0.0]),
            ],
        };
        let m = AxesModel::resolve(&req).unwrap();
        let p = m.combo_params(0);
        let arms = &p.exit.as_ref().unwrap().0[&MetricGroupId::Snapshot][0].metrics[&MetricId::Liquidity];
        assert_eq!(arms.len(), 1);
        assert_eq!(arms[0].len(), 2);
    }

    #[test]
    fn entry_same_metric_compatible_axes_stay_and() {
        let req = AxesRequest {
            axes: vec![
                metric_axis(AxisSide::Entry, "m_snapshot", "time", ">", None, vec![10.0]),
                metric_axis(AxisSide::Entry, "m_snapshot", "time", "<", None, vec![50.0]),
            ],
        };
        let m = AxesModel::resolve(&req).unwrap();
        let p = m.combo_params(0);
        let arms = &p.entry.as_ref().unwrap().0[&MetricGroupId::Snapshot][0].metrics[&MetricId::Time];
        assert_eq!(arms.len(), 1);
        assert_eq!(arms[0].len(), 2);
        RuleParams::parse(&p.to_value()).unwrap();
    }

    #[test]
    fn entry_same_metric_crossed_bounds_become_or() {
        let req = AxesRequest {
            axes: vec![
                metric_axis(AxisSide::Entry, "m_snapshot", "time", ">", None, vec![50.0]),
                metric_axis(AxisSide::Entry, "m_snapshot", "time", "<", None, vec![10.0]),
            ],
        };
        let m = AxesModel::resolve(&req).unwrap();
        let p = m.combo_params(0);
        let arms = &p.entry.as_ref().unwrap().0[&MetricGroupId::Snapshot][0].metrics[&MetricId::Time];
        assert_eq!(arms.len(), 2);
        RuleParams::parse(&p.to_value()).unwrap();
    }
}
