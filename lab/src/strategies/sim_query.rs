//! In-memory server-side query over a finished backtest's per-token results.
//!
//! The Simulated token table pages/sorts/filters/searches over the **unified**
//! `TableRequest` contract — same shape as Positions/Matched — but its data source
//! is the already-resident `Vec<Value>` in [`SimResults`](crate::state::sim_results)
//! (lab is single-user, workstation RAM), so there's no DB to query. This module owns
//! only the **grammar** — which frontend column key maps to which JSON field + type
//! ([`resolve`]) — and hands it to the shared, generic evaluator
//! [`trading_core::api::table_eval::apply_table_request`], which applies the request
//! (search → filters → sort → page) with the exact same op semantics as the SQL path
//! (`strategy_repo::push_filter_predicate`). Numeric operators compare numerically; a
//! numeric op on a text field is dropped just like the SQL whitelist drops it.
//!
//! Only whitelisted keys are honored (unknown → ignored). Several columns use a
//! friendlier display key than the underlying JSON field, so those are aliased here.

use serde_json::Value;

use trading_core::api::table_eval::{apply_table_request, resolve_token_enrichment_key, ColKind};
use trading_core::api::table_query::TableRequest;

/// Resolve a frontend column key to the JSON field it reads + its type. `None` =
/// not filterable/sortable (dropped). Mirrors the frontend `simColumns` +
/// `appendedTokenColumns` keys — several columns use a friendlier display key
/// than the underlying JSON field name, so those are aliased here too. The
/// `appendedTokenColumns` set (`creator`, `trade_count`, `initial_buy`, `cu_limit`,
/// `migrated`, ...) reads token metadata that `token_enrich::TokenEnrichment`
/// flattens onto the row — see that module for where it's populated.
fn resolve(key: &str) -> Option<(&'static str, ColKind)> {
    use ColKind::{Number, Text};
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
        // The sim row owns its own `created_at` (the token's), so it maps `created`
        // here rather than through the shared enrichment resolver (which excludes it).
        "created" | "created_at" => ("created_at", Text),

        // --- shared token_enrich::TokenEnrichment fields (appendedTokenColumns) ---
        // Single-sourced with the live Holdings table via `resolve_token_enrichment_key`.
        _ => return resolve_token_enrichment_key(key),
    })
}

/// One page of a finished sim's rows after applying the request's search + filters +
/// sort, plus the total match count (before paging) for `X-Total-Count`. The returned
/// rows are cloned refs into the shared `Arc` payload. Thin adapter over the shared
/// [`apply_table_request`] evaluator with the sim's [`resolve`] grammar.
pub fn query(rows: &[Value], req: &TableRequest) -> (Vec<Value>, usize) {
    apply_table_request(rows, req, resolve)
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
