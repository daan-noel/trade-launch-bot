//! Sweep **axes** — the dimensions a grouped sweep varies.
//!
//! Each combo picks one value from every axis, and the picks assemble one rule
//! ([`RuleParams`]) that the scan runs through the engine's own decision code, so a
//! swept combo and the promoted rule trade identically.
//!
//! Axis kinds (the wire `kind`, default `"metric"`):
//! * `metric` — one condition `metric @tag [span] <operator> value`, where `side` says
//!   where it goes: `entry` ⇒ an `enter.filters` condition (the coin must pass it to be
//!   bought), `exit` ⇒ its own `always` sell line (sell everything when it holds). A
//!   `null` value is the **off** pick: that combo leaves the condition out, so one
//!   grid sweeps with-vs-without.
//! * `take_profit` / `stop_loss` — each value sets the rule's TP / SL %.
//!
//! Example: `{"side": "entry", "metric": "m_flow.buy_sol", "tag": "!volume",
//! "span": "10s", "operator": ">=", "values": [1, 2, 4]}` sweeps "SOL bought by
//! trades without `volume` in the last 10 s is at least 1 / 2 / 4".
//!
//! Two axes on the same read (metric + tag + span) join into one condition: AND when
//! the pair can hold together (`> 5` and `< 50`), else OR (`< 5` or `> 50`).

use hunter_engine::fingerprint::FingerprintId;
use hunter_engine::metrics::evaluator::{coalesce_contributions, Condition, Operator};
use hunter_engine::metrics::series::SeriesColumn;
use hunter_engine::metrics::{metric_spec, Metric, MetricRef, WindowUnit};
use hunter_engine::rule_params::{Cond, Line, RuleParams, Sell};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The fingerprint id a sweep's tag reads are scoped to. The combo's compiled rule and
/// the run's series columns share it, so their column indices line up. Promote writes
/// the run's tags onto a real fingerprint.
pub const SWEEP_FLOW_FP: FingerprintId = FingerprintId(Uuid::nil());

/// Where a metric axis's condition goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AxisSide {
    /// An `enter.filters` condition.
    Entry,
    /// Its own `always` sell line.
    Exit,
}

/// The wire form of one axis (the request's `axes.axes[]`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxisSpec {
    /// `"metric"` (default), `"take_profit"`, or `"stop_loss"`.
    #[serde(default = "default_kind")]
    pub kind: String,
    /// Metric axes only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<AxisSide>,
    /// Metric axes only — the registry path, `m_family.name`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric: Option<String>,
    /// Metric axes only — `volume`, `!volume`, a built-in class.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// Metric axes only — the span, `10s`, `30sl@1`, `20p`, `age60s`; absent = life.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<String>,
    /// Metric axes only — the nested slice of a two-window read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slice: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator: Option<Operator>,
    /// The swept values. On a metric axis a `null` is the **off** pick.
    pub values: Vec<Option<f64>>,
}

fn default_kind() -> String {
    "metric".to_string()
}

/// The request body's `axes` field.
#[derive(Clone, Debug, Deserialize, Default)]
pub struct AxesRequest {
    #[serde(default)]
    pub axes: Vec<AxisSpec>,
}

/// A validated axis.
#[derive(Clone, Debug)]
pub enum ResolvedAxis {
    /// Each value → `{operator, value}` on `r`; `None` is the off pick (sorted first).
    Metric { side: AxisSide, r: MetricRef, operator: Operator, values: Vec<Option<f64>> },
    TakeProfit { values: Vec<f64> },
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

    /// This axis's value at `pick` — an inner `None` is a metric axis's off pick.
    pub fn value_at(&self, pick: usize) -> Option<Option<f64>> {
        match self {
            ResolvedAxis::Metric { values, .. } => values.get(pick).copied(),
            ResolvedAxis::TakeProfit { values } | ResolvedAxis::StopLoss { values } => values.get(pick).copied().map(Some),
        }
    }

    /// An entry axis. Entry axes are the high-order combo digits, so combos sharing an
    /// entry stay contiguous and the per-token entry walk is shared.
    fn is_entry(&self) -> bool {
        matches!(self, ResolvedAxis::Metric { side: AxisSide::Entry, .. })
    }

