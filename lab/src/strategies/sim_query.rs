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
/// not filterable/sortable (dropped). Mirrors the frontend `simColumns` +
/// `appendedTokenColumns` keys — several columns use a friendlier display key
/// than the underlying JSON field name, so those are aliased here too. The
/// `appendedTokenColumns` set (`creator`, `trade_count`, `initial_buy`, `cu_limit`,
/// `migrated`, ...) reads token metadata that `token_enrich::TokenEnrichment`
/// flattens onto the row — see that module for where it's populated.
fn resolve(key: &str) -> Option<(&'static str, Kind)> {
    use Kind::{Number, Text};
    Some(match key {
        "mint" => ("mint", Text),
        "symbol" => ("symbol", Text),
        "reason" | "exit_reason" => ("exit_reason", Text),
        "entry_tx" => ("entry_tx", Text),
        "exit_tx" => ("exit_tx", Text),
        "target_price" => ("target_price", Number),
        "entry_price" => ("entry_price", Number),
        "ath_price" => ("ath_price", Number),
        "exit_price" => ("exit_price", Number),
        "entry_token_amount" => ("entry_token_amount", Number),
        "holding" | "holding_secs" => ("holding_secs", Number),
        "pnl_pct" | "pnl_percent" => ("pnl_percent", Number),
        "pnl_sol" => ("pnl_sol", Number),
        // Time fields sort/filter lexicographically on the RFC3339 string, which is
        // chronological — treat as text.
        "entry_time" => ("entry_time", Text),
        "exit_time" => ("exit_time", Text),
        "target_time" => ("target_time", Text),

        // --- token_enrich::TokenEnrichment fields (appendedTokenColumns) ---
        "name" => ("name", Text),
        "created" | "created_at" => ("created_at", Text),
        "creator" | "creator_address" => ("creator_address", Text),
        "create_tx" | "create_tx_address" => ("create_tx_address", Text),
        "trade_count" => ("trade_count", Number),
        "last_trade" | "last_trade_at" => ("last_trade_at", Text),
        "last_synced" | "last_synced_at" => ("last_synced_at", Text),
        "current_price" => ("current_price", Number),
        "ath_timestamp" => ("ath_timestamp", Text),
        "market_cap" => ("market_cap", Number),
        "volume" | "volume_sol_total" => ("volume_sol_total", Number),
        "first_slot_buy" | "first_slot_buy_sol" => ("first_slot_buy_sol", Number),
        "first_slot_sell" | "first_slot_sell_sol" => ("first_slot_sell_sol", Number),
        "initial_buy" | "init_buy" | "initial_buy_sol" => ("initial_buy_sol", Number),
        "init_supply" | "initial_supply_token" => ("initial_supply_token", Number),
        "token_amount" => ("token_amount", Number),
        "max_cost_lamports" => ("max_cost_lamports", Number),
        "spendable_lamports_in" => ("spendable_lamports_in", Number),
        "min_tokens_out" => ("min_tokens_out", Number),
        "cu_limit" => ("cu_limit", Number),
        "cu_price" => ("cu_price", Number),
        "ix_count" | "ix_labels_count" => ("ix_labels_count", Number),
        // Booleans sort via `field_num`'s bool→0/1 coercion (no dedicated filter
        // UI for these columns client-side, same as the SQL-backed tables).
        "migrated" | "is_migrated" => ("is_migrated", Number),
        "dead" | "is_dead" => ("is_dead", Number),
        "mayhem_mode" | "is_mayhem_mode" => ("is_mayhem_mode", Number),
        "cashback" | "is_cashback_enabled" => ("is_cashback_enabled", Number),
        _ => return None,
    })
}

