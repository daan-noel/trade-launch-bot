//! Condition evaluator — the one place `{operator, value}` lists are judged
//! against a metric value. Pure; shared verbatim by live and replay (parity).
//!
//! Semantics (settled design):
//! * All conditions on a metric AND together (an empty list is vacuously true —
//!   rule validation rejects empty lists at save, so it never ships).
//! * `=` / `!=` are **bucket-equality** with the metric's own registry-declared
//!   tolerance: equal ⇔ `|value − x| <= tol / 2`.
//! * A non-finite metric value (NaN/±inf) satisfies nothing — `eval` returns
//!   `false` so a poisoned value can never fire an entry or exit.

use serde::{Deserialize, Serialize};

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

/// Judge a metric's full condition list (AND). Empty ⇒ `true` (vacuous;
/// validation rejects empty lists at rule save).
pub fn eval(conditions: &[Condition], value: f64, tol: f64) -> bool {
    conditions.iter().all(|&c| eval_one(c, value, tol))
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
        // |v - 20| <= 0.25 ⇔ equal.
        assert!(eval_one(c(Operator::Eq, 20.0), 20.0, tol));
        assert!(eval_one(c(Operator::Eq, 20.0), 20.25, tol)); // on the edge: equal
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
    fn list_ands_together() {
        let window = [c(Operator::Gt, 10.0), c(Operator::Lt, 30.0)];
        assert!(eval(&window, 20.0, 0.5));
        assert!(!eval(&window, 5.0, 0.5));
        assert!(!eval(&window, 30.0, 0.5));
        // Empty list is vacuously true.
        assert!(eval(&[], 123.0, 0.5));
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
