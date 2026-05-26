use actix_web::{web, HttpResponse, Responder};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    models::{Position, StrategyTPSLRule},
    state::app_state::AppState,
    storage::repositories::{
        position_repo::PositionRepo,
        strategy_tpsl_rule_repo::StrategyTPSLRuleRepo,
    },
};

// ---------------------------------------------------------------------------
// Response Types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct RuleResponse {
    pub id: Uuid,
    pub rule_name: String,
    pub p_initial_buy_sol: Option<f64>,
    pub p_cu_limit: Option<u64>,
    pub p_cu_price: Option<u64>,
    pub p_max_sol_cost: Option<f64>,
    pub p_spendable_sol_in: Option<f64>,
    pub p_ix_labels: serde_json::Value,
    pub buy_amount: f64,
    pub take_profit: f64,
    pub stop_loss: f64,
    pub tolerance_pct: f64,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<StrategyTPSLRule> for RuleResponse {
    fn from(r: StrategyTPSLRule) -> Self {
        Self {
            id: r.id,
            rule_name: r.rule_name,
            p_initial_buy_sol: r.p_initial_buy_sol,
            p_cu_limit: r.p_cu_limit,
            p_cu_price: r.p_cu_price,
            p_max_sol_cost: r.p_max_sol_cost,
            p_spendable_sol_in: r.p_spendable_sol_in,
            p_ix_labels: r.p_ix_labels,
            buy_amount: r.buy_amount,
            take_profit: r.take_profit,
            stop_loss: r.stop_loss,
            tolerance_pct: r.tolerance_pct,
            is_active: r.is_active,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

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
            created_at: p.created_at,
            updated_at: p.updated_at,
        }
    }
}

#[derive(Deserialize)]
pub struct CreateRuleRequest {
    pub rule_name: String,
    pub p_initial_buy_sol: Option<f64>,
    pub p_cu_limit: Option<u64>,
    pub p_cu_price: Option<u64>,
    pub p_max_sol_cost: Option<f64>,
    pub p_spendable_sol_in: Option<f64>,
    pub p_ix_labels: serde_json::Value,
    pub buy_amount: f64,
    pub take_profit: f64,
    pub stop_loss: f64,
    pub tolerance_pct: Option<f64>,
}

