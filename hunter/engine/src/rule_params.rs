//! `RuleParams` — the typed form of `strategy_rules.params` (JSONB), the "WHEN
//! it trades" half of a generic rule. Canonical JSON shape (design SSOT:
//! `hunter/docs/roadmap/fingerprint-metrics-engine-plan.md` §5):
//!
//! ```json
//! {
//!   "take_profit": 100,
//!   "stop_loss": 30,
//!   "entry": {
//!     "m_snapshot":   { "time": [{"operator": ">", "value": 10}], ... },
//!     "m_time_window": { "window_size_sec": 10, "gross_flow": [ ... ] }
//!   },
//!   "exit": { ... }
//! }
//! ```
//!
//! Group objects mix **strict params** (e.g. `window_size_sec`) with **metric
//! condition lists** at the same level, so parsing is a registry-guided walk
//! rather than a plain derive: every group / strict-param / metric / operator
//! name is checked against [`crate::metrics::REGISTRY`], so a typo fails the
//! save instead of silently never matching.
//!
//! Absent = unconstrained: `entry: None` ⇒ enter on arm (fingerprint alone);
//! `exit: None` ⇒ TP/SL/death only; absent TP/SL ⇒ that guard is off.
//!
//! Parse **once at rule load** into these structs — never per event.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::metrics::evaluator::{
    check_expr_satisfiable, condition_expr_to_value, normalize_condition_expr,
    parse_condition_expr, ConditionExpr,
};
use crate::metrics::{group_by_name, group_spec, metric_spec, MetricGroupId, MetricId, REGISTRY};

/// Typed, registry-checked `params`. See module docs for the JSON shape.
#[derive(Debug, Clone, PartialEq)]
pub struct RuleParams {
    /// Take-profit as % of entry price (e.g. `100` = +100%). `None` = off.
    pub take_profit: Option<f64>,
    /// Stop-loss as % drop from entry (e.g. `30` = −30%). `None` = off.
    pub stop_loss: Option<f64>,
    /// Entry conditions. `None` = enter on arm (the fingerprint alone decides).
    pub entry: Option<SideConditions>,
    /// Exit conditions. `None` = TP/SL/death only.
    pub exit: Option<SideConditions>,
}

/// One side's (entry or exit) metric-condition groups.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SideConditions(pub BTreeMap<MetricGroupId, GroupConditions>);

/// One group's authored content: strict params beside per-metric condition
/// exprs (DNF: OR of AND-arms within a metric; across metrics/groups the side
/// combinator is entry-AND / exit-OR — see `arm.rs`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GroupConditions {
    /// Strict (non-condition) params, e.g. `window_size_sec` — names validated
    /// against the group's registry entry.
    pub strict: BTreeMap<String, f64>,
    /// DNF condition arms per metric of this group.
    pub metrics: BTreeMap<MetricId, ConditionExpr>,
}

impl GroupConditions {
    /// A strict param's value by JSON name (`None` = not set).
    pub fn strict_param(&self, name: &str) -> Option<f64> {
        self.strict.get(name).copied()
    }
}

