use actix_web::{web, HttpResponse, Responder};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    models::Position,
    state::deploy_state::DeployState,
    storage::repositories::{
        tpsl2_paper_trading_repo::Tpsl2PaperTradingRepo, tpsl2_position_repo::Tpsl2PositionRepo,
        tpsl2_strategy_rule_repo::Tpsl2StrategyRuleRepo,
    },
};

// ---------------------------------------------------------------------------
// Response Types
// ---------------------------------------------------------------------------

// `PositionResponse` (this tpsl2 shape, with the `target_*` snapshot) moved to
// `backend-core::models::position` so the core SSE render bridge can emit it;
// re-exported so existing `tpsl2_positions::PositionResponse` paths resolve.
pub use crate::models::position::PositionResponse;

/// Query params for the position list views. Bounds every list query so a
/// growing `tpsl2_real_positions` table can't be fetched whole in one request.
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
// Handlers
// ---------------------------------------------------------------------------

/// Load a rule's positions from the correct tpsl2 table.
///
/// Paper-mode rules record to `tpsl2_paper_positions` (only the latest run is
/// retained), so they're served from the paper repo's current run; real rules
/// use `tpsl2_real_positions`. A paper rule with no run yet yields an empty list.
pub(crate) async fn load_rule_positions(
    db: &PgPool,
    rule_id: Uuid,
    limit: i64,
    offset: i64,
) -> anyhow::Result<Vec<Position>> {
    let is_paper = match Tpsl2StrategyRuleRepo::new(db.clone()).find_by_id(rule_id).await? {
        Some(rule) => rule.trade_mode == "paper",
        None => false,
    };

    if is_paper {
        let paper_repo = Tpsl2PaperTradingRepo::new(db.clone());
        match paper_repo.current_run(rule_id).await? {
            Some(run) => paper_repo.find_by_run(run.id, limit, offset).await,
            None => Ok(Vec::new()),
        }
    } else {
        Tpsl2PositionRepo::new(db.clone())
            .find_by_rule(rule_id, limit, offset)
            .await
    }
}

/// Get all positions for a specific TPSL2 rule (by rule_id).
/// GET /api/strategies/tpsl2/rules/{rule_id}/positions
pub async fn get_positions_by_rule(
    app_state: web::Data<Arc<DeployState>>,
    rule_id: web::Path<Uuid>,
    query: web::Query<PositionListParams>,
) -> impl Responder {
    let rule_id = rule_id.into_inner();
    let (limit, offset) = query.bounds();
    match load_rule_positions(&app_state.db, rule_id, limit, offset).await {
        Ok(positions) => {
            let responses: Vec<PositionResponse> =
                positions.into_iter().map(PositionResponse::from).collect();
            HttpResponse::Ok().json(responses)
        }
        Err(e) => {
            tracing::error!("Failed to get positions for rule {rule_id}: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "Failed to get positions"}))
        }
    }
}

/// List all tpsl2 positions.
/// GET /api/strategies/tpsl2/positions
pub async fn list_positions(
    app_state: web::Data<Arc<DeployState>>,
    query: web::Query<PositionListParams>,
) -> impl Responder {
    let repo = app_state.tpsl2_position_repo();
    let (limit, offset) = query.bounds();

    match repo.find_by_strategy("TPSL2", limit, offset).await {
        Ok(positions) => {
            let responses: Vec<PositionResponse> =
                positions.into_iter().map(PositionResponse::from).collect();
            HttpResponse::Ok().json(responses)
        }
        Err(e) => {
            tracing::error!("Failed to list positions: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "Failed to list positions"}))
        }
    }
}

/// Get tpsl2 positions by mint (token).
/// GET /api/strategies/tpsl2/positions/mint/{mint}
pub async fn get_positions_by_mint(
    app_state: web::Data<Arc<DeployState>>,
    mint: web::Path<String>,
    query: web::Query<PositionListParams>,
) -> impl Responder {
    let repo = app_state.tpsl2_position_repo();
    let mint = mint.into_inner();
    let (limit, offset) = query.bounds();

    match repo.find_holding_by_mint(&mint, limit, offset).await {
        Ok(positions) => {
            let responses: Vec<PositionResponse> =
                positions.into_iter().map(PositionResponse::from).collect();
            HttpResponse::Ok().json(responses)
        }
        Err(e) => {
            tracing::error!("Failed to get positions for mint {mint}: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "Failed to get positions"}))
        }
    }
}

/// Get tpsl2 positions by wallet.
/// GET /api/strategies/tpsl2/positions/wallet/{wallet}
pub async fn get_positions_by_wallet(
    app_state: web::Data<Arc<DeployState>>,
    wallet: web::Path<String>,
    query: web::Query<PositionListParams>,
) -> impl Responder {
    let repo = app_state.tpsl2_position_repo();
    let wallet = wallet.into_inner();
    let (limit, offset) = query.bounds();

    match repo.find_holding_by_wallet(&wallet, limit, offset).await {
        Ok(positions) => {
            let responses: Vec<PositionResponse> =
                positions.into_iter().map(PositionResponse::from).collect();
            HttpResponse::Ok().json(responses)
        }
        Err(e) => {
            tracing::error!("Failed to get positions for wallet {wallet}: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "Failed to get positions"}))
        }
    }
}

/// Get a specific tpsl2 position.
/// GET /api/strategies/tpsl2/positions/{position_id}
pub async fn get_position(
    app_state: web::Data<Arc<DeployState>>,
    position_id: web::Path<Uuid>,
) -> impl Responder {
    let repo = app_state.tpsl2_position_repo();
    let position_id = position_id.into_inner();

    match repo.find_by_id(position_id).await {
        Ok(Some(position)) => HttpResponse::Ok().json(PositionResponse::from(position)),
        Ok(None) => {
            HttpResponse::NotFound().json(serde_json::json!({"error": "Position not found"}))
        }
        Err(e) => {
            tracing::error!("Failed to get position {position_id}: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "Failed to get position"}))
        }
    }
}
