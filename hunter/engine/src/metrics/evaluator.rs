//! Condition evaluator — the one place `{operator, value}` DNF exprs are judged
//! against a metric value. Pure; shared verbatim by live and replay (parity).
//!
//! Semantics (settled design):
//! * A metric's condition expr is **DNF**: OR of AND-arms. Within an arm,
//!   conditions AND (forms ranges). Across arms, `|` OR. Empty arms list is
//!   vacuously true (rule validation rejects empty at save, so it never ships).
//! * `=` / `!=` are **bucket-equality** with the metric's own registry-declared
//!   tolerance: equal ⇔ `|value − x| <= tol / 2`.
//! * A non-finite metric value (NaN/±inf) satisfies nothing — `eval` returns
//!   `false` so a poisoned value can never fire an entry or exit.
//!
//! Wire JSON: flat `[{op,value},…]` = one AND arm (legacy); nested
//! `[[{…}],[{…}]]` = multi-arm OR. Serialize keeps flat when there is one arm.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A comparison operator a rule can put on a metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Operator {
    #[serde(rename = ">")]
    Gt,
    #[serde(rename = ">=")]
    Gte,
    #[serde(rename = "<")]
    Lt,
    #[serde(rename = "<=")]
    Lte,
    #[serde(rename = "=")]
    Eq,
    #[serde(rename = "!=")]
    Ne,
}

impl Operator {
    /// The JSON token (`">"`, `"="`, …) — for error messages.
    pub fn symbol(self) -> &'static str {
        match self {
            Operator::Gt => ">",
            Operator::Gte => ">=",
            Operator::Lt => "<",
            Operator::Lte => "<=",
            Operator::Eq => "=",
            Operator::Ne => "!=",
        }
    }
}

/// One authored condition: `metric <operator> value`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Condition {
    pub operator: Operator,
    pub value: f64,
}

/// DNF condition expr: OR of AND-arms (`Vec<Vec<Condition>>`).
pub type ConditionExpr = Vec<Vec<Condition>>;

/// Judge one condition against a metric value. `tol` is the metric's
/// registry-declared `=`-tolerance (used by `Eq`/`Ne` only). Non-finite
/// `value` never satisfies (incl. `Ne`).
pub fn eval_one(cond: Condition, value: f64, tol: f64) -> bool {
    if !value.is_finite() {
        return false;
    }
    match cond.operator {
        Operator::Gt => value > cond.value,
        Operator::Gte => value >= cond.value,
        Operator::Lt => value < cond.value,
        Operator::Lte => value <= cond.value,
        Operator::Eq => (value - cond.value).abs() <= tol / 2.0,
        Operator::Ne => (value - cond.value).abs() > tol / 2.0,
    }
}

/// Judge a metric's DNF condition expr. Empty arms ⇒ `true` (vacuous;
/// validation rejects empty at rule save). Any arm whose conditions all hold
/// satisfies the expr.
pub fn eval(arms: &[Vec<Condition>], value: f64, tol: f64) -> bool {
    if arms.is_empty() {
        return true;
    }
    arms.iter().any(|arm| arm.iter().all(|&c| eval_one(c, value, tol)))
}

/// Parse wire JSON into DNF arms. Accepts legacy flat `[{op,value},…]` (one arm)
/// or nested `[[{…}], …]` (multi-arm OR).
pub fn parse_condition_expr(val: &Value) -> Result<ConditionExpr, String> {
    let arr = val
        .as_array()
        .ok_or_else(|| "condition expr must be a JSON array".to_string())?;
    if arr.is_empty() {
        return Ok(Vec::new());
    }
    // Nested vs flat: first element is an array ⇒ arms; else ⇒ one flat arm.
    if arr[0].is_array() {
        let mut arms = Vec::with_capacity(arr.len());
        for (i, arm_val) in arr.iter().enumerate() {
            let arm_arr = arm_val.as_array().ok_or_else(|| {
                format!("condition arm {i}: expected array of {{operator, value}}")
            })?;
            if arm_arr.is_empty() {
                return Err(format!("condition arm {i}: empty arm"));
            }
            let mut arm = Vec::with_capacity(arm_arr.len());
            for c in arm_arr {
                arm.push(serde_json::from_value(c.clone()).map_err(|e| {
                    format!("invalid condition (expect {{\"operator\": \">\", \"value\": 10}}): {e}")
                })?);
            }
            arms.push(arm);
        }
        Ok(arms)
    } else {
        let mut arm = Vec::with_capacity(arr.len());
        for c in arr {
            arm.push(serde_json::from_value(c.clone()).map_err(|e| {
                format!("invalid condition (expect {{\"operator\": \">\", \"value\": 10}}): {e}")
            })?);
        }
        Ok(vec![arm])
    }
}

