//! Generic in-memory evaluator for the shared [`TableRequest`](super::table_query::TableRequest)
//! contract over `serde_json::Value` rows.
//!
//! This is the Rust half of the "one shared method" for token tables whose data is
//! already resident in RAM (no DB to page): the Simulated table's finished-backtest
//! rows today, any future client-fed table tomorrow. It embodies the **same**
//! op semantics as the SQL path (`strategy_repo::push_filter_predicate` /
//! `handlers::tokens::sql`): numeric operators compare numerically, a numeric op on
//! a text column is dropped, a null numeric field can't satisfy a numeric predicate,
//! `contains` on a number degrades to equality, free-text search is mint/symbol only,
//! and ties break by raw (case-sensitive) `mint` ASC so paging is stable.
//!
//! Callers own the **grammar** (which frontend key maps to which JSON field + type)
//! and hand it in as a `resolve` closure — the whitelist stays next to the table's
//! row shape while the evaluation machinery lives here once. A parallel **TS**
//! evaluator mirrors this for client-resident tables (Wallet Holdings); the two are
//! held at parity by a shared-fixture conformance test.

use serde_json::Value;

use super::table_query::{FilterOp, FilterSpec, SortSpec, TableRequest};

/// Column type for a row field — decides which ops are legal and how a value
/// compares (string `contains`/`eq` vs numeric compare), mirroring the SQL
/// `FilterKind`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColKind {
    Text,
    Number,
}

