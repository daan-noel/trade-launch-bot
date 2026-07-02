//! In-memory server-side query over a finished backtest's per-token results.
//!
//! The Simulated token table pages/sorts/filters/searches over the **unified**
//! `TableRequest` contract — same shape as Positions/Matched — but its data source
//! is the already-resident `Vec<Value>` in [`SimResults`](crate::state::sim_results)
//! (lab is single-user, workstation RAM), so there's no DB to query: we apply the
//! request in Rust. Numeric operators (`gt`, `between`, …) compare **numerically**,
//! matching the SQL path's semantics (see `strategy_repo::push_filter_predicate`).
//!
//! Only whitelisted keys are honored (unknown → ignored), and each filter/sort key
//! resolves to a JSON field + type, so a numeric op on a text field is dropped just
//! like the SQL whitelist drops it.

use serde_json::Value;

use trading_core::api::table_query::{FilterOp, FilterSpec, TableRequest};

/// Column type for a sim result field — decides which ops are legal and how a
/// value compares (string `contains`/`eq` vs numeric compare), mirroring the SQL
/// `FilterKind`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Text,
    Number,
}

/// Resolve a frontend column key to the JSON field it reads + its type. `None` =
/// not filterable/sortable (dropped). Mirrors the frontend `simColumns` keys.
fn resolve(key: &str) -> Option<(&'static str, Kind)> {
    use Kind::{Number, Text};
    Some(match key {
        "mint" => ("mint", Text),
        "symbol" => ("symbol", Text),
        "exit_reason" => ("exit_reason", Text),
        "entry_tx" => ("entry_tx", Text),
        "exit_tx" => ("exit_tx", Text),
        "target_price" => ("target_price", Number),
        "entry_price" => ("entry_price", Number),
        "ath_price" => ("ath_price", Number),
        "exit_price" => ("exit_price", Number),
        "entry_token_amount" => ("entry_token_amount", Number),
        "holding_secs" => ("holding_secs", Number),
        "pnl_percent" => ("pnl_percent", Number),
        "pnl_sol" => ("pnl_sol", Number),
        "total_trades" => ("total_trades", Number),
        // Time fields sort/filter lexicographically on the RFC3339 string, which is
        // chronological — treat as text.
        "entry_time" => ("entry_time", Text),
        "exit_time" => ("exit_time", Text),
        "target_time" => ("target_time", Text),
        _ => return None,
    })
}

/// Read a JSON field as `f64` (number or numeric string); `None` if absent/null.
fn field_num(row: &Value, field: &str) -> Option<f64> {
    match row.get(field) {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.parse().ok(),
        _ => None,
    }
}

/// Read a JSON field as a lowercased string (number stringified); `None` if
/// absent/null.
fn field_text(row: &Value, field: &str) -> Option<String> {
    match row.get(field) {
        Some(Value::String(s)) => Some(s.to_lowercase()),
        Some(Value::Number(n)) => Some(n.to_string()),
        _ => None,
    }
}

