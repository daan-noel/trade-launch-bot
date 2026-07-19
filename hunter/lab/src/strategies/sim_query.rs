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

use trading_core::api::table_eval::{apply_table_request, filter_table_request, resolve_token_enrichment_key, ColKind};
use trading_core::api::table_query::TableRequest;
use trading_core::strategies::kernel::{run_summary, ExitCode, RunSummary, TokenOutcome};

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
        "mint_address" => ("mint_address", Text),
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

/// Every row matching `req`'s search + filters (no sort/page) — for summary roll-ups.
pub fn filter_rows(rows: &[Value], req: &TableRequest) -> Vec<Value> {
    filter_table_request(rows, req, resolve)
}

/// Narrow one sim result row (the JSON shape [`super::replay::outcome_to_row`]
/// emits) to the kernel's [`TokenOutcome`]. Every row in a finished sim's payload
/// is an *entered* position — the replay only emits a row once an entry filled —
/// so `fired` is unconditionally true and `n_fired` is the row count.
///
/// "Closed" is decided by `exit_reason`, **not** by `exit_time != null`: the
/// analysis-only death-close (`ExitCode::Dead`) is a genuine close that carries no
/// exit tx/time, and reading `exit_time` would misfile it as open. `exit_reason` is
/// the same discriminator the sweep aggregates on, which is the point.
fn row_to_outcome(row: &Value) -> TokenOutcome {
    let num = |k: &str| -> f64 { row.get(k).and_then(Value::as_f64).unwrap_or(0.0) };
    let exit = row
        .get("exit_reason")
        .and_then(Value::as_str)
        .map(ExitCode::from_reason)
        .unwrap_or(ExitCode::Open);
    TokenOutcome {
        fired: true,
        // Open rows carry `holding_secs: null`; the kernel excludes them from every
        // holding statistic anyway, so 0 is never summed.
        holding_secs: row.get("holding_secs").and_then(Value::as_i64).unwrap_or(0),
        pnl_percent: num("pnl_percent") as f32,
        pnl_sol: num("pnl_sol") as f32,
        exit,
    }
}

