use actix_web::{web, HttpResponse, Responder};
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    models::Position,
    state::app_state::AppState,
    storage::repositories::{
        tpsl2_paper_trading_repo::Tpsl2PaperTradingRepo, tpsl2_position_repo::Tpsl2PositionRepo,
        tpsl2_strategy_rule_repo::Tpsl2StrategyRuleRepo,
    },
};

// ---------------------------------------------------------------------------
// Response Types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct PositionResponse {
    pub id: Uuid,
    pub mint: String,
    pub wallet: String,
    /// Target (trigger-trade) snapshot — the scalp-entry signal trade that armed
    /// this position, distinct from the actual entry fill. `None` until armed.
    /// The gap vs. the `entry_*` columns is derived client-side, not stored.
    pub target_price: Option<f64>,
    pub target_token_amount: Option<f64>,
    pub target_time: Option<DateTime<Utc>>,
    pub target_tx: Option<String>,
    pub entry_price: Option<f64>,
    pub entry_token_amount: Option<f64>,
    pub entry_time: Option<DateTime<Utc>>,
    /// First entry leg's signature (display/back-compat); empty until adopted.
    pub entry_tx: String,
    pub exit_price: Option<f64>,
    pub exit_token_amount: Option<f64>,
    pub exit_time: Option<DateTime<Utc>>,
    /// Last exit leg's signature (display/back-compat); `None` until a sell lands.
    pub exit_tx: Option<String>,
    /// All signatures that made up the entry fill (per-signature attribution, 1C).
    pub entry_tx_signatures: Vec<String>,
    /// All signatures that made up the exit fill (multi-leg: retries / re-routes).
    pub exit_tx_signatures: Vec<String>,
    pub pnl_percent: Option<f64>,
    pub status: String,
    pub strategy: String,
    pub rule_id: Uuid,
    /// Why the position exited ("TakeProfit", "StopLoss", "TrailingStop",
    /// "Stall", "TimeStop", "LiquidityExit"); `None` while still open.
    pub exit_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<Position> for PositionResponse {
    fn from(p: Position) -> Self {
        let pnl_percent = p.pnl_percentage();
        let exit_reason = p.exit_reason_or_derived();
        Self {
            id: p.id,
            mint: p.mint,
            wallet: p.wallet,
            target_price: p.target_price,
            target_token_amount: p.target_token_amount,
            target_time: p.target_time,
            target_tx: p.target_tx,
            entry_price: p.entry_price,
            entry_token_amount: p.entry_token_amount,
            entry_time: p.entry_time,
            // First entry leg / last exit leg for the single-address display columns.
            entry_tx: p.entry_tx_signatures.first().cloned().unwrap_or_default(),
            exit_price: p.exit_price,
            exit_token_amount: p.exit_token_amount,
            exit_time: p.exit_time,
            exit_tx: p.exit_tx_signatures.last().cloned(),
            entry_tx_signatures: p.entry_tx_signatures,
            exit_tx_signatures: p.exit_tx_signatures,
            pnl_percent,
            status: p.status.to_string(),
            strategy: p.strategy,
            rule_id: p.rule_id,
            exit_reason,
            created_at: p.created_at,
            updated_at: p.updated_at,
        }
    }
}

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
    app_state: web::Data<Arc<AppState>>,
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
    app_state: web::Data<Arc<AppState>>,
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
    app_state: web::Data<Arc<AppState>>,
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
    app_state: web::Data<Arc<AppState>>,
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
    app_state: web::Data<Arc<AppState>>,
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