/// Serialize DNF → wire JSON. One arm → flat legacy list; multi-arm → nested.
pub fn condition_expr_to_value(arms: &[Vec<Condition>]) -> Value {
    if arms.len() == 1 {
        serde_json::to_value(&arms[0]).expect("Condition serializes")
    } else {
        serde_json::to_value(arms).expect("ConditionExpr serializes")
    }
}

/// Reject an AND arm no value can satisfy (e.g. `> 30` AND `< 10`).
/// Returns `Ok(())` when feasible. Public so sweep assemble can coalesce.
pub fn check_arm_satisfiable(conds: &[Condition], tol: f64) -> Result<(), String> {
    let half = tol / 2.0;
    let (mut lo, mut lo_strict) = (f64::NEG_INFINITY, false);
    let (mut hi, mut hi_strict) = (f64::INFINITY, false);
    let mut ne_bands: Vec<(f64, f64)> = Vec::new();
    for c in conds {
        match c.operator {
            Operator::Gt => raise_lo(&mut lo, &mut lo_strict, c.value, true),
            Operator::Gte => raise_lo(&mut lo, &mut lo_strict, c.value, false),
            Operator::Lt => drop_hi(&mut hi, &mut hi_strict, c.value, true),
            Operator::Lte => drop_hi(&mut hi, &mut hi_strict, c.value, false),
            Operator::Eq => {
                raise_lo(&mut lo, &mut lo_strict, c.value - half, false);
                drop_hi(&mut hi, &mut hi_strict, c.value + half, false);
            }
            Operator::Ne => ne_bands.push((c.value - half, c.value + half)),
        }
    }
    if lo > hi || (lo == hi && (lo_strict || hi_strict)) {
        return Err(format!("bounds cross: feasible range is empty around {lo}"));
    }
    for (b_lo, b_hi) in ne_bands {
        if lo.is_finite() && hi.is_finite() && b_lo <= lo && hi <= b_hi {
            return Err(format!("'!=' band [{b_lo}, {b_hi}] covers the feasible range"));
        }
    }
    Ok(())
}

/// An expr is unsatisfiable only when **every** OR arm is.
pub fn check_expr_satisfiable(arms: &[Vec<Condition>], tol: f64) -> Result<(), String> {
    if arms.is_empty() {
        return Ok(());
    }
    let mut last_err = None;
    for arm in arms {
        match check_arm_satisfiable(arm, tol) {
            Ok(()) => return Ok(()),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| "all OR arms unsatisfiable".into()))
}

/// Coalesce a flat list of axis contributions into DNF: AND when the combined
/// list is satisfiable (range case), else one OR arm per contribution.
pub fn coalesce_contributions(conds: Vec<Condition>, tol: f64) -> ConditionExpr {
    if conds.is_empty() {
        return Vec::new();
    }
    if check_arm_satisfiable(&conds, tol).is_ok() {
        vec![conds]
    } else {
        conds.into_iter().map(|c| vec![c]).collect()
    }
}

/// Normalize a metric's condition expr for same-field multi-operator input.
///
/// Across *different* metrics the side combinator stays entry-AND / exit-OR.
/// Within one metric, a single AND arm that is unsatisfiable (e.g. `< 30, >= 70`)
/// is rewritten to OR arms; a feasible AND arm (range) is kept. Explicit
/// multi-arm `|` exprs are left untouched so author intent is preserved.
pub fn normalize_condition_expr(arms: ConditionExpr, tol: f64) -> ConditionExpr {
    if arms.len() == 1 {
        let arm = arms.into_iter().next().unwrap();
        coalesce_contributions(arm, tol)
    } else {
        arms
    }
}

fn raise_lo(lo: &mut f64, lo_strict: &mut bool, v: f64, strict: bool) {
    if v > *lo || (v == *lo && strict) {
        *lo = v;
        *lo_strict = strict;
    }
}