/// Aggregate rollup over a finished sim's rows, shared by the filtered
/// Simulated-summary card
/// ([`super::super::api::handlers::strategies::positions::sim_result_summary`])
/// and the unfiltered rules-table last-simulation rollup ([`super::sim_spawn`]).
///
/// **Delegates to the core kernel** ([`run_summary`]) rather than counting here,
/// so a single-rule simulate and a grouped-sweep combo over the same outcomes
/// produce byte-identical numbers — same realized-only semantics in the
/// `realized` band (a still-`Open` mark feeds `n_fired`/`n_open`/`open_pnl_sol`
/// and nothing else), same win-rate denominator, same exit-code buckets, and the
/// same `mtm` counterpart band. The previous hand-rolled version summed open
/// marks into a single `total_pnl_sol` and averaged `pnl_percent` over open rows
/// too, so a rule holding its losers open read as profitable here while the sweep
/// reported the loss (parity plan B1-B4).
///
/// Exact (not sketch) quantiles: a sim's row set is bounded — one rule over one
/// corpus, already resident in RAM — so this matches the sweep **drill-in**
/// (`ComboMetrics::exact_from_rows`) precisely. The persisted sweep row goes
/// through the streaming DDSketch instead and carries ~15% error on the two
/// interior quantiles; that is a property of the unbounded combos × tokens fold,
/// not a parity break here.
pub fn summarize(rows: &[Value]) -> RunSummary {
    let outcomes: Vec<TokenOutcome> = rows.iter().map(row_to_outcome).collect();
    run_summary(outcomes.iter())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rows() -> Vec<Value> {
        vec![
            json!({"mint_address":"a","symbol":"BONK","pnl_percent":10.0,"pnl_sol":1.0,"exit_time":"2026-01-01T00:00:00Z"}),
            json!({"mint_address":"b","symbol":"pumpcat","pnl_percent":-5.0,"pnl_sol":-0.5,"exit_time":"2026-01-01T00:00:00Z"}),
            json!({"mint_address":"c","symbol":"WIF","pnl_percent":50.0,"pnl_sol":2.0,"exit_time":null}),
        ]
    }

    fn req(json: serde_json::Value) -> TableRequest {
        serde_json::from_value(json).expect("TableRequest")
    }

    // ── summary parity (plan B1-B4) ─────────────────────────────────────────

    /// A sim row as `outcome_to_row` emits it: `exit_reason` always present,
    /// `exit_time`/`holding_secs` null on a still-open position.
    fn sim_row(pnl_sol: f64, pnl_pct: f64, exit: &str, holding: Option<i64>) -> Value {
        json!({
            "mint_address": "m", "symbol": "S",
            "pnl_sol": pnl_sol, "pnl_percent": pnl_pct,
            "exit_reason": exit,
            "holding_secs": holding,
            "exit_time": if exit == "Open" { Value::Null } else { json!("2026-01-01T00:00:00Z") },
        })
    }

    #[test]
    fn open_marks_are_excluded_from_the_headline_total() {
        // The regression this whole change exists for: a rule whose losers closed
        // and whose big winner is still open must NOT report the open mark in
        // `total_pnl_sol`. The old hand-rolled rollup summed every row.
        let rows = vec![
            sim_row(1.0, 50.0, "TakeProfit", Some(10)),
            sim_row(-1.0, -50.0, "StopLoss", Some(10)),
            sim_row(1_000.0, 5_000.0, "Open", None),
        ];
        let m = summarize(&rows).realized;
        assert_eq!(m.n_fired, 3);
        assert_eq!(m.n_open, 1);
        assert_eq!(m.n_closed, 2);
        assert!((m.total_pnl_sol - 0.0).abs() < 1e-9, "realized total excludes the open mark");
        assert!((m.open_pnl_sol - 1_000.0).abs() < 1e-9, "open mark surfaced separately");
        assert!((m.win_rate - 0.5).abs() < 1e-9, "win rate over closed only");
        assert_eq!(m.best_pnl_pct, 50.0, "the open mark must not become the best");
    }

    #[test]
    fn death_close_counts_as_closed_despite_a_null_exit_time() {
        // `ExitCode::Dead` is a genuine close that carries no exit tx/time. The old
        // rollup keyed "closed" off `exit_time != null` and so booked it as open.
        let rows = vec![sim_row(-0.4, -80.0, "Dead", Some(600))];
        let m = summarize(&rows).realized;
        assert_eq!(m.n_closed, 1, "a death-close is closed");
        assert_eq!(m.n_open, 0);
        assert_eq!(m.n_exit_dead, 1);
        assert!((m.total_pnl_sol - -0.4).abs() < 1e-6, "its loss lands in the realized total");
    }

    #[test]
    fn simulate_summary_equals_the_sweep_drill_in_on_the_same_outcomes() {
        // The parity lock: identical outcomes must roll up identically through the
        // simulate path (JSON rows → `summarize`) and the grouped-sweep drill-in
        // (`ComboTokenResult` rows → `ComboMetrics::exact_from_rows`). Both now
        // delegate to `exact_run_metrics`, so this can only break if one of them
        // grows a private aggregate again.
        use crate::sweep::aggregate::ComboMetrics;
        use trading_core::models::grouped_sweep::ComboTokenResult;

        let specs: Vec<(f64, f64, &str, Option<i64>)> = vec![
            (2.0, 100.0, "TakeProfit", Some(10)),
            (-1.0, -50.0, "StopLoss", Some(20)),
            (0.5, 25.0, "Metrics", Some(35)),
            (-0.4, -80.0, "Dead", Some(600)),
            (5.0, 999.0, "Open", None),
        ];

        let sim_rows: Vec<Value> =
            specs.iter().map(|&(sol, pct, ex, h)| sim_row(sol, pct, ex, h)).collect();
        let sweep_rows: Vec<ComboTokenResult> = specs
            .iter()
            .map(|&(sol, pct, ex, h)| ComboTokenResult {
                mint_address: "m".into(),
                symbol: "S".into(),
                fired: true,
                pnl_sol: sol as f32,
                pnl_pct: pct as f32,
                holding_secs: h.unwrap_or(0),
                exit: ex.into(),
                entry_time: None,
                entry_price: None,
                entry_tx: None,
                entry_slot: None,
                exit_time: None,
                exit_price: None,
                exit_tx: None,
                exit_slot: None,
                created_at: None,
                ath_price: None,
                token: Default::default(),
            })
            .collect();

        let sim = summarize(&sim_rows).realized;
        let sweep = ComboMetrics::exact_from_rows(0, &sweep_rows);

        assert_eq!(sim.n_fired, sweep.n_fired);
        assert_eq!(sim.n_open, sweep.n_open);
        assert_eq!(sim.n_closed, sweep.n_closed);
        assert!((sim.win_rate - sweep.win_rate).abs() < 1e-9);
        assert!((sim.total_pnl_sol - sweep.total_pnl_sol).abs() < 1e-6);
        assert!((sim.open_pnl_sol - sweep.open_pnl_sol).abs() < 1e-6);
        assert!((sim.mean_pnl_pct - sweep.mean_pnl_pct).abs() < 1e-6);
        assert!((sim.median_pnl_pct - sweep.median_pnl_pct).abs() < 1e-6);
        assert!((sim.expectancy_sol - sweep.expectancy_sol).abs() < 1e-6);
        assert!((sim.avg_holding_secs - sweep.avg_holding_secs).abs() < 1e-9);
        assert_eq!(sim.n_exit_dead, sweep.n_exit_dead);
        assert_eq!(sim.n_exit_metrics, sweep.n_exit_metrics);
        assert_eq!(sim.profit_factor.is_some(), sweep.profit_factor.is_some());
    }

    #[test]
    fn filter_rows_matches_query_total_without_paging() {
        let r = req(json!({"filters": {"pnl_percent": {"op":"gt","val":5}}}));
        let (_, total) = query(&rows(), &r);
        let filtered = filter_rows(&rows(), &r);
        assert_eq!(filtered.len(), total, "filter_rows count must match query total");
        assert_eq!(filtered.len(), 2);
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
            json!({"mint_address":"a","symbol":"BONK","trade_count":3,"exit_reason":"TakeProfit"}),
            json!({"mint_address":"b","symbol":"WIF","trade_count":9,"exit_reason":"StopLoss"}),
        ];
        let r = req(json!({"filters": {"trade_count": {"op":"gt","val":5}}}));
        let (_, total) = query(&rows, &r);
        assert_eq!(total, 1, "'trade_count' (Token Trades) filters the enrichment count");

        let r = req(json!({"filters": {"reason": {"op":"eq","val":"StopLoss"}}}));
        let (page, total) = query(&rows, &r);
        assert_eq!(total, 1, "'reason' alias must filter on exit_reason");
        assert_eq!(page[0]["mint_address"], "b");

        let r = req(json!({"sorting": [{"col":"holding","dir":"asc"}]}));
        let (page, _) = query(&rows, &r);
        assert_eq!(page.len(), 2, "'holding' alias must not drop rows lacking holding_secs");
    }

    #[test]
    fn token_enrichment_fields_sort_and_filter() {
        // Fields flattened onto the row by `token_enrich::TokenEnrichment` — the
        // frontend's `appendedTokenColumns` display keys must alias to them.
        let rows = vec![
            json!({"mint_address":"a","symbol":"BONK","initial_buy_sol":0.5,"cu_price":1000,"is_migrated":true}),
            json!({"mint_address":"b","symbol":"WIF","initial_buy_sol":2.0,"cu_price":5000,"is_migrated":false}),
        ];

        let r = req(json!({"filters": {"initial_buy": {"op":"gt","val":1.0}}}));
        let (page, total) = query(&rows, &r);
        assert_eq!(total, 1, "'initial_buy' alias must filter on initial_buy_sol");
        assert_eq!(page[0]["mint_address"], "b");

        let r = req(json!({"sorting": [{"col":"cu_price","dir":"desc"}]}));
        let (page, _) = query(&rows, &r);
        assert_eq!(page[0]["mint_address"], "b", "cu_price sorts desc: WIF(5000) first");

        // Booleans coerce to 0.0/1.0 for sort — true (migrated) sorts before false.
        let r = req(json!({"sorting": [{"col":"migrated","dir":"desc"}]}));
        let (page, _) = query(&rows, &r);
        assert_eq!(page[0]["mint_address"], "a", "'migrated' alias sorts is_migrated true first (desc)");
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
        assert_eq!(page[0]["mint_address"], "c", "WIF(50) first desc");
        assert_eq!(page[1]["mint_address"], "a", "BONK(10) second");
    }

    #[test]
    fn equal_sort_key_breaks_ties_by_mint_asc() {
        // Three rows share pnl_sol=1.0 — without a tiebreak their order is unstable
        // across page seams. The `mint` ASC tail must pin them to b < m < z.
        let rows = vec![
            json!({"mint_address":"z","symbol":"Z","pnl_sol":1.0}),
            json!({"mint_address":"b","symbol":"B","pnl_sol":1.0}),
            json!({"mint_address":"m","symbol":"M","pnl_sol":1.0}),
        ];
        let r = req(json!({"sorting": [{"col":"pnl_sol","dir":"desc"}]}));
        let (page, _) = query(&rows, &r);
        assert_eq!(
            page.iter().map(|r| r["mint_address"].as_str().unwrap()).collect::<Vec<_>>(),
            vec!["b", "m", "z"],
            "equal pnl_sol rows order by mint ASC"
        );
    }

    #[test]
    fn no_sort_column_still_orders_by_mint() {
        // Default view (no sort levels) must still be deterministic → mint ASC.
        let rows = vec![
            json!({"mint_address":"z","symbol":"Z"}),
            json!({"mint_address":"a","symbol":"A"}),
            json!({"mint_address":"m","symbol":"M"}),
        ];
        let (page, _) = query(&rows, &req(json!({})));
        assert_eq!(
            page.iter().map(|r| r["mint_address"].as_str().unwrap()).collect::<Vec<_>>(),
            vec!["a", "m", "z"],
            "no sort key → stable mint ASC order"
        );
    }
}