    /// The series column this axis reads. A position metric reads none: it comes from
    /// the held position during the exit scan, not from the coin.
    fn column(&self) -> Option<SeriesColumn> {
        match self {
            ResolvedAxis::Metric { r, .. } if !r.is_position() => Some(SeriesColumn {
                r: *r,
                fp: r.is_fingerprint_scoped().then_some(SWEEP_FLOW_FP),
            }),
            _ => None,
        }
    }
}

/// A resolved axes model: the ordered axes and the combo math.
#[derive(Clone, Debug)]
pub struct AxesModel {
    /// Combo-significance order (index 0 most significant): entry axes first.
    pub axes: Vec<ResolvedAxis>,
}

impl AxesModel {
    /// Resolve and validate the wire specs against the registry.
    pub fn resolve(req: &AxesRequest) -> Result<Self, String> {
        if req.axes.is_empty() {
            return Err("at least one axis is required".to_string());
        }
        let mut resolved = Vec::with_capacity(req.axes.len());
        for (i, spec) in req.axes.iter().enumerate() {
            resolved.push(resolve_one(spec).map_err(|e| format!("axis {i}: {e}"))?);
        }
        resolved.sort_by_key(|a| !a.is_entry());
        Ok(Self { axes: resolved })
    }

    /// Total combos = product of every axis's value count (`usize::MAX` on overflow).
    pub fn combo_count(&self) -> usize {
        let mut n: usize = 1;
        for a in &self.axes {
            match n.checked_mul(a.value_count().max(1)) {
                Some(p) => n = p,
                None => return usize::MAX,
            }
        }
        n
    }

    /// The distinct series columns every combo could read.
    pub fn columns(&self) -> Vec<SeriesColumn> {
        let mut cols: Vec<SeriesColumn> = Vec::new();
        for c in self.axes.iter().filter_map(ResolvedAxis::column) {
            if !cols.contains(&c) {
                cols.push(c);
            }
        }
        cols
    }

    /// The tags the axes read (without `!`), for the caller to check against the run's
    /// tags document.
    pub fn tag_names(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        for a in &self.axes {
            if let ResolvedAxis::Metric { r, .. } = a {
                if let Some(t) = r.tag.filter(|t| !t.is_builtin()) {
                    if !out.contains(&t.name) {
                        out.push(t.name);
                    }
                }
            }
        }
        out
    }

    /// Whether any axis reads a tag.
    pub fn references_tags(&self) -> bool {
        self.axes.iter().any(|a| matches!(a, ResolvedAxis::Metric { r, .. } if r.tag.is_some()))
    }

    /// Largest span any metric axis reads, in seconds (slot spans at the nominal slot
    /// time, print spans as 0 — a print moves only on a trade, which emits its own row).
    /// Sizes the sparse grid's decay region; a horizon, never a reading.
    pub fn max_window_secs(&self) -> f64 {
        self.axes
            .iter()
            .filter_map(|a| match a {
                ResolvedAxis::Metric { r, .. } => Some([r.span.window, r.span.slice]),
                _ => None,
            })
            .flatten()
            .flatten()
            .map(|w| match w.unit {
                WindowUnit::Sec => w.size + w.lag,
                WindowUnit::Slot => (w.size + w.lag) * hunter_engine::metrics::NOMINAL_SLOT_SECS,
                WindowUnit::Print => 0.0,
            })
            .fold(0.0_f64, f64::max)
    }

    /// The largest swept value on `metric` (+ its `=` tolerance), `0.0` when unswept —
    /// the sparse grid's horizon for a clock metric: past it no condition on the clock
    /// changes truth.
    pub fn metric_value_ceiling(&self, metric: Metric) -> f64 {
        let mut max: Option<f64> = None;
        for a in &self.axes {
            if let ResolvedAxis::Metric { r, values, .. } = a {
                if r.metric == metric {
                    for v in values.iter().flatten() {
                        max = Some(max.map_or(*v, |m: f64| m.max(*v)));
                    }
                }
            }
        }
        max.map_or(0.0, |m| m + metric_spec(metric).eq_tolerance)
    }

    /// The number of entry axes.
    pub fn entry_axis_count(&self) -> usize {
        self.axes.iter().filter(|a| a.is_entry()).count()
    }

