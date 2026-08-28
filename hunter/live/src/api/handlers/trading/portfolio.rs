//! Portfolio/PnL read endpoints — the `/api/portfolio/*` surface the
//! Holdings, Home, and Live-Trading pages read. Thin handlers over
//! [`crate::services::portfolio`] (the composition SSOT); no logic lives here.

use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use serde::Deserialize;
use serde_json::Value;

use trading_core::api::table_eval::{apply_table_request, resolve_token_enrichment_key, ColKind};
use trading_core::api::table_query::{Page, TableRequest};

use crate::services::portfolio::{self, cash_summary, partition_cash, HoldingsTableSummary};
use crate::state::deploy_state::DeployState;

/// Column grammar for the server-paged Holdings table: frontend column key → the
/// serialized [`portfolio::PortfolioHolding`] JSON field + type. Row-owned wallet
/// columns are bespoke here; the shared token-enrichment columns fall through to
/// [`resolve_token_enrichment_key`] (the SSOT the lab Simulated table also uses).
///
/// **Live marks** (`value_usd`/`value_sol`/`price_usd`/`price_change_24h`/
/// `liquidity`) are the composition's **scan-time** snapshot — server-sortable /
/// filterable on that snapshot; the client price-poll overlays fresher *display*
/// values on the current page only. `managed_by` is a nested object, so it's
/// intentionally absent (not cleanly comparable in the in-memory evaluator) — its
/// column is display-only.
fn holdings_resolve(key: &str) -> Option<(&'static str, ColKind)> {
    use ColKind::{Number, Text};
    Some(match key {
        // identity + on-chain balance (row-owned on PortfolioHolding)
        "mint_address" => ("mint_address", Text),
        "symbol" => ("symbol", Text),
        "asset_kind" => ("asset_kind", Text),
        "ui_amount" => ("ui_amount", Number),
        "amount" => ("amount", Number),
        "decimals" => ("decimals", Number),
        "token_program" => ("token_program_id", Text),
        "token_account" => ("token_account", Text),
        "token_created_at" => ("token_created_at", Text),
        // live marks (scan-time snapshot — see fn docs)
        "value_usd" => ("value_usd", Number),
        "value_sol" => ("value_sol", Number),
        "price_usd" => ("price_usd", Number),
        "price_change_24h" => ("price_change_24h", Number),
        "liquidity" => ("liquidity", Number),
        // SOL cost basis + unrealized PnL (SSOT compute)
        "cost_basis_sol" => ("cost_basis_sol", Number),
        "unrealized_pnl" => ("unrealized_pnl_sol", Number),
        "unrealized_pnl_pct" => ("unrealized_pnl_pct", Number),
        // shared token-enrichment columns (name, creator, mcap, flags, migrated, …)
        _ => return resolve_token_enrichment_key(key),
    })
}

/// Serialize the composed holdings to JSON rows the in-memory evaluator reads.
fn holdings_to_values(holdings: &[&portfolio::PortfolioHolding]) -> Vec<Value> {
    holdings
        .iter()
        .map(|h| serde_json::to_value(h).unwrap_or(Value::Null))
        .collect()
}

