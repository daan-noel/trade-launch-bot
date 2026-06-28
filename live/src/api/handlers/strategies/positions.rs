//! Unified strategy position-read handlers.
//!
//! One set of handlers over [`StrategyRepo`] (the unified `strategy_positions`
//! table), keyed by a `{strategy}` path segment. The legacy per-strategy URLs
//! (`/strategies/tpsl1/...`, `/strategies/tpsl2/...`) stay valid — the segment
//! maps to a canonical `strategy_id` via [`StrategyImpl::from_id`], which accepts
//! both the short alias (`tpsl1`) and the canonical id (`tpsl_sniper_1`).

use actix_web::{web, HttpResponse, Responder};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

use crate::state::deploy_state::DeployState;
use trading_core::models::StrategyPosition;
use trading_core::storage::repositories::strategy_repo::StrategyRepo;
use trading_core::strategies::registry::StrategyImpl;

// ---------------------------------------------------------------------------
// Response type
// ---------------------------------------------------------------------------

/// Wire shape for a position read. Field set is kept stable for the frontend; the
/// JSONB signature arrays are decoded to `Vec<String>` and the single-address
/// display columns are the first entry leg / last exit leg.
#[derive(Serialize)]
pub struct PositionResponse {
    pub id: Uuid,
    pub run_id: Uuid,
    pub mint: String,
    pub wallet: String,
    pub entry_price: Option<f64>,
    pub exit_price: Option<f64>,
    /// First entry leg's signature (display/back-compat); empty until the fill is
    /// adopted. The full per-leg list is `entry_tx_signatures`.
    pub entry_tx: String,
    /// Last exit leg's signature (display/back-compat); `None` until a sell lands.
    /// The full per-leg list is `exit_tx_signatures`.
    pub exit_tx: Option<String>,
    pub entry_tx_signatures: Vec<String>,
    pub exit_tx_signatures: Vec<String>,
    pub status: String,
    pub strategy: String,
    /// Owning rule (`None` if the rule was deleted — `ON DELETE SET NULL`).
    pub rule_id: Option<Uuid>,
    pub entry_token_amount: Option<f64>,
    pub exit_token_amount: Option<f64>,
    pub pnl_percent: Option<f64>,
    pub entry_time: Option<DateTime<Utc>>,
    pub exit_time: Option<DateTime<Utc>>,
    pub exit_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<StrategyPosition> for PositionResponse {
    fn from(p: StrategyPosition) -> Self {
        let pnl_percent = p.pnl_pct();
        let entry_sigs = p.entry_tx_sigs();
        let exit_sigs = p.exit_tx_sigs();
        Self {
            id: p.id,
            run_id: p.run_id,
            mint: p.mint,
            wallet: p.wallet,
            entry_price: p.entry_price,
            exit_price: p.exit_price,
            entry_tx: entry_sigs.first().cloned().unwrap_or_default(),
            exit_tx: exit_sigs.last().cloned(),
            entry_tx_signatures: entry_sigs,
            exit_tx_signatures: exit_sigs,
            status: p.status,
            strategy: p.strategy_id,
            rule_id: p.rule_id,
            entry_token_amount: p.entry_token_amount,
            exit_token_amount: p.exit_token_amount,
            pnl_percent,
            entry_time: p.entry_time,
            exit_time: p.exit_time,
            exit_reason: p.exit_reason,
            created_at: p.created_at,
            updated_at: p.updated_at,
        }
    }
}

/// Query params for the list views. Bounds every list query so a growing
/// `strategy_positions` table can't be fetched whole in one request.
#[derive(serde::Deserialize)]
pub struct PositionListParams {
    #[serde(default = "default_positions_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_positions_limit() -> i64 {
    200
}

impl PositionListParams {
    /// Clamp to a sane window: limit in 1..=1000, offset >= 0.
    fn bounds(&self) -> (i64, i64) {
        (self.limit.clamp(1, 1000), self.offset.max(0))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Resolve the `{strategy}` path segment to its canonical `strategy_id`, or a
/// `400` if it names no known strategy.
fn strategy_id(seg: &str) -> Result<&'static str, HttpResponse> {
    StrategyImpl::from_id(seg).map(|s| s.id()).ok_or_else(|| {
        HttpResponse::BadRequest().json(serde_json::json!({"error": "Unknown strategy"}))
    })
}

fn repo(app_state: &DeployState) -> &StrategyRepo {
    app_state.strategy.repo()
}

fn json_positions(positions: Vec<StrategyPosition>) -> HttpResponse {
    let responses: Vec<PositionResponse> =
        positions.into_iter().map(PositionResponse::from).collect();
    HttpResponse::Ok().json(responses)
}

fn list_error(what: &str, e: anyhow::Error) -> HttpResponse {
    tracing::error!("Failed to {what}: {e}");
    HttpResponse::InternalServerError().json(serde_json::json!({"error": "Failed to load positions"}))
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/strategies/{strategy}/rules/{rule_id}/positions
///
/// Paper rules retain only the current run's bag, so they're served from the
/// rule's latest paper run; real rules carry their full lifetime history.
pub async fn get_positions_by_rule(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<(String, Uuid)>,
    query: web::Query<PositionListParams>,
) -> impl Responder {
    let (strategy, rule_id) = path.into_inner();
    if let Err(resp) = strategy_id(&strategy) {
        return resp;
    }
    let (limit, offset) = query.bounds();
    let repo = repo(&app_state);

    let rule = match repo.find_rule(rule_id).await {
        Ok(Some(rule)) => rule,
        Ok(None) => return json_positions(Vec::new()),
        Err(e) => return list_error("load rule", e),
    };

    let result = if rule.trade_mode == "paper" {
        match repo.latest_run(rule_id, "paper").await {
            Ok(Some(run)) => repo.find_positions_by_run_paged(run.id, limit, offset).await,
            Ok(None) => Ok(Vec::new()),
            Err(e) => return list_error("load paper run", e),
        }
    } else {
        repo.find_positions_by_rule_paged(rule_id, limit, offset).await
    };

    match result {
        Ok(positions) => json_positions(positions),
        Err(e) => list_error("load positions for rule", e),
    }
}

/// GET /api/strategies/{strategy}/positions
pub async fn list_positions(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<String>,
    query: web::Query<PositionListParams>,
) -> impl Responder {
    let strategy = path.into_inner();
    let sid = match strategy_id(&strategy) {
        Ok(sid) => sid,
        Err(resp) => return resp,
    };
    let (limit, offset) = query.bounds();
    match repo(&app_state).find_positions_by_strategy(sid, limit, offset).await {
        Ok(positions) => json_positions(positions),
        Err(e) => list_error("list positions", e),
    }
}

/// GET /api/strategies/{strategy}/positions/mint/{mint}
pub async fn get_positions_by_mint(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<(String, String)>,
    query: web::Query<PositionListParams>,
) -> impl Responder {
    let (strategy, mint) = path.into_inner();
    let sid = match strategy_id(&strategy) {
        Ok(sid) => sid,
        Err(resp) => return resp,
    };
    let (limit, offset) = query.bounds();
    match repo(&app_state).find_holding_by_mint(sid, &mint, limit, offset).await {
        Ok(positions) => json_positions(positions),
        Err(e) => list_error("load positions for mint", e),
    }
}

/// GET /api/strategies/{strategy}/positions/wallet/{wallet}
pub async fn get_positions_by_wallet(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<(String, String)>,
    query: web::Query<PositionListParams>,
) -> impl Responder {
    let (strategy, wallet) = path.into_inner();
    let sid = match strategy_id(&strategy) {
        Ok(sid) => sid,
        Err(resp) => return resp,
    };
    let (limit, offset) = query.bounds();
    match repo(&app_state).find_holding_by_wallet(sid, &wallet, limit, offset).await {
        Ok(positions) => json_positions(positions),
        Err(e) => list_error("load positions for wallet", e),
    }
}

/// GET /api/strategies/{strategy}/positions/{position_id}
pub async fn get_position(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<(String, Uuid)>,
) -> impl Responder {
    let (strategy, position_id) = path.into_inner();
    if let Err(resp) = strategy_id(&strategy) {
        return resp;
    }
    match repo(&app_state).find_position(position_id).await {
        Ok(Some(position)) => HttpResponse::Ok().json(PositionResponse::from(position)),
        Ok(None) => HttpResponse::NotFound().json(serde_json::json!({"error": "Position not found"})),
        Err(e) => {
            tracing::error!("Failed to get position {position_id}: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "Failed to get position"}))
        }
    }
}