/// Read a JSON field as `f64` (number, numeric string, or bool as 0.0/1.0);
/// `None` if absent/null.
fn field_num(row: &Value, field: &str) -> Option<f64> {
    match row.get(field) {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.parse().ok(),
        Some(Value::Bool(b)) => Some(if *b { 1.0 } else { 0.0 }),
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

/// Raw (case-sensitive) `mint` string for the stable sort tiebreak — mirrors the
/// SQL `t.mint_address ASC` tail, which does NOT lowercase. `""` when absent.
fn mint_raw(row: &Value) -> &str {
    row.get("mint").and_then(Value::as_str).unwrap_or("")
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
    // extra keys, if ever sent, are ignored), then a deterministic `mint` ASC tail
    // so ties don't shuffle across page seams — matching the SQL tables'
    // `t.mint_address ASC` tiebreak (raw, case-sensitive base58). NULLS-last on the
    // primary key regardless of direction. Runs even with no sort column so paging
    // is stable in the default view too.
    let primary = req
        .sorting
        .first()
        .and_then(|s| resolve(&s.col).map(|(field, kind)| (field, kind, s.dir.is_desc())));
    matched.sort_by(|a, b| {
        let ord = match primary {
            Some((field, Kind::Number, desc)) => cmp_opt(field_num(a, field), field_num(b, field), desc),
            Some((field, Kind::Text, desc)) => cmp_opt(field_text(a, field), field_text(b, field), desc),
            None => std::cmp::Ordering::Equal,
        };
        ord.then_with(|| mint_raw(a).cmp(mint_raw(b)))
    });

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
    fn frontend_display_key_aliases_resolve() {
        // Frontend `simColumns` keys (`holding`, `pnl_pct`, `reason`) are friendlier
        // than the backend field names — must resolve identically.
        let rows = vec![
            json!({"mint":"a","symbol":"BONK","trade_count":3,"exit_reason":"TakeProfit"}),
            json!({"mint":"b","symbol":"WIF","trade_count":9,"exit_reason":"StopLoss"}),
        ];
        let r = req(json!({"filters": {"trade_count": {"op":"gt","val":5}}}));
        let (_, total) = query(&rows, &r);
        assert_eq!(total, 1, "'trade_count' (Token Trades) filters the enrichment count");

        let r = req(json!({"filters": {"reason": {"op":"eq","val":"StopLoss"}}}));
        let (page, total) = query(&rows, &r);
        assert_eq!(total, 1, "'reason' alias must filter on exit_reason");
        assert_eq!(page[0]["mint"], "b");

        let r = req(json!({"sorting": [{"col":"holding","dir":"asc"}]}));
        let (page, _) = query(&rows, &r);
        assert_eq!(page.len(), 2, "'holding' alias must not drop rows lacking holding_secs");
    }

    #[test]
    fn token_enrichment_fields_sort_and_filter() {
        // Fields flattened onto the row by `token_enrich::TokenEnrichment` — the
        // frontend's `appendedTokenColumns` display keys must alias to them.
        let rows = vec![
            json!({"mint":"a","symbol":"BONK","initial_buy_sol":0.5,"cu_price":1000,"is_migrated":true}),
            json!({"mint":"b","symbol":"WIF","initial_buy_sol":2.0,"cu_price":5000,"is_migrated":false}),
        ];

        let r = req(json!({"filters": {"initial_buy": {"op":"gt","val":1.0}}}));
        let (page, total) = query(&rows, &r);
        assert_eq!(total, 1, "'initial_buy' alias must filter on initial_buy_sol");
        assert_eq!(page[0]["mint"], "b");

        let r = req(json!({"sorting": [{"col":"cu_price","dir":"desc"}]}));
        let (page, _) = query(&rows, &r);
        assert_eq!(page[0]["mint"], "b", "cu_price sorts desc: WIF(5000) first");

        // Booleans coerce to 0.0/1.0 for sort — true (migrated) sorts before false.
        let r = req(json!({"sorting": [{"col":"migrated","dir":"desc"}]}));
        let (page, _) = query(&rows, &r);
        assert_eq!(page[0]["mint"], "a", "'migrated' alias sorts is_migrated true first (desc)");
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

    #[test]
    fn equal_sort_key_breaks_ties_by_mint_asc() {
        // Three rows share pnl_sol=1.0 — without a tiebreak their order is unstable
        // across page seams. The `mint` ASC tail must pin them to b < m < z.
        let rows = vec![
            json!({"mint":"z","symbol":"Z","pnl_sol":1.0}),
            json!({"mint":"b","symbol":"B","pnl_sol":1.0}),
            json!({"mint":"m","symbol":"M","pnl_sol":1.0}),
        ];
        let r = req(json!({"sorting": [{"col":"pnl_sol","dir":"desc"}]}));
        let (page, _) = query(&rows, &r);
        assert_eq!(
            page.iter().map(|r| r["mint"].as_str().unwrap()).collect::<Vec<_>>(),
            vec!["b", "m", "z"],
            "equal pnl_sol rows order by mint ASC"
        );
    }

    #[test]
    fn no_sort_column_still_orders_by_mint() {
        // Default view (no sort levels) must still be deterministic → mint ASC.
        let rows = vec![
            json!({"mint":"z","symbol":"Z"}),
            json!({"mint":"a","symbol":"A"}),
            json!({"mint":"m","symbol":"M"}),
        ];
        let (page, _) = query(&rows, &req(json!({})));
        assert_eq!(
            page.iter().map(|r| r["mint"].as_str().unwrap()).collect::<Vec<_>>(),
            vec!["a", "m", "z"],
            "no sort key → stable mint ASC order"
        );
    }
}