impl SideConditions {
    /// Whether this side constrains anything (an empty side ≡ absent).
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl RuleParams {
    /// `entry` absent/empty ⇒ arming the (token, rule) IS the entry signal.
    pub fn enter_on_arm(&self) -> bool {
        self.entry.as_ref().is_none_or(SideConditions::is_empty)
    }

    /// `exit` absent/empty ⇒ only TP / SL / death close the position.
    pub fn exit_is_tp_sl_only(&self) -> bool {
        self.exit.as_ref().is_none_or(SideConditions::is_empty)
    }

    /// Parse **and validate** stored/authored params JSON. This is the one
    /// entry point rule save and rule load share — everything it returns is
    /// registry-checked and semantically sane.
    pub fn parse(json: &Value) -> Result<Self, String> {
        let parsed = Self::parse_shape(json)?;
        parsed.validate()?;
        Ok(parsed)
    }

    /// Serialize back to the canonical JSONB shape (inverse of [`Self::parse`]).
    pub fn to_value(&self) -> Value {
        let mut root = Map::new();
        if let Some(tp) = self.take_profit {
            root.insert("take_profit".into(), tp.into());
        }
        if let Some(sl) = self.stop_loss {
            root.insert("stop_loss".into(), sl.into());
        }
        if let Some(side) = &self.entry {
            root.insert("entry".into(), side_to_value(side));
        }
        if let Some(side) = &self.exit {
            root.insert("exit".into(), side_to_value(side));
        }
        Value::Object(root)
    }

    // ── Shape walk (names + JSON types) ────────────────────────────────────

    fn parse_shape(json: &Value) -> Result<Self, String> {
        let obj = json.as_object().ok_or("params must be a JSON object")?;
        for key in obj.keys() {
            if !matches!(key.as_str(), "take_profit" | "stop_loss" | "entry" | "exit") {
                return Err(format!("unknown params key '{key}'"));
            }
        }
        Ok(RuleParams {
            take_profit: parse_opt_number(obj.get("take_profit"), "take_profit")?,
            stop_loss: parse_opt_number(obj.get("stop_loss"), "stop_loss")?,
            entry: parse_opt_side(obj.get("entry"), "entry")?,
            exit: parse_opt_side(obj.get("exit"), "exit")?,
        })
    }

    // ── Semantic validation (values + satisfiability) ──────────────────────

    fn validate(&self) -> Result<(), String> {
        for (name, v) in [("take_profit", self.take_profit), ("stop_loss", self.stop_loss)] {
            if let Some(v) = v {
                if !v.is_finite() || v <= 0.0 {
                    return Err(format!("{name} must be a finite number > 0"));
                }
            }
        }
        for (side_name, side) in
            [("entry", self.entry.as_ref()), ("exit", self.exit.as_ref())]
        {
            let Some(side) = side else { continue };
            for (group_id, group) in &side.0 {
                validate_group(side_name, *group_id, group)?;
            }
        }
        Ok(())
    }
}

fn parse_opt_number(v: Option<&Value>, name: &str) -> Result<Option<f64>, String> {
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => {
            n.as_f64().map(Some).ok_or_else(|| format!("{name} is not a valid number"))
        }
        Some(_) => Err(format!("{name} must be a number")),
    }
}

fn parse_opt_side(v: Option<&Value>, side_name: &str) -> Result<Option<SideConditions>, String> {
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Object(groups)) => {
            let mut side = SideConditions::default();
            for (group_name, group_val) in groups {
                let spec = group_by_name(group_name).ok_or_else(|| {
                    format!("{side_name}: unknown metric group '{group_name}'")
                })?;
                let group_obj = group_val.as_object().ok_or_else(|| {
                    format!("{side_name}.{group_name} must be an object")
                })?;
                let mut group = GroupConditions::default();
                for (key, val) in group_obj {
                    if spec.strict_param_by_name(key).is_some() {
                        let n = val.as_f64().ok_or_else(|| {
                            format!("{side_name}.{group_name}.{key} must be a number")
                        })?;
                        group.strict.insert(key.clone(), n);
                    } else if let Some(m) = spec.metric_by_name(key) {
                        let arms = parse_condition_expr(val).map_err(|e| {
                            format!(
                                "{side_name}.{group_name}.{key}: invalid condition list \
                                 (expect [{{\"operator\": \">\", \"value\": 10}}, ...] \
                                 or nested OR arms): {e}"
                            )
                        })?;
                        // Same metric, different ops: AND if feasible, else OR.
                        group.metrics.insert(
                            m.id,
                            normalize_condition_expr(arms, m.eq_tolerance),
                        );
                    } else {
                        // Better message when the metric exists in another group.
                        let owner = REGISTRY
                            .iter()
                            .find(|g| g.metric_by_name(key).is_some())
                            .map(|g| g.name);
                        return Err(match owner {
                            Some(owner) => format!(
                                "{side_name}.{group_name}: metric '{key}' belongs to group '{owner}'"
                            ),
                            None => format!(
                                "{side_name}.{group_name}: unknown metric or param '{key}'"
                            ),
                        });
                    }
                }
                side.0.insert(spec.id, group);
            }
            Ok(Some(side))
        }
        Some(_) => Err(format!("{side_name} must be an object")),
    }
}

