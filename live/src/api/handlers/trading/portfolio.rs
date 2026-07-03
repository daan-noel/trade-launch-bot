//! Portfolio/PnL read endpoints (Phase 1.5) — the `/api/portfolio/*` surface the
//! Holdings, Home, and Live-Trading pages read. Thin handlers over
//! [`crate::services::portfolio`] (the composition SSOT); no logic lives here.

use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use serde::Deserialize;

use crate::services::portfolio;
use crate::state::deploy_state::DeployState;

/// `GET /api/portfolio/holdings`
///
/// Enriched wallet holdings + cost basis + unrealized PnL + bot-managed tag —
/// backs the Holdings page and the Home top-holdings widget.
pub async fn get_portfolio_holdings(app_state: web::Data<Arc<DeployState>>) -> impl Responder {
    match portfolio::list_holdings(app_state.get_ref()).await {
        Ok(holdings) => HttpResponse::Ok().json(holdings),
        Err(e) => {
            tracing::warn!("get_portfolio_holdings failed: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({ "error": e.to_string() }))
        }
    }
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