/// `GET /api/portfolio/holdings`
///
/// Full enriched wallet holdings (unpaged) — backs the Home top-holdings widget +
/// live-trade feed. Includes cash (USDC) with `asset_kind: "cash"`; consumers that
/// want meme bags only must filter. The Holdings **page** uses the paged POST
/// below (positions only); both share the short-TTL scan cache.
pub async fn get_portfolio_holdings(app_state: web::Data<Arc<DeployState>>) -> impl Responder {
    match portfolio::list_holdings_cached(app_state.get_ref(), false).await {
        Ok(holdings) => HttpResponse::Ok().json(holdings.as_ref()),
        Err(e) => {
            tracing::warn!("get_portfolio_holdings failed: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({ "error": e.to_string() }))
        }
    }
}

/// Query params for the paged Holdings reads. `fresh=true` busts the scan cache before
/// serving — the Wallet page sends it once after a confirmed trade so the reload
/// reflects the new on-chain balance immediately (normal paging/sort/filter omit it
/// and reuse the warm scan). Serde's bool query deserializer only accepts
/// `true`/`false` (not `1`/`0`).
#[derive(Deserialize)]
pub struct HoldingsQueryParams {
    #[serde(default)]
    pub fresh: bool,
}

/// `POST /api/portfolio/holdings/query[?fresh=true]`
///
/// One page of the wallet **meme positions** under the unified [`TableRequest`]
/// contract (server-side search/sort/filter/paging). Cash (USDC) is excluded here —
/// it rides the summary's `cash_holdings` strip instead. Runs the in-memory
/// [`apply_table_request`] evaluator over the composed positions (bounded — tens of
/// tokens) with [`holdings_resolve`]; the full match count rides `X-Total-Count`.
pub async fn query_portfolio_holdings(
    app_state: web::Data<Arc<DeployState>>,
    query: web::Query<HoldingsQueryParams>,
    body: web::Json<TableRequest>,
) -> impl Responder {
    let req = body.into_inner();
    let holdings = match portfolio::list_holdings_cached(app_state.get_ref(), query.fresh).await {
        Ok(h) => h,
        Err(e) => {
            tracing::warn!("query_portfolio_holdings failed: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": e.to_string() }));
        }
    };
    let (_cash, positions) = partition_cash(&holdings);
    let values = holdings_to_values(&positions);
    let (page, total) = apply_table_request(&values, &req, holdings_resolve);
    HttpResponse::Ok()
        .insert_header(("X-Total-Count", total.to_string()))
        .insert_header(("Access-Control-Expose-Headers", "X-Total-Count"))
        .json(page)
}

/// `POST /api/portfolio/holdings/summary`
///
/// Roll-up for the page's summary bar + cash strip. Cash is always taken from the
/// full scan (unfiltered). Native SOL comes from the push-fed trader cache (no
/// RPC). Position metrics (value/PnL/24h/count) use the same filter body as the
/// table so the bar agrees with the meme-position grid.
pub async fn portfolio_holdings_summary(
    app_state: web::Data<Arc<DeployState>>,
    body: web::Json<TableRequest>,
) -> impl Responder {
    let mut req = body.into_inner();
    // Measure the whole filtered set (holdings are tens of rows; 1000 covers all).
    req.pagination = Page { page: 1, page_size: 1000 };
    req.sorting.clear();
    let holdings = match portfolio::list_holdings_cached(app_state.get_ref(), false).await {
        Ok(h) => h,
        Err(e) => {
            tracing::warn!("portfolio_holdings_summary failed: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": e.to_string() }));
        }
    };
    let (cash, positions) = partition_cash(&holdings);
    let (cash_holdings, cash_usd, cash_sol) = cash_summary(&cash);
    let (sol_balance_sol, sol_balance_usd) = portfolio::native_sol_balance(app_state.get_ref());

    let values = holdings_to_values(&positions);
    let (rows, _) = apply_table_request(&values, &req, holdings_resolve);

    let num = |row: &Value, k: &str| row.get(k).and_then(Value::as_f64);
    let mut s = HoldingsTableSummary {
        positions: rows.len(),
        sol_balance_sol,
        sol_balance_usd,
        cash_holdings,
        cash_value_usd: (!cash.is_empty()).then_some(cash_usd),
        cash_value_sol: cash_sol,
        ..Default::default()
    };
    let (mut pos_sol, mut pos_usd, mut pnl, mut has_value, mut has_pnl) =
        (0.0, 0.0, 0.0, false, false);
    let (mut wchange, mut wweight) = (0.0, 0.0);
    for row in &rows {
        // Defensive: never fold a cash row into position math if one slips through.
        if row.get("asset_kind").and_then(Value::as_str) == Some("cash") {
            continue;
        }
        if let Some(v) = num(row, "value_usd") {
            pos_usd += v;
            has_value = true;
            if let Some(c) = num(row, "price_change_24h") {
                wchange += v * c;
                wweight += v;
            }
        }
        if let Some(v) = num(row, "value_sol") {
            pos_sol += v;
        }
        if let Some(v) = num(row, "cost_basis_sol") {
            s.total_cost_basis_sol += v;
        }
        if let Some(v) = num(row, "unrealized_pnl_sol") {
            pnl += v;
            has_pnl = true;
        }
    }
    s.positions_value_usd = has_value.then_some(pos_usd);
    s.positions_value_sol = has_value.then_some(pos_sol);
    s.total_unrealized_pnl_sol = has_pnl.then_some(pnl);
    s.change_24h_pct = (wweight > 0.0).then(|| wchange / wweight);

    let has_cash = s.cash_value_usd.is_some();
    s.total_value_usd = match (has_cash, has_value) {
        (true, true) => Some(cash_usd + pos_usd),
        (true, false) => Some(cash_usd),
        (false, true) => Some(pos_usd),
        (false, false) => None,
    };
    s.total_value_sol = match (cash_sol, has_value) {
        (Some(cs), true) => Some(cs + pos_sol),
        (Some(cs), false) => Some(cs),
        (None, true) => Some(pos_sol),
        (None, false) => None,
    };

    HttpResponse::Ok().json(s)
}