    /// The per-axis value indices combo `idx` picks (mixed radix, axis 0 most
    /// significant). The one decode every combo accessor reads.
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

    /// The rule combo `idx` assembles.
    pub fn combo_params(&self, idx: usize) -> RuleParams {
        self.assemble(&self.combo_picks(idx))
    }

    /// The packed entry-axis picks of combo `idx` — two combos with the same key share
    /// their whole entry side.
    pub fn entry_key(&self, idx: usize) -> u64 {
        let picks = self.combo_picks(idx);
        let mut key = 0u64;
        for (a_idx, axis) in self.axes.iter().enumerate() {
            if axis.is_entry() {
                key = key.wrapping_mul(axis.value_count() as u64 + 1).wrapping_add(picks[a_idx] as u64 + 1);
            }
        }
        key
    }

    fn assemble(&self, picks: &[usize]) -> RuleParams {
        // Re-entry, exclusivity and priority are not sweepable — exclusivity is a
        // documented sweep difference (docs/plans/sweep/sim-parity.md).
        let mut rp = RuleParams::default();
        // Per side, one condition per read, in first-seen order.
        let mut entry: Vec<(MetricRef, Vec<Condition>)> = Vec::new();
        let mut exit: Vec<(MetricRef, Vec<Condition>)> = Vec::new();
        for (axis, &pick) in self.axes.iter().zip(picks) {
            match axis {
                ResolvedAxis::TakeProfit { values } => rp.take_profit = Some(values[pick]),
                ResolvedAxis::StopLoss { values } => rp.stop_loss = Some(values[pick]),
                ResolvedAxis::Metric { side, r, operator, values } => {
                    // The off pick: this combo behaves as if the axis were never authored.
                    let Some(value) = values[pick] else { continue };
                    let list = match side {
                        AxisSide::Entry => &mut entry,
                        AxisSide::Exit => &mut exit,
                    };
                    let c = Condition { operator: *operator, value };
                    match list.iter_mut().find(|(x, _)| x == r) {
                        Some((_, cs)) => cs.push(c),
                        None => list.push((*r, vec![c])),
                    }
                }
            }
        }
        let cond = |r: MetricRef, cs: Vec<Condition>| Cond::Metric {
            r,
            is: coalesce_contributions(cs, metric_spec(r.metric).eq_tolerance),
            off: false,
        };
        rp.enter.filters = entry.into_iter().map(|(r, cs)| cond(r, cs)).collect();
        rp.always = exit
            .into_iter()
            .map(|(r, cs)| Line { when: vec![cond(r, cs)], sell: Some(Sell { label: None, pct: None }), go: None, off: false })
            .collect();
        rp
    }
}