/// Coerce a filter operand `Value` to `f64` (number or numeric string).
fn operand_num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Coerce a filter operand `Value` to a lowercased string.
fn operand_text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.trim().to_lowercase()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Does `row` satisfy one structured filter? A shape/type mismatch (numeric op on
/// a text field, non-numeric operand, missing bound) is treated as **not a
/// constraint** → keeps the row, mirroring the SQL side dropping the predicate.
fn row_matches(row: &Value, field: &str, kind: Kind, spec: &FilterSpec) -> bool {
    match (kind, spec.op) {
        (Kind::Text, FilterOp::Contains) => match operand_text(&spec.val) {
            Some(needle) if !needle.is_empty() => {
                field_text(row, field).is_some_and(|hay| hay.contains(&needle))
            }
            _ => true,
        },
        (Kind::Text, FilterOp::Eq) => match operand_text(&spec.val) {
            Some(needle) if !needle.is_empty() => {
                field_text(row, field).is_some_and(|hay| hay == needle)
            }
            _ => true,
        },
        // Numeric op on a text field → not a constraint.
        (Kind::Text, _) => true,

        (Kind::Number, FilterOp::Between) => {
            match (operand_num(&spec.min), operand_num(&spec.max)) {
                (Some(min), Some(max)) => {
                    field_num(row, field).is_some_and(|v| v >= min && v <= max)
                }
                _ => true,
            }
        }
        (Kind::Number, op) => match operand_num(&spec.val) {
            Some(target) => match field_num(row, field) {
                Some(v) => match op {
                    FilterOp::Gt => v > target,
                    FilterOp::Gte => v >= target,
                    FilterOp::Lt => v < target,
                    FilterOp::Lte => v <= target,
                    // `eq`/`contains` on a numeric field → equality.
                    _ => v == target,
                },
                // Field absent/null can't satisfy a numeric predicate.
                None => false,
            },
            None => true,
        },
    }
}

/// Does `row` match the free-text search over mint / symbol? Blank search matches
/// everything.
fn matches_search(row: &Value, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    ["mint", "symbol"]
        .iter()
        .filter_map(|f| field_text(row, f))
        .any(|hay| hay.contains(needle))
}

/// One page of a finished sim's rows after applying the request's search +
/// filters + sort, plus the total match count (before paging) for `X-Total-Count`.
/// The returned rows are cloned refs into the shared `Arc` payload.
pub fn query(rows: &[Value], req: &TableRequest) -> (Vec<Value>, usize) {
    let needle = req.search.trim().to_lowercase();

    // Resolve filters once (key → field/kind), dropping unknown keys.
    let filters: Vec<(&'static str, Kind, &FilterSpec)> = req
        .filters
        .iter()
        .filter_map(|(key, spec)| resolve(key).map(|(field, kind)| (field, kind, spec)))
        .collect();

    let mut matched: Vec<&Value> = rows
        .iter()
        .filter(|row| matches_search(row, &needle))
        .filter(|row| filters.iter().all(|(field, kind, spec)| row_matches(row, field, *kind, spec)))
        .collect();

    let total = matched.len();

    // Apply the first resolved sort key (the table sorts by one column at a time;
    // extra keys, if ever sent, are ignored). NULLS-last regardless of direction.
    if let Some((field, kind)) = req.sorting.first().and_then(|s| resolve(&s.col).map(|r| (r.0, r.1))) {
        let desc = req.sorting[0].dir.is_desc();
        match kind {
            Kind::Number => matched.sort_by(|a, b| {
                let (na, nb) = (field_num(a, field), field_num(b, field));
                cmp_opt(na, nb, desc)
            }),
            Kind::Text => matched.sort_by(|a, b| {
                let (ta, tb) = (field_text(a, field), field_text(b, field));
                cmp_opt(ta, tb, desc)
            }),
        }
    }

    // Page.
    let (limit, offset) = req.pagination.bounds();
    let page: Vec<Value> = matched
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize)
        .cloned()
        .collect();
    (page, total)
}

/// Ordering for `Option<T>` values with NULLS-last semantics under both
/// directions: a present value always sorts before an absent one.
fn cmp_opt<T: PartialOrd>(a: Option<T>, b: Option<T>, desc: bool) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (Some(a), Some(b)) => {
            let ord = a.partial_cmp(&b).unwrap_or(Ordering::Equal);
            if desc { ord.reverse() } else { ord }
        }
        (Some(_), None) => Ordering::Less,  // present before absent (NULLS last)
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rows() -> Vec<Value> {
        vec![
            json!({"mint":"a","symbol":"BONK","pnl_percent":10.0,"pnl_sol":1.0,"exit_time":"2026-01-01T00:00:00Z"}),
            json!({"mint":"b","symbol":"pumpcat","pnl_percent":-5.0,"pnl_sol":-0.5,"exit_time":"2026-01-01T00:00:00Z"}),
            json!({"mint":"c","symbol":"WIF","pnl_percent":50.0,"pnl_sol":2.0,"exit_time":null}),
        ]
    }

    fn req(json: serde_json::Value) -> TableRequest {
        serde_json::from_value(json).expect("TableRequest")
    }

    #[test]
    fn numeric_gt_filters_server_side() {
        let r = req(json!({"filters": {"pnl_percent": {"op":"gt","val":5}}}));
        let (page, total) = query(&rows(), &r);
        assert_eq!(total, 2, "pnl_percent > 5 keeps BONK(10) + WIF(50)");
        assert_eq!(page.len(), 2);
    }

    #[test]
    fn between_is_inclusive() {
        let r = req(json!({"filters": {"pnl_percent": {"op":"between","min":10,"max":50}}}));
        let (_, total) = query(&rows(), &r);
        assert_eq!(total, 2, "10..=50 keeps 10 and 50");
    }

    #[test]
    fn numeric_op_on_text_col_is_ignored() {
        // `symbol` is Text; a numeric `gt` must not constrain (mirrors SQL drop).
        let r = req(json!({"filters": {"symbol": {"op":"gt","val":5}}}));
        let (_, total) = query(&rows(), &r);
        assert_eq!(total, 3, "numeric op on text col keeps all rows");
    }

    #[test]
    fn search_matches_symbol_substring() {
        let r = req(json!({"search": "pump"}));
        let (_, total) = query(&rows(), &r);
        assert_eq!(total, 1, "search 'pump' matches only pumpcat");
    }

    #[test]
    fn sort_desc_and_page() {
        let r = req(json!({
            "sorting": [{"col":"pnl_percent","dir":"desc"}],
            "pagination": {"page":1,"pageSize":2}
        }));
        let (page, total) = query(&rows(), &r);
        assert_eq!(total, 3);
        assert_eq!(page.len(), 2, "page size 2");
        assert_eq!(page[0]["mint"], "c", "WIF(50) first desc");
        assert_eq!(page[1]["mint"], "a", "BONK(10) second");
    }
}