/// `GET /api/portfolio/summary`
///
/// Wallet-wide roll-up (value SOL/USD, total unrealized PnL, held-bag count,
/// realized PnL today, active real rules, open real positions) — the Home KPI row.
pub async fn get_portfolio_summary(app_state: web::Data<Arc<DeployState>>) -> impl Responder {
    match portfolio::summary(app_state.get_ref()).await {
        Ok(summary) => HttpResponse::Ok().json(summary),
        Err(e) => {
            tracing::warn!("get_portfolio_summary failed: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({ "error": e.to_string() }))
        }
    }
}

#[derive(Deserialize)]
pub struct PositionsQuery {
    /// Restrict to live-money positions. Defaults to `true` — the Live-Trading
    /// roll-up monitors real money; pass `?real=false` to include paper.
    #[serde(default = "default_real")]
    pub real: bool,
}

fn default_real() -> bool {
    true
}

/// `GET /api/portfolio/positions[?real=true|false]`
///
/// All open **strategy** positions across every rule (the Live-Trading roll-up,
/// ). `real` defaults to true (real-money only).
pub async fn get_portfolio_positions(
    app_state: web::Data<Arc<DeployState>>,
    query: web::Query<PositionsQuery>,
) -> impl Responder {
    match portfolio::open_positions(app_state.get_ref(), query.real).await {
        Ok(positions) => HttpResponse::Ok().json(positions),
        Err(e) => {
            tracing::warn!("get_portfolio_positions failed: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({ "error": e.to_string() }))
        }
    }
}

#[derive(Deserialize)]
pub struct RecentClosedQuery {
    #[serde(default = "default_recent_limit")]
    pub limit: i64,
}

fn default_recent_limit() -> i64 {
    50
}

/// `GET /api/portfolio/recent-closes[?limit=50]`
///
/// Latest `End` / `EntryFailed` rows from `strategy_positions` — hydrates Floor
/// Recent so closes survive refresh / missed SSE.
pub async fn get_portfolio_recent_closes(
    app_state: web::Data<Arc<DeployState>>,
    query: web::Query<RecentClosedQuery>,
) -> impl Responder {
    match portfolio::recent_closed(app_state.get_ref(), query.limit).await {
        Ok(positions) => HttpResponse::Ok().json(positions),
        Err(e) => {
            tracing::warn!("get_portfolio_recent_closes failed: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({ "error": e.to_string() }))
        }
    }
}

#[derive(Deserialize)]
pub struct PerformanceQuery {
    #[serde(default = "default_perf_range")]
    pub range: String,
    /// Custom window bounds (UTC, `to` exclusive) — read only for `range=custom`.
    #[serde(default)]
    pub from: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub to: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default = "default_perf_mode")]
    pub mode: String,
}

fn default_perf_range() -> String {
    "today".into()
}
fn default_perf_mode() -> String {
    "real".into()
}

/// `GET /api/portfolio/performance?range=today|7d|30d|all|custom[&from=&to=]&mode=real|paper`
///
/// Cross-rule closed-trade rollup for the Portfolio page.
pub async fn get_portfolio_performance(
    app_state: web::Data<Arc<DeployState>>,
    query: web::Query<PerformanceQuery>,
) -> impl Responder {
    match portfolio::performance(
        app_state.get_ref(),
        &query.range,
        query.from,
        query.to,
        &query.mode,
    )
    .await
    {
        Ok(body) => HttpResponse::Ok().json(body),
        Err(e) => {
            tracing::warn!("get_portfolio_performance failed: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({ "error": e.to_string() }))
        }
    }
}

#[derive(Deserialize)]
pub struct ClosesSeriesQuery {
    #[serde(default = "default_perf_range")]
    pub range: String,
    /// Custom window bounds (UTC, `to` exclusive) — read only for `range=custom`.
    #[serde(default)]
    pub from: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub to: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default = "default_perf_mode")]
    pub mode: String,
    /// Restrict to one rule; absent = all rules.
    #[serde(default)]
    pub rule_id: Option<uuid::Uuid>,
}

