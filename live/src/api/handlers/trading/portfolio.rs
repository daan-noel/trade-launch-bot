//! Portfolio/PnL read endpoints (Phase 1.5) — the `/api/portfolio/*` surface the
//! Holdings, Home, and Live-Trading pages read. Thin handlers over
//! [`crate::services::portfolio`] (the composition SSOT); no logic lives here.

use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use serde::Deserialize;
use serde_json::Value;

use trading_core::api::table_eval::{apply_table_request, resolve_token_enrichment_key, ColKind};
use trading_core::api::table_query::{Page, TableRequest};

use crate::services::portfolio::{self, HoldingsTableSummary};
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
fn holdings_to_values(holdings: &[portfolio::PortfolioHolding]) -> Vec<Value> {
    holdings
        .iter()
        .map(|h| serde_json::to_value(h).unwrap_or(Value::Null))
        .collect()
}

/// `GET /api/portfolio/holdings`
///
/// Full enriched wallet holdings (unpaged) — backs the Home top-holdings widget +
/// live-trade feed. The Holdings **page** uses the paged POST below; both share the
/// short-TTL scan cache.
pub async fn get_portfolio_holdings(app_state: web::Data<Arc<DeployState>>) -> impl Responder {
    match portfolio::list_holdings_cached(app_state.get_ref(), false).await {
        Ok(holdings) => HttpResponse::Ok().json(holdings.as_ref()),
        Err(e) => {
            tracing::warn!("get_portfolio_holdings failed: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({ "error": e.to_string() }))
        }
    }
}

/// Query params for the paged Holdings reads. `fresh=1` busts the scan cache before
/// serving — the Wallet page sends it once after a confirmed trade so the reload
/// reflects the new on-chain balance immediately (normal paging/sort/filter omit it
/// and reuse the warm scan).
#[derive(Deserialize)]
pub struct HoldingsQueryParams {
    #[serde(default)]
    pub fresh: bool,
}

/// `POST /api/portfolio/holdings/query[?fresh=1]`
///
/// One page of the wallet holdings under the unified [`TableRequest`] contract
/// (server-side search/sort/filter/paging), mirroring the positions/matched POST.
/// Runs the in-memory [`apply_table_request`] evaluator over the composed holdings
/// (bounded — tens of tokens) with [`holdings_resolve`]; the full match count rides
/// `X-Total-Count`.
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
    let values = holdings_to_values(&holdings);
    let (page, total) = apply_table_request(&values, &req, holdings_resolve);
    HttpResponse::Ok()
        .insert_header(("X-Total-Count", total.to_string()))
        .insert_header(("Access-Control-Expose-Headers", "X-Total-Count"))
        .json(page)
}

/// `POST /api/portfolio/holdings/summary`
///
/// Whole-population roll-up (value/cost/PnL/24h) over the **filtered** holdings for
/// the page's summary bar — same request body as the table so the two agree, but
/// measured over every matching row (not one page). Bounded, so we simply evaluate
/// the filters with an all-encompassing page and sum the result.
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
    let values = holdings_to_values(&holdings);
    let (rows, _) = apply_table_request(&values, &req, holdings_resolve);

    let num = |row: &Value, k: &str| row.get(k).and_then(Value::as_f64);
    let mut s = HoldingsTableSummary { positions: rows.len(), ..Default::default() };
    let (mut value_sol, mut value_usd, mut pnl, mut has_value, mut has_pnl) =
        (0.0, 0.0, 0.0, false, false);
    let (mut wchange, mut wweight) = (0.0, 0.0);
    for row in &rows {
        if let Some(v) = num(row, "value_usd") {
            value_usd += v;
            has_value = true;
            if let Some(c) = num(row, "price_change_24h") {
                wchange += v * c;
                wweight += v;
            }
        }
        if let Some(v) = num(row, "value_sol") {
            value_sol += v;
        }
        if let Some(v) = num(row, "cost_basis_sol") {
            s.total_cost_basis_sol += v;
        }
        if let Some(v) = num(row, "unrealized_pnl_sol") {
            pnl += v;
            has_pnl = true;
        }
    }
    s.total_value_usd = has_value.then_some(value_usd);
    s.total_value_sol = has_value.then_some(value_sol);
    s.total_unrealized_pnl_sol = has_pnl.then_some(pnl);
    s.change_24h_pct = (wweight > 0.0).then(|| wchange / wweight);
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
/// Phase 4). `real` defaults to true (real-money only).
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