/// Resolves a frontend column key to the JSON field it reads + its type. `None` =
/// not filterable/sortable (the key is dropped, exactly as an unknown key is dropped
/// from the SQL whitelist). Implemented by any `Fn(&str) -> Option<(&str, ColKind)>`.
pub trait ColResolver {
    fn resolve(&self, key: &str) -> Option<(&'static str, ColKind)>;
}

impl<F: Fn(&str) -> Option<(&'static str, ColKind)>> ColResolver for F {
    fn resolve(&self, key: &str) -> Option<(&'static str, ColKind)> {
        (self)(key)
    }
}

/// The two fields free-text search scans, everywhere (locked project decision — see
/// CLAUDE.md / Phase 6). Kept here so the Rust and TS evaluators can't diverge on it.
const SEARCH_FIELDS: [&str; 2] = ["mint_address", "symbol"];

/// Resolve a frontend column key that names a **shared token-enrichment** field
/// (the `appendedTokenColumns` set flattened from
/// [`crate::storage::token_enrichment::TokenEnrichment`]) to its JSON field name +
/// [`ColKind`]. `None` = not an enrichment key.
///
/// This is the SSOT for the in-memory-evaluator half of the enrichment grammar:
/// every in-RAM token table that flattens `TokenEnrichment` (the lab Simulated
/// table, the live Holdings table) resolves its enrichment columns through here,
/// then layers its own row-owned keys on top via `.or_else(...)`. Field names match
/// the serialized `TokenEnrichment` JSON (and the frontend `TOKEN_ENRICH_FIELDS`
/// exactly); the accepted display-key aliases mirror the frontend
/// `appendedTokenColumns` keys. `created`/`created_at` is deliberately **not** here
/// — it's row-owned and diverges per table (a position's vs. a token's date), so
/// each host maps it itself.
pub fn resolve_token_enrichment_key(key: &str) -> Option<(&'static str, ColKind)> {
    use ColKind::{Number, Text};
    Some(match key {
        "name" => ("name", Text),
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
        // Booleans sort/filter via the evaluator's bool→0/1 coercion.
        "migrated" | "is_migrated" => ("is_migrated", Number),
        "dead" | "is_dead" => ("is_dead", Number),
        "mayhem_mode" | "is_mayhem_mode" => ("is_mayhem_mode", Number),
        "cashback" | "is_cashback_enabled" => ("is_cashback_enabled", Number),
        _ => return None,
    })
}

/// Read a JSON field as `f64` (number, numeric string, or bool as 0.0/1.0); `None`
/// if absent/null.
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

/// Raw (case-sensitive) `mint` string for the stable sort tiebreak — mirrors the SQL
/// `t.mint_address ASC` tail, which does NOT lowercase. `""` when absent.
fn mint_raw(row: &Value) -> &str {
    row.get("mint_address").and_then(Value::as_str).unwrap_or("")
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

/// Does `row` satisfy one structured filter? A shape/type mismatch (numeric op on a
/// text field, non-numeric operand, missing bound) is treated as **not a
/// constraint** → keeps the row, mirroring the SQL side dropping the predicate.
fn row_matches(row: &Value, field: &str, kind: ColKind, spec: &FilterSpec) -> bool {
    match (kind, spec.op) {
        (ColKind::Text, FilterOp::Contains) => match operand_text(&spec.val) {
            Some(needle) if !needle.is_empty() => {
                field_text(row, field).is_some_and(|hay| hay.contains(&needle))
            }
            _ => true,
        },
        (ColKind::Text, FilterOp::Eq) => match operand_text(&spec.val) {
            Some(needle) if !needle.is_empty() => {
                field_text(row, field).is_some_and(|hay| hay == needle)
            }
            _ => true,
        },
        // Set membership: `val` is a JSON array; the row's field must equal one of
        // its (trimmed, lowercased) entries. An empty/absent/non-array operand is
        // not a constraint (keeps the row), mirroring the SQL side dropping it.
        (ColKind::Text, FilterOp::In) => match spec.val.as_array() {
            Some(arr) => {
                let set: Vec<String> = arr.iter().filter_map(operand_text).collect();
                if set.is_empty() {
                    true
                } else {
                    field_text(row, field).is_some_and(|hay| set.iter().any(|s| *s == hay))
                }
            }
            None => true,
        },
        // Other numeric op on a text field → not a constraint.
        (ColKind::Text, _) => true,

        (ColKind::Number, FilterOp::Between) => {
            match (operand_num(&spec.min), operand_num(&spec.max)) {
                (Some(min), Some(max)) => {
                    field_num(row, field).is_some_and(|v| v >= min && v <= max)
                }
                _ => true,
            }
        }
        (ColKind::Number, op) => match operand_num(&spec.val) {
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

/// Does `row` match the free-text search over mint / symbol? Blank search (already
/// trimmed+lowercased by the caller) matches everything.
fn matches_search(row: &Value, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    SEARCH_FIELDS
        .iter()
        .filter_map(|f| field_text(row, f))
        .any(|hay| hay.contains(needle))
}

/// Ordering for `Option<T>` values with NULLS-last semantics under both directions:
/// a present value always sorts before an absent one.
fn cmp_opt<T: PartialOrd>(a: Option<T>, b: Option<T>, desc: bool) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (Some(a), Some(b)) => {
            let ord = a.partial_cmp(&b).unwrap_or(Ordering::Equal);
            if desc { ord.reverse() } else { ord }
        }
        (Some(_), None) => Ordering::Less, // present before absent (NULLS last)
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// Compare two rows under one resolved sort key.
fn cmp_by_key(a: &Value, b: &Value, field: &str, kind: ColKind, desc: bool) -> std::cmp::Ordering {
    match kind {
        ColKind::Number => cmp_opt(field_num(a, field), field_num(b, field), desc),
        ColKind::Text => cmp_opt(field_text(a, field), field_text(b, field), desc),
    }
}

/// Apply the shared `TableRequest` (search → filters → sort → page) to in-memory
/// `serde_json::Value` rows, using `resolve` for the column grammar. Returns one page
/// of **cloned** rows plus the total match count (before paging) for `X-Total-Count`.
///
/// Sort applies every resolved sort key in order (unknown keys dropped), then a
/// deterministic raw-`mint` ASC tail so ties don't shuffle across page seams. It runs
/// even with no sort key, so the default view pages stably too.
pub fn apply_table_request<R: ColResolver>(
    rows: &[Value],
    req: &TableRequest,
    resolve: R,
) -> (Vec<Value>, usize) {
    let needle = req.search.trim().to_lowercase();

    // Resolve filters once (key → field/kind), dropping unknown keys.
    let filters: Vec<(&'static str, ColKind, &FilterSpec)> = req
        .filters
        .iter()
        .filter_map(|(key, spec)| resolve.resolve(key).map(|(field, kind)| (field, kind, spec)))
        .collect();

    let mut matched: Vec<&Value> = rows
        .iter()
        .filter(|row| matches_search(row, &needle))
        .filter(|row| filters.iter().all(|(field, kind, spec)| row_matches(row, field, *kind, spec)))
        .collect();

    let total = matched.len();

    // Resolve all sort keys once (drop unknown), then sort by them in order followed
    // by the raw-`mint` ASC tiebreak.
    let sort_keys: Vec<(&'static str, ColKind, bool)> = req
        .sorting
        .iter()
        .filter_map(|SortSpec { col, dir }| {
            resolve.resolve(col).map(|(field, kind)| (field, kind, dir.is_desc()))
        })
        .collect();
    matched.sort_by(|a, b| {
        sort_keys
            .iter()
            .map(|(field, kind, desc)| cmp_by_key(a, b, field, *kind, *desc))
            .find(|ord| ord.is_ne())
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| mint_raw(a).cmp(mint_raw(b)))
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Tiny resolver mirroring a couple of sim fields for the evaluator's own tests.
    fn resolve(key: &str) -> Option<(&'static str, ColKind)> {
        use ColKind::{Number, Text};
        Some(match key {
            "mint_address" => ("mint_address", Text),
            "symbol" => ("symbol", Text),
            "reason" => ("exit_reason", Text),
            "pnl" => ("pnl_percent", Number),
            "trades" => ("trade_count", Number),
            _ => return None,
        })
    }

    fn req(json: serde_json::Value) -> TableRequest {
        serde_json::from_value(json).expect("TableRequest")
    }

    fn rows() -> Vec<Value> {
        vec![
            json!({"mint_address":"a","symbol":"BONK","pnl_percent":10.0,"trade_count":3}),
            json!({"mint_address":"b","symbol":"pumpcat","pnl_percent":-5.0,"trade_count":9}),
            json!({"mint_address":"c","symbol":"WIF","pnl_percent":50.0,"trade_count":1}),
        ]
    }

    #[test]
    fn numeric_gt_filters() {
        let (_, total) = apply_table_request(&rows(), &req(json!({"filters": {"pnl": {"op":"gt","val":5}}})), resolve);
        assert_eq!(total, 2, "pnl > 5 keeps 10 + 50");
    }

    #[test]
    fn numeric_op_on_text_is_ignored() {
        let (_, total) = apply_table_request(&rows(), &req(json!({"filters": {"symbol": {"op":"gt","val":5}}})), resolve);
        assert_eq!(total, 3);
    }

    #[test]
    fn search_is_mint_symbol_only() {
        let (_, total) = apply_table_request(&rows(), &req(json!({"search": "pump"})), resolve);
        assert_eq!(total, 1);
    }

    #[test]
    fn unknown_sort_key_falls_back_to_mint_tail() {
        let rows = vec![json!({"mint_address":"z"}), json!({"mint_address":"a"}), json!({"mint_address":"m"})];
        let (page, _) = apply_table_request(&rows, &req(json!({"sorting":[{"col":"nope","dir":"desc"}]})), resolve);
        assert_eq!(
            page.iter().map(|r| r["mint_address"].as_str().unwrap()).collect::<Vec<_>>(),
            vec!["a", "m", "z"],
            "unknown sort key dropped → stable mint ASC"
        );
    }

    #[test]
    fn conformance_shared_fixtures() {
        // Golden fixtures pinning this evaluator's op/sort/search/tiebreak/paging
        // semantics. Originally shared with a TS twin (`tableEval.conformance.test.ts`)
        // that retired when Wallet Holdings moved server-side (Phase 4) — the JSON
        // stays a **Rust-only** fixture (do not delete it: this test `include_str!`s
        // it). See the JSON's `_comment` for the grammar (it matches `resolve` above).
        let data: Value = serde_json::from_str(include_str!(
            "../../../frontend-react/src/shared/services/tableEval.fixtures.json"
        ))
        .expect("fixtures parse");
        let rows: Vec<Value> = data["rows"].as_array().expect("rows array").clone();
        for case in data["cases"].as_array().expect("cases array") {
            let name = case["name"].as_str().unwrap_or("<unnamed>");
            let request: TableRequest =
                serde_json::from_value(case["request"].clone()).expect("request");
            let (page, _) = apply_table_request(&rows, &request, resolve);
            let got: Vec<&str> = page.iter().map(|r| r["mint_address"].as_str().unwrap()).collect();
            let want: Vec<&str> = case["expected"]
                .as_array()
                .expect("expected array")
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect();
            assert_eq!(got, want, "case `{name}`");
        }
    }

    #[test]
    fn multi_key_sort_then_mint_tail() {
        // Two rows tie on pnl; the second sort key (trades ASC) breaks it, and equal
        // on both would fall to mint ASC.
        let rows = vec![
            json!({"mint_address":"z","pnl_percent":10.0,"trade_count":5}),
            json!({"mint_address":"a","pnl_percent":10.0,"trade_count":2}),
            json!({"mint_address":"m","pnl_percent":10.0,"trade_count":5}),
        ];
        let r = req(json!({"sorting":[{"col":"pnl","dir":"desc"},{"col":"trades","dir":"asc"}]}));
        let (page, _) = apply_table_request(&rows, &r, resolve);
        assert_eq!(
            page.iter().map(|r| r["mint_address"].as_str().unwrap()).collect::<Vec<_>>(),
            vec!["a", "m", "z"],
            "trades ASC breaks the pnl tie (a=2 first); m,z tie on both → mint ASC"
        );
    }
}