/// `GET /api/portfolio/closes-series?range=[&from=&to=]&mode=&rule_id=` (B2)
///
/// The per-close array behind every portfolio chart. One fetch feeds the equity
/// curve, the PnL histogram, the calendar/hour heatmap and the per-rule
/// comparison — no per-chart endpoint, so no aggregation drift between them.
pub async fn get_portfolio_closes_series(
    app_state: web::Data<Arc<DeployState>>,
    query: web::Query<ClosesSeriesQuery>,
) -> impl Responder {
    match portfolio::closes_series(
        app_state.get_ref(),
        &query.range,
        query.from,
        query.to,
        &query.mode,
        query.rule_id,
    )
    .await
    {
        Ok(body) => HttpResponse::Ok().json(body),
        Err(e) => {
            tracing::warn!("get_portfolio_closes_series failed: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({ "error": e.to_string() }))
        }
    }
}

/// `POST /api/portfolio/positions/query` (B1)
///
/// One page of positions across **all rules and runs** — the Console History
/// table. Shares the [`TableRequest`] contract (and the repo SQL) with the
/// per-rule positions read; the cohort narrows through the body's filters
/// (`mode` / `rule_id` / `status` / `exit_reason`) and `range` window.
pub async fn query_portfolio_positions(
    app_state: web::Data<Arc<DeployState>>,
    body: web::Json<TableRequest>,
) -> impl Responder {
    trading_core::api::handlers::strategies::rule_positions::portfolio_positions_page(
        &app_state.strategy_repo,
        body.into_inner(),
    )
    .await
}

/// `POST /api/portfolio/positions/summary`
///
/// The aggregate twin of [`query_portfolio_positions`] — same body, same cohort,
/// so the Console History summary strip can't disagree with the table under it.
/// Open positions are marked to market from the live in-memory token cache (no DB
/// or RPC round-trip), exactly as the per-rule summary does.
pub async fn query_portfolio_positions_summary(
    app_state: web::Data<Arc<DeployState>>,
    body: web::Json<TableRequest>,
) -> impl Responder {
    let token_cache = app_state.token_cache.clone();
    let mark_of = |mint: &str| trading_core::state::token_cache::mark_quote(&token_cache, mint);
    trading_core::api::handlers::strategies::rule_positions::portfolio_positions_summary(
        &app_state.strategy_repo,
        body.into_inner(),
        mark_of,
    )
    .await
}