#[derive(Deserialize)]
pub struct UpdateRuleRequest {
    pub rule_name: Option<String>,
    pub buy_amount: Option<f64>,
    pub take_profit: Option<f64>,
    pub stop_loss: Option<f64>,
    pub p_max_sol_cost: Option<f64>,
    pub p_spendable_sol_in: Option<f64>,
    pub tolerance_pct: Option<f64>,
    pub is_active: Option<bool>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// List all TPSL rules
pub async fn list_tpsl_rules(app_state: web::Data<Arc<AppState>>) -> impl Responder {
    let repo = StrategyTPSLRuleRepo::new(app_state.db.clone());

    match repo.find_all().await {
        Ok(rules) => {
            let responses: Vec<RuleResponse> =
                rules.into_iter().map(RuleResponse::from).collect();
            HttpResponse::Ok().json(responses)
        }
        Err(e) => {
            tracing::error!("Failed to list TPSL rules: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "Failed to list rules"}))
        }
    }
}

/// Get a specific TPSL rule
pub async fn get_tpsl_rule(
    app_state: web::Data<Arc<AppState>>,
    rule_id: web::Path<Uuid>,
) -> impl Responder {
    let repo = StrategyTPSLRuleRepo::new(app_state.db.clone());
    let rule_id = rule_id.into_inner();

    match repo.find_by_id(rule_id).await {
        Ok(Some(rule)) => HttpResponse::Ok().json(RuleResponse::from(rule)),
        Ok(None) => HttpResponse::NotFound()
            .json(serde_json::json!({"error": "Rule not found"})),
        Err(e) => {
            tracing::error!("Failed to get TPSL rule {rule_id}: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "Failed to get rule"}))
        }
    }
}

/// Create a new TPSL rule
pub async fn create_tpsl_rule(
    app_state: web::Data<Arc<AppState>>,
    req: web::Json<CreateRuleRequest>,
) -> impl Responder {
    let rule = StrategyTPSLRule::new(
        req.rule_name.clone(),
        req.p_initial_buy_sol,
        req.p_cu_limit,
        req.p_cu_price,
        req.p_ix_labels.clone(),
        req.buy_amount,
        req.take_profit,
        req.stop_loss,
        req.p_max_sol_cost,
        req.p_spendable_sol_in,
        req.tolerance_pct,
    );

    let repo = StrategyTPSLRuleRepo::new(app_state.db.clone());

    match repo.insert(&rule).await {
        Ok(_) => HttpResponse::Created().json(RuleResponse::from(rule)),
        Err(e) => {
            tracing::error!("Failed to create TPSL rule: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "Failed to create rule"}))
        }
    }
}

/// Update an existing TPSL rule
pub async fn update_tpsl_rule(
    app_state: web::Data<Arc<AppState>>,
    rule_id: web::Path<Uuid>,
    req: web::Json<UpdateRuleRequest>,
) -> impl Responder {
    let rule_id = rule_id.into_inner();
    let repo = StrategyTPSLRuleRepo::new(app_state.db.clone());

    match repo.find_by_id(rule_id).await {
        Ok(Some(mut rule)) => {
            // Update fields if provided
            if let Some(name) = &req.rule_name {
                rule.rule_name = name.clone();
            }
            if let Some(buy_amount) = req.buy_amount {
                rule.buy_amount = buy_amount;
            }
            if let Some(take_profit) = req.take_profit {
                rule.take_profit = take_profit;
            }
            if let Some(stop_loss) = req.stop_loss {
                rule.stop_loss = stop_loss;
            }
            if let Some(max_sol_cost) = req.p_max_sol_cost {
                rule.p_max_sol_cost = Some(max_sol_cost);
            }
            if let Some(spendable_sol_in) = req.p_spendable_sol_in {
                rule.p_spendable_sol_in = Some(spendable_sol_in);
            }
            if let Some(tolerance_pct) = req.tolerance_pct {
                rule.tolerance_pct = tolerance_pct;
            }
            if let Some(is_active) = req.is_active {
                rule.is_active = is_active;
            }

            match repo.update(&rule).await {
                Ok(_) => HttpResponse::Ok().json(RuleResponse::from(rule)),
                Err(e) => {
                    tracing::error!("Failed to update TPSL rule {rule_id}: {e}");
                    HttpResponse::InternalServerError()
                        .json(serde_json::json!({"error": "Failed to update rule"}))
                }
            }
        }
        Ok(None) => HttpResponse::NotFound()
            .json(serde_json::json!({"error": "Rule not found"})),
        Err(e) => {
            tracing::error!("Failed to get TPSL rule {rule_id}: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "Failed to get rule"}))
        }
    }
}

/// Delete a TPSL rule
pub async fn delete_tpsl_rule(
    app_state: web::Data<Arc<AppState>>,
    rule_id: web::Path<Uuid>,
) -> impl Responder {
    let rule_id = rule_id.into_inner();
    let repo = StrategyTPSLRuleRepo::new(app_state.db.clone());

    match repo.delete(rule_id).await {
        Ok(_) => HttpResponse::NoContent().finish(),
        Err(e) => {
            tracing::error!("Failed to delete TPSL rule {rule_id}: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "Failed to delete rule"}))
        }
    }
}

// ---------------------------------------------------------------------------
// Position Handlers
// ---------------------------------------------------------------------------

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
        Ok(None) => HttpResponse::NotFound()
            .json(serde_json::json!({"error": "Position not found"})),
        Err(e) => {
            tracing::error!("Failed to get position {position_id}: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "Failed to get position"}))
        }
    }
}

// ---------------------------------------------------------------------------
// Simulation
// ---------------------------------------------------------------------------

// ── Handler ──────────────────────────────────────────────────────────────────

/// Simulate a TPSL rule against all historically matched tokens.
///
/// GET /api/strategies/tpsl/rules/{rule_id}/simulate
pub async fn simulate_tpsl_rule(
    app_state: web::Data<Arc<AppState>>,
    rule_id: web::Path<Uuid>,
) -> impl Responder {
    // delegate to strategies/tpsl simulation module (returns domain summary)
    let rid = rule_id.into_inner();
    match crate::strategies::tpsl::run_simulation(app_state.clone(), rid).await {
        Ok(summary) => HttpResponse::Ok().json(summary),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("Rule not found") {
                HttpResponse::NotFound().json(serde_json::json!({"error": msg}))
            } else if msg.contains("All rule criteria are empty") {
                HttpResponse::BadRequest().json(serde_json::json!({"error": msg}))
            } else {
                tracing::error!("Simulation failed: {e}");
                HttpResponse::InternalServerError().json(serde_json::json!({"error": msg}))
            }
        }
    }
}