fn validate_group(
    side_name: &str,
    group_id: MetricGroupId,
    group: &GroupConditions,
) -> Result<(), String> {
    let spec = group_spec(group_id);
    // Required strict params present; all strict values finite and > 0.
    for p in spec.strict_params {
        if p.required && !group.strict.contains_key(p.name) {
            return Err(format!(
                "{side_name}.{}: missing required param '{}'",
                spec.name, p.name
            ));
        }
    }
    for (name, v) in &group.strict {
        if !v.is_finite() || *v <= 0.0 {
            return Err(format!(
                "{side_name}.{}.{name} must be a finite number > 0",
                spec.name
            ));
        }
    }
    // A group with no metric conditions constrains nothing — reject the no-op.
    if group.metrics.is_empty() {
        return Err(format!(
            "{side_name}.{}: group has no metric conditions",
            spec.name
        ));
    }
    for (metric_id, arms) in &group.metrics {
        let m = metric_spec(*metric_id);
        if arms.is_empty() {
            return Err(format!(
                "{side_name}.{}.{}: empty condition list",
                spec.name, m.name
            ));
        }
        for (ai, arm) in arms.iter().enumerate() {
            if arm.is_empty() {
                return Err(format!(
                    "{side_name}.{}.{}: empty OR arm {ai}",
                    spec.name, m.name
                ));
            }
            for c in arm {
                if !c.value.is_finite() {
                    return Err(format!(
                        "{side_name}.{}.{}: condition value must be finite",
                        spec.name, m.name
                    ));
                }
            }
        }
        check_expr_satisfiable(arms, m.eq_tolerance).map_err(|why| {
            format!(
                "{side_name}.{}.{}: contradictory conditions ({why})",
                spec.name, m.name
            )
        })?;
    }
    Ok(())
}