/// Resolve and validate one axis spec.
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
        return Err("`values` needs at least one number besides `off`".to_string());
    }
    numbers.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    numbers.dedup_by(|a, b| (*a - *b).abs() < f64::EPSILON);

    match spec.kind.as_str() {
        "take_profit" | "stop_loss" => {
            if has_off {
                return Err("TP/SL axes cannot carry `off` — omit the axis to leave it off".to_string());
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
            let side = spec.side.ok_or("a metric axis needs `side` (entry or exit)")?;
            let operator = spec.operator.ok_or("a metric axis needs `operator`")?;
            // One parser for every read: the rule's own.
            let mut obj = serde_json::Map::new();
            obj.insert("metric".into(), serde_json::json!(spec.metric.as_deref().ok_or("a metric axis needs `metric`")?));
            for (k, v) in [("tag", &spec.tag), ("span", &spec.span), ("slice", &spec.slice)] {
                if let Some(v) = v {
                    obj.insert(k.into(), serde_json::json!(v));
                }
            }
            let r = MetricRef::from_json(&obj)?;
            if r.is_position() && side == AxisSide::Entry {
                return Err(format!(
                    "`{}` reads our position, which has no value before the buy — put it on the exit side",
                    r.label()
                ));
            }
            let mut values: Vec<Option<f64>> = Vec::with_capacity(numbers.len() + 1);
            if has_off {
                values.push(None);
            }
            values.extend(numbers.into_iter().map(Some));
            Ok(ResolvedAxis::Metric { side, r, operator, values })
        }
        other => Err(format!("unknown axis kind `{other}`")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hunter_engine::rule_params::RuleParams;

    fn axis(side: AxisSide, metric: &str, span: Option<&str>, op: &str, vals: Vec<f64>) -> AxisSpec {
        AxisSpec {
            kind: "metric".to_string(),
            side: Some(side),
            metric: Some(metric.to_string()),
            tag: None,
            span: span.map(str::to_string),
            slice: None,
            operator: Some(serde_json::from_str(&format!("\"{op}\"")).unwrap()),
            values: vals.into_iter().map(Some).collect(),
        }
    }

    fn tp(vals: Vec<f64>) -> AxisSpec {
        AxisSpec {
            kind: "take_profit".to_string(),
            side: None,
            metric: None,
            tag: None,
            span: None,
            slice: None,
            operator: None,
            values: vals.into_iter().map(Some).collect(),
        }
    }

    fn model(axes: Vec<AxisSpec>) -> AxesModel {
        AxesModel::resolve(&AxesRequest { axes }).expect("resolves")
    }

    /// Every assembled combo is a rule the engine itself accepts.
    fn promotable(p: &RuleParams) {
        RuleParams::parse(&p.to_value()).expect("the assembled rule passes the engine gate");
    }

    #[test]
    fn combo_count_is_the_product_and_columns_dedup() {
        let m = model(vec![
            axis(AxisSide::Entry, "m_state.age_sec", None, ">", vec![5.0, 10.0, 15.0]),
            axis(AxisSide::Entry, "m_flow.net_sol", Some("10s"), ">", vec![0.0, 2.5]),
            tp(vec![50.0, 100.0, 200.0]),
        ]);
        assert_eq!(m.combo_count(), 3 * 2 * 3);
        assert_eq!(m.columns().len(), 2);
    }

    #[test]
    fn an_entry_axis_is_a_filter_and_an_exit_axis_a_sell_line() {
        let m = model(vec![
            axis(AxisSide::Entry, "m_state.age_sec", None, ">", vec![5.0, 10.0]),
            axis(AxisSide::Exit, "m_position.retrace_pct", None, ">=", vec![20.0]),
            tp(vec![100.0]),
        ]);
        let p0 = m.combo_params(0);
        assert_eq!(p0.take_profit, Some(100.0));
        assert_eq!(p0.enter.filters.len(), 1);
        let Cond::Metric { r, is, .. } = &p0.enter.filters[0] else { panic!("a metric filter") };
        assert_eq!(r.metric, Metric::AgeSec);
        assert_eq!(is[0][0].value, 5.0);
        assert_eq!(p0.always.len(), 1, "the exit axis sells on its own line");
        assert!(p0.always[0].sell.is_some_and(|s| s.pct.is_none()), "it sells everything");
        let Cond::Metric { is, .. } = &m.combo_params(1).enter.filters[0] else { panic!() };
        assert_eq!(is[0][0].value, 10.0, "entry is the high-order digit");
        promotable(&p0);
    }

    #[test]
    fn the_off_pick_leaves_the_condition_out() {
        let mut spec = axis(AxisSide::Entry, "m_state.age_sec", None, ">", vec![5.0]);
        spec.values.push(None);
        let m = model(vec![spec, tp(vec![100.0])]);
        assert_eq!(m.combo_count(), 2);
        let p0 = m.combo_params(0);
        assert!(p0.enter.filters.is_empty() && p0.enter_on_arm(), "off sorts first");
        assert_eq!(m.combo_params(1).enter.filters.len(), 1);
        promotable(&p0);
        promotable(&m.combo_params(1));
    }

    #[test]
    fn two_axes_on_one_read_join_into_one_condition() {
        // Feasible bounds AND into a range...
        let m = model(vec![
            axis(AxisSide::Exit, "m_state.liquidity_sol", None, ">", vec![0.0]),
            axis(AxisSide::Exit, "m_state.liquidity_sol", None, "<", vec![40.0]),
        ]);
        let p = m.combo_params(0);
        assert_eq!(p.always.len(), 1, "one read, one line");
        let Cond::Metric { is, .. } = &p.always[0].when[0] else { panic!() };
        assert_eq!((is.len(), is[0].len()), (1, 2), "a range is one AND arm");
        promotable(&p);
        // ...crossed bounds OR into an outside band.
        let m = model(vec![
            axis(AxisSide::Exit, "m_state.liquidity_sol", None, "<", vec![30.0]),
            axis(AxisSide::Exit, "m_state.liquidity_sol", None, ">", vec![70.0]),
        ]);
        let Cond::Metric { is, .. } = &m.combo_params(0).always[0].when[0] else { panic!() };
        assert_eq!(is.len(), 2, "an outside band is two OR arms");
    }

    #[test]
    fn one_metric_on_two_spans_is_two_reads() {
        let m = model(vec![
            axis(AxisSide::Exit, "m_flow.buy_sol", Some("30s"), "<", vec![1.0]),
            axis(AxisSide::Exit, "m_flow.buy_sol", Some("60s"), "<", vec![1.0]),
            axis(AxisSide::Entry, "m_flow.buy_sol", Some("30sl"), ">=", vec![1.0]),
        ]);
        assert_eq!(m.columns().len(), 3, "a size on two bases is two reads");
        let p = m.combo_params(0);
        assert_eq!(p.always.len(), 2);
        promotable(&p);
    }

    #[test]
    fn a_two_window_read_needs_its_slice() {
        let mut spec = axis(AxisSide::Entry, "m_flow.slice_trade_share_pct", Some("60s"), ">=", vec![40.0]);
        assert!(AxesModel::resolve(&AxesRequest { axes: vec![spec.clone()] }).is_err(), "no slice");
        spec.slice = Some("3s".to_string());
        let m = model(vec![spec]);
        promotable(&m.combo_params(0));
    }

    #[test]
    fn a_position_metric_is_exit_only_and_reads_no_column() {
        let entry = axis(AxisSide::Entry, "m_position.retrace_pct", None, ">=", vec![3.0]);
        let e = AxesModel::resolve(&AxesRequest { axes: vec![entry] }).unwrap_err();
        assert!(e.contains("exit side"), "{e}");
        let m = model(vec![axis(AxisSide::Exit, "m_position.retrace_pct", None, ">=", vec![1.5, 3.0])]);
        assert!(m.columns().is_empty());
        promotable(&m.combo_params(0));
    }

    #[test]
    fn bad_axes_are_refused() {
        let mut all_off = axis(AxisSide::Entry, "m_state.age_sec", None, ">", vec![]);
        all_off.values.push(None);
        assert!(AxesModel::resolve(&AxesRequest { axes: vec![all_off] }).unwrap_err().contains("besides `off`"));
        let mut tp_off = tp(vec![100.0]);
        tp_off.values.push(None);
        assert!(AxesModel::resolve(&AxesRequest { axes: vec![tp_off] }).unwrap_err().contains("cannot carry `off`"));
        assert!(AxesModel::resolve(&AxesRequest { axes: vec![axis(AxisSide::Entry, "m_bogus.x", None, ">", vec![1.0])] }).is_err());
        for bad in ["abc", "30x", "0p"] {
            let spec = axis(AxisSide::Entry, "m_flow.buy_sol", Some(bad), ">=", vec![5.0]);
            assert!(AxesModel::resolve(&AxesRequest { axes: vec![spec] }).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_price_window_sizes_the_grid_and_clocks_set_ceilings() {
        let m = model(vec![
            axis(AxisSide::Entry, "m_price.trail_pct", Some("30s"), ">=", vec![8.0, 15.0]),
            axis(AxisSide::Entry, "m_state.age_sec", None, "<=", vec![20.0, 60.0]),
        ]);
        assert_eq!(m.max_window_secs(), 30.0);
        assert!(m.metric_value_ceiling(Metric::AgeSec) >= 60.0);
        assert_eq!(m.metric_value_ceiling(Metric::StallSec), 0.0);
    }

    #[test]
    fn entry_axes_sort_first() {
        let m = model(vec![tp(vec![100.0, 200.0]), axis(AxisSide::Entry, "m_state.age_sec", None, ">", vec![5.0, 10.0])]);
        assert!(m.axes[0].is_entry());
    }
}
