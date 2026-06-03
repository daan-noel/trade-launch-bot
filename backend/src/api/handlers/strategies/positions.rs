use actix_web::{web, HttpResponse, Responder};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    models::Position, state::app_state::AppState,
    storage::repositories::position_repo::PositionRepo,
};

// ---------------------------------------------------------------------------
// Response Types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct PositionResponse {
    pub id: Uuid,
    pub mint: String,
    pub wallet: String,
    pub entry_price: f64,
    pub exit_price: Option<f64>,
    pub entry_tx: String,
    pub exit_tx: Option<String>,
    pub status: String,
    pub strategy: String,
    pub rule_id: Uuid,
    pub entry_amount: f64,
    pub exit_amount: Option<f64>,
    pub pnl_percent: Option<f64>,
    pub entry_time: Option<DateTime<Utc>>,
    pub exit_time: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<Position> for PositionResponse {
    fn from(p: Position) -> Self {
        let pnl_percent = p.pnl_percentage();
        Self {
            id: p.id,
            mint: p.mint,
            wallet: p.wallet,
            entry_price: p.entry_price,
            exit_price: p.exit_price,
            entry_tx: p.entry_tx,
            exit_tx: p.exit_tx,
            status: p.status.to_string(),
            strategy: p.strategy,
            rule_id: p.rule_id,
            entry_amount: p.entry_amount,
            exit_amount: p.exit_amount,
            pnl_percent,
            entry_time: p.entry_time,
            exit_time: p.exit_time,
            created_at: p.created_at,
            updated_at: p.updated_at,
        }
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Get all positions for a specific TPSL rule (by rule_id)
/// GET /api/strategies/tpsl/rules/{rule_id}/positions
pub async fn get_positions_by_rule(
    app_state: web::Data<Arc<AppState>>,
    rule_id: web::Path<Uuid>,
) -> impl Responder {
    let repo = PositionRepo::new(app_state.db.clone());
    let rule_id = rule_id.into_inner();
    match repo.find_by_rule(rule_id).await {
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

/// List all positions
pub async fn list_positions(app_state: web::Data<Arc<AppState>>) -> impl Responder {
    let repo = PositionRepo::new(app_state.db.clone());

    match repo.find_by_strategy("TPSL").await {
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

/// Get positions by mint (token)
pub async fn get_positions_by_mint(
    app_state: web::Data<Arc<AppState>>,
    mint: web::Path<String>,
) -> impl Responder {
    let repo = PositionRepo::new(app_state.db.clone());
    let mint = mint.into_inner();

    match repo.find_holding_by_mint(&mint).await {
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

/// Get positions by wallet
pub async fn get_positions_by_wallet(
    app_state: web::Data<Arc<AppState>>,
    wallet: web::Path<String>,
) -> impl Responder {
    let repo = PositionRepo::new(app_state.db.clone());
    let wallet = wallet.into_inner();

    match repo.find_holding_by_wallet(&wallet).await {
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

/// Get a specific position
pub async fn get_position(
    app_state: web::Data<Arc<AppState>>,
    position_id: web::Path<Uuid>,
) -> impl Responder {
    let repo = PositionRepo::new(app_state.db.clone());
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