fn drop_hi(hi: &mut f64, hi_strict: &mut bool, v: f64, strict: bool) {
    if v < *hi || (v == *hi && strict) {
        *hi = v;
        *hi_strict = strict;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(operator: Operator, value: f64) -> Condition {
        Condition { operator, value }
    }

    #[test]
    fn ordering_operators() {
        assert!(eval_one(c(Operator::Gt, 10.0), 10.1, 0.5));
        assert!(!eval_one(c(Operator::Gt, 10.0), 10.0, 0.5));
        assert!(eval_one(c(Operator::Gte, 10.0), 10.0, 0.5));
        assert!(eval_one(c(Operator::Lt, 10.0), 9.9, 0.5));
        assert!(!eval_one(c(Operator::Lt, 10.0), 10.0, 0.5));
        assert!(eval_one(c(Operator::Lte, 10.0), 10.0, 0.5));
    }

    #[test]
    fn eq_is_bucket_equality_with_half_tolerance() {
        let tol = 0.5;
        assert!(eval_one(c(Operator::Eq, 20.0), 20.0, tol));
        assert!(eval_one(c(Operator::Eq, 20.0), 20.25, tol));
        assert!(eval_one(c(Operator::Eq, 20.0), 19.75, tol));
        assert!(!eval_one(c(Operator::Eq, 20.0), 20.2500001, tol));
        assert!(!eval_one(c(Operator::Eq, 20.0), 19.7499999, tol));
    }

    #[test]
    fn ne_is_the_exact_complement_of_eq() {
        let tol = 1.0;
        for v in [19.0, 19.4999, 19.5, 20.0, 20.5, 20.5001, 21.0] {
            assert_ne!(
                eval_one(c(Operator::Eq, 20.0), v, tol),
                eval_one(c(Operator::Ne, 20.0), v, tol),
                "v={v}"
            );
        }
    }

    #[test]
    fn non_finite_value_satisfies_nothing() {
        for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            for op in [
                Operator::Gt,
                Operator::Gte,
                Operator::Lt,
                Operator::Lte,
                Operator::Eq,
                Operator::Ne,
            ] {
                assert!(!eval_one(c(op, 10.0), v, 0.5), "{op:?} vs {v}");
            }
        }
    }

    #[test]
    fn and_arm_ands_together() {
        let window = vec![vec![c(Operator::Gt, 10.0), c(Operator::Lt, 30.0)]];
        assert!(eval(&window, 20.0, 0.5));
        assert!(!eval(&window, 5.0, 0.5));
        assert!(!eval(&window, 30.0, 0.5));
        assert!(eval(&[], 123.0, 0.5));
    }

    #[test]
    fn or_arms_any_match() {
        let outside = vec![
            vec![c(Operator::Lt, 30.0)],
            vec![c(Operator::Gte, 70.0)],
        ];
        assert!(eval(&outside, 20.0, 0.1));
        assert!(!eval(&outside, 50.0, 0.1));
        assert!(eval(&outside, 70.0, 0.1));
        assert!(check_expr_satisfiable(&outside, 0.1).is_ok());
        assert!(check_arm_satisfiable(
            &[c(Operator::Lt, 30.0), c(Operator::Gte, 70.0)],
            0.1
        )
        .is_err());
    }

    #[test]
    fn coalesce_ands_when_feasible_else_ors() {
        let range = coalesce_contributions(
            vec![c(Operator::Gt, 10.0), c(Operator::Lt, 50.0)],
            0.1,
        );
        assert_eq!(range.len(), 1);
        assert_eq!(range[0].len(), 2);

        let outside = coalesce_contributions(
            vec![c(Operator::Lt, 30.0), c(Operator::Gte, 70.0)],
            0.1,
        );
        assert_eq!(outside.len(), 2);
        assert_eq!(outside[0], vec![c(Operator::Lt, 30.0)]);
        assert_eq!(outside[1], vec![c(Operator::Gte, 70.0)]);
    }

    #[test]
    fn normalize_rewrites_single_unsat_arm_to_or_keeps_explicit_or() {
        let rewritten = normalize_condition_expr(
            vec![vec![c(Operator::Lt, 30.0), c(Operator::Gte, 70.0)]],
            0.1,
        );
        assert_eq!(rewritten.len(), 2);

        let explicit = vec![
            vec![c(Operator::Lt, 30.0)],
            vec![c(Operator::Gte, 70.0)],
        ];
        assert_eq!(normalize_condition_expr(explicit.clone(), 0.1), explicit);
    }

    #[test]
    fn wire_flat_and_nested_round_trip() {
        let flat = serde_json::json!([
            {"operator": ">", "value": 10},
            {"operator": "<", "value": 30}
        ]);
        let arms = parse_condition_expr(&flat).unwrap();
        assert_eq!(arms.len(), 1);
        assert_eq!(arms[0].len(), 2);
        let back = condition_expr_to_value(&arms);
        assert!(back.as_array().unwrap()[0].is_object());

        let nested = serde_json::json!([
            [{"operator": "<", "value": 30}],
            [{"operator": ">=", "value": 70}]
        ]);
        let arms2 = parse_condition_expr(&nested).unwrap();
        assert_eq!(arms2.len(), 2);
        let back2 = condition_expr_to_value(&arms2);
        assert!(back2.as_array().unwrap()[0].is_array());
    }

    #[test]
    fn operator_serde_round_trips_symbols() {
        for (op, sym) in [
            (Operator::Gt, "\">\""),
            (Operator::Gte, "\">=\""),
            (Operator::Lt, "\"<\""),
            (Operator::Lte, "\"<=\""),
            (Operator::Eq, "\"=\""),
            (Operator::Ne, "\"!=\""),
        ] {
            assert_eq!(serde_json::to_string(&op).unwrap(), sym);
            let back: Operator = serde_json::from_str(sym).unwrap();
            assert_eq!(back, op);
        }
        assert!(serde_json::from_str::<Operator>("\"=>\"").is_err());
    }
}