fn side_to_value(side: &SideConditions) -> Value {
    let mut groups = Map::new();
    for (group_id, group) in &side.0 {
        let mut obj = Map::new();
        for (name, v) in &group.strict {
            obj.insert(name.clone(), (*v).into());
        }
        for (metric_id, arms) in &group.metrics {
            obj.insert(
                metric_spec(*metric_id).name.to_string(),
                condition_expr_to_value(arms),
            );
        }
        groups.insert(group_spec(*group_id).name.to_string(), Value::Object(obj));
    }
    Value::Object(groups)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::evaluator::{Condition, Operator};
    use serde_json::json;

    /// The canonical params example from the design docs
    /// (`Bot/docs/strategy-redesign-answer-1.md`, trailing commas removed).
    fn docs_example() -> Value {
        let side = json!({
            "m_snapshot": {
                "time": [
                    {"operator": ">", "value": 10},
                    {"operator": "<", "value": 30}
                ],
                "liquidity": [ {"operator": "=", "value": 20} ]
            },
            "m_price_path": {
                "stall": [ {"operator": "<", "value": 10} ],
                "trail": [ {"operator": "<", "value": 10} ]
            },
            "m_time_window": {
                "window_size_sec": 10,
                "gross_flow": [ {"operator": "=", "value": 15} ],
                "net_flow": [ {"operator": "=", "value": 5} ],
                "buy": [ {"operator": "=", "value": 10} ],
                "sell": [ {"operator": "=", "value": 5} ]
            }
        });
        json!({
            "take_profit": 100,
            "stop_loss": 30,
            "entry": side,
            "exit": side
        })
    }

    #[test]
    fn docs_example_round_trips() {
        let parsed = RuleParams::parse(&docs_example()).expect("docs example is valid");
        assert_eq!(parsed.take_profit, Some(100.0));
        assert_eq!(parsed.stop_loss, Some(30.0));
        let entry = parsed.entry.as_ref().unwrap();
        assert_eq!(entry.0.len(), 3);
        let tw = &entry.0[&MetricGroupId::TimeWindow];
        assert_eq!(tw.strict_param("window_size_sec"), Some(10.0));
        assert_eq!(
            tw.metrics[&MetricId::GrossFlow],
            vec![vec![Condition { operator: Operator::Eq, value: 15.0 }]]
        );
        // Full round-trip: to_value → parse → identical struct.
        let reparsed = RuleParams::parse(&parsed.to_value()).unwrap();
        assert_eq!(reparsed, parsed);
    }

    #[test]
    fn absent_sides_mean_unconstrained() {
        let p = RuleParams::parse(&json!({"take_profit": 50})).unwrap();
        assert!(p.enter_on_arm(), "entry: None ⇒ arm is enter");
        assert!(p.exit_is_tp_sl_only(), "exit: None ⇒ TP/SL only");
        assert_eq!(p.stop_loss, None);

        // Empty side objects behave like absent sides.
        let p = RuleParams::parse(&json!({"entry": {}, "exit": {}})).unwrap();
        assert!(p.enter_on_arm());
        assert!(p.exit_is_tp_sl_only());
        // And a fully-empty params object is legal: fingerprint-only rule.
        assert!(RuleParams::parse(&json!({})).is_ok());
    }

    #[test]
    fn unknown_names_rejected() {
        // Unknown top-level key.
        let e = RuleParams::parse(&json!({"take_profits": 1})).unwrap_err();
        assert!(e.contains("unknown params key"), "{e}");
        // Unknown group.
        let e = RuleParams::parse(&json!({"entry": {"m_snapshots": {}}})).unwrap_err();
        assert!(e.contains("unknown metric group"), "{e}");
        // Unknown metric within a known group.
        let e = RuleParams::parse(
            &json!({"entry": {"m_snapshot": {"tyme": [{"operator": ">", "value": 1}]}}}),
        )
        .unwrap_err();
        assert!(e.contains("unknown metric or param 'tyme'"), "{e}");
        // Unknown operator.
        let e = RuleParams::parse(
            &json!({"entry": {"m_snapshot": {"time": [{"operator": "=>", "value": 1}]}}}),
        )
        .unwrap_err();
        assert!(e.contains("invalid condition list"), "{e}");
    }

    #[test]
    fn metric_under_wrong_group_names_the_right_one() {
        let e = RuleParams::parse(
            &json!({"exit": {"m_snapshot": {"stall": [{"operator": "<", "value": 10}]}}}),
        )
        .unwrap_err();
        assert!(e.contains("belongs to group 'm_price_path'"), "{e}");
    }

    #[test]
    fn time_window_requires_window_size() {
        let e = RuleParams::parse(
            &json!({"entry": {"m_time_window": {"buy": [{"operator": ">", "value": 1}]}}}),
        )
        .unwrap_err();
        assert!(e.contains("missing required param 'window_size_sec'"), "{e}");

        // And the strict value must be positive/finite.
        let e = RuleParams::parse(&json!({"entry": {"m_time_window": {
            "window_size_sec": 0,
            "buy": [{"operator": ">", "value": 1}]
        }}}))
        .unwrap_err();
        assert!(e.contains("window_size_sec must be a finite number > 0"), "{e}");
    }

    #[test]
    fn tp_sl_must_be_positive() {
        for bad in [json!({"take_profit": 0}), json!({"stop_loss": -5})] {
            let e = RuleParams::parse(&bad).unwrap_err();
            assert!(e.contains("> 0"), "{e}");
        }
    }

    #[test]
    fn same_metric_unsat_and_normalizes_to_or() {
        // Flat `< 30, >= 70` (legacy AND) → OR arms, promotable / savable.
        let p = RuleParams::parse(&json!({"exit": {"m_snapshot": {"liquidity": [
            {"operator": "<", "value": 30},
            {"operator": ">=", "value": 70}
        ]}}}))
        .unwrap();
        let arms = &p.exit.as_ref().unwrap().0[&MetricGroupId::Snapshot].metrics
            [&MetricId::Liquidity];
        assert_eq!(arms.len(), 2);

        // Crossed time bounds similarly become OR (not rejected).
        let p = RuleParams::parse(&json!({"entry": {"m_snapshot": {"time": [
            {"operator": ">", "value": 30},
            {"operator": "<", "value": 10}
        ]}}}))
        .unwrap();
        assert_eq!(
            p.entry.as_ref().unwrap().0[&MetricGroupId::Snapshot].metrics[&MetricId::Time].len(),
            2
        );
    }

    #[test]
    fn same_metric_feasible_and_stays_and() {
        // > 10 AND >= 10 AND < 30 is fine (redundant, not contradictory).
        assert!(RuleParams::parse(&json!({"entry": {"m_snapshot": {"time": [
            {"operator": ">", "value": 10},
            {"operator": ">=", "value": 10},
            {"operator": "<", "value": 30}
        ]}}}))
        .is_ok());

        // >= 10 AND <= 10 is the single point 10 — satisfiable AND.
        let p = RuleParams::parse(&json!({"entry": {"m_snapshot": {"time": [
            {"operator": ">=", "value": 10},
            {"operator": "<=", "value": 10}
        ]}}}))
        .unwrap();
        assert_eq!(
            p.entry.as_ref().unwrap().0[&MetricGroupId::Snapshot].metrics[&MetricId::Time].len(),
            1
        );

        // Explicit nested OR arms left as authored.
        assert!(RuleParams::parse(&json!({"exit": {"m_snapshot": {"liquidity": [
            [{"operator": "<", "value": 30}],
            [{"operator": ">=", "value": 70}]
        ]}}}))
        .is_ok());
    }

    #[test]
    fn multi_arm_all_unsat_still_rejected() {
        // Explicit `|` of two unsatisfiable AND arms — normalize does not expand
        // multi-arm exprs, so the whole metric stays contradictory.
        let e = RuleParams::parse(&json!({"entry": {"m_snapshot": {"time": [
            [{"operator": ">", "value": 30}, {"operator": "<", "value": 10}],
            [{"operator": ">", "value": 50}, {"operator": "<", "value": 20}]
        ]}}}))
        .unwrap_err();
        assert!(e.contains("contradictory"), "{e}");
    }

    #[test]
    fn empty_lists_and_no_op_groups_rejected() {
        let e = RuleParams::parse(&json!({"entry": {"m_snapshot": {"time": []}}}))
            .unwrap_err();
        assert!(e.contains("empty condition list"), "{e}");

        // A group carrying only its strict param constrains nothing.
        let e = RuleParams::parse(
            &json!({"entry": {"m_time_window": {"window_size_sec": 10}}}),
        )
        .unwrap_err();
        assert!(e.contains("no metric conditions"), "{e}");
    }

    #[test]
    fn non_finite_condition_value_rejected() {
        // JSON can't carry NaN/inf, so exercise validate() directly.
        let mut group = GroupConditions::default();
        group.metrics.insert(
            MetricId::Time,
            vec![vec![Condition {
                operator: Operator::Gt,
                value: f64::NAN,
            }]],
        );
        let mut side = SideConditions::default();
        side.0.insert(MetricGroupId::Snapshot, group);
        let p = RuleParams { take_profit: None, stop_loss: None, entry: Some(side), exit: None };
        let e = p.validate().unwrap_err();
        assert!(e.contains("must be finite"), "{e}");
    }
}
