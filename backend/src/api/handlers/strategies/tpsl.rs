use actix_web::{web, HttpResponse, Responder};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    models::{PositionStatus, StrategyTPSLRule},
    state::app_state::AppState,
    storage::repositories::{
        paper_trading_repo::PaperTradingRepo, strategy_tpsl_rule_repo::StrategyTPSLRuleRepo,
        token_repo::TokenRepo,
    },
    strategies::tpsl_sniper_1::{
        handler_tpsl::token_matches_rule, simulation_tpsl::SimulatedTokenResult,
    },
};

// ---------------------------------------------------------------------------
// Response / Request Types
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
    pub p_max_concurrent_tokens: Option<u64>,
    pub p_max_total_tokens: Option<u64>,
    pub p_ix_labels: serde_json::Value,
    pub trade_mode: String,
    pub buy_amount: f64,
    pub take_profit: f64,
    pub stop_loss: f64,
    pub p_trailing_stop_pct: Option<f64>,
    pub p_time_stop_secs: Option<u64>,
    pub p_stall_secs: Option<u64>,
    pub p_liquidity_drop_pct: Option<f64>,
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
            p_max_concurrent_tokens: r.p_max_concurrent_tokens,
            p_max_total_tokens: r.p_max_total_tokens,
            p_ix_labels: r.p_ix_labels,
            trade_mode: r.trade_mode,
            buy_amount: r.buy_amount,
            take_profit: r.take_profit,
            stop_loss: r.stop_loss,
            p_trailing_stop_pct: r.p_trailing_stop_pct,
            p_time_stop_secs: r.p_time_stop_secs,
            p_stall_secs: r.p_stall_secs,
            p_liquidity_drop_pct: r.p_liquidity_drop_pct,
            tolerance_pct: r.tolerance_pct,
            is_active: r.is_active,
            created_at: r.created_at,
            updated_at: r.updated_at,
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
    pub p_max_concurrent_tokens: Option<u64>,
    pub p_max_total_tokens: Option<u64>,
    pub p_ix_labels: serde_json::Value,
    pub trade_mode: String,
    pub buy_amount: f64,
    pub take_profit: f64,
    pub stop_loss: f64,
    pub p_trailing_stop_pct: Option<f64>,
    pub p_time_stop_secs: Option<u64>,
    pub p_stall_secs: Option<u64>,
    pub p_liquidity_drop_pct: Option<f64>,
    pub tolerance_pct: Option<f64>,
}

#[derive(Deserialize, Serialize)]
pub struct UpdateRuleRequest {
    pub rule_name: Option<String>,
    pub buy_amount: Option<f64>,
    pub take_profit: Option<f64>,
    pub stop_loss: Option<f64>,
    pub p_trailing_stop_pct: Option<f64>,
    pub p_time_stop_secs: Option<u64>,
    pub p_stall_secs: Option<u64>,
    pub p_liquidity_drop_pct: Option<f64>,
    #[serde(default)]
    pub p_initial_buy_sol: Option<Option<f64>>,
    #[serde(default)]
    pub p_cu_limit: Option<Option<u64>>,
    #[serde(default)]
    pub p_cu_price: Option<Option<u64>>,
    #[serde(default)]
    pub p_ix_labels: Option<Option<serde_json::Value>>,
    #[serde(default)]
    pub p_max_sol_cost: Option<Option<f64>>,
    #[serde(default)]
    pub p_spendable_sol_in: Option<Option<f64>>,
    // Outer Option = field present; inner Option = value or explicit null
    #[serde(default)]
    pub p_max_concurrent_tokens: Option<Option<u64>>,
    #[serde(default)]
    pub p_max_total_tokens: Option<Option<u64>>,
    pub tolerance_pct: Option<f64>,
    pub is_active: Option<bool>,
    pub trade_mode: Option<String>,
}

// ---------------------------------------------------------------------------
// Rule Handlers
// ---------------------------------------------------------------------------

/// List all TPSL rules
pub async fn list_tpsl_rules(app_state: web::Data<Arc<AppState>>) -> impl Responder {
    let repo = StrategyTPSLRuleRepo::new(app_state.db.clone());

    match repo.find_all().await {
        Ok(rules) => {
            let responses: Vec<RuleResponse> = rules.into_iter().map(RuleResponse::from).collect();
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
        Ok(None) => HttpResponse::NotFound().json(serde_json::json!({"error": "Rule not found"})),
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
        req.trade_mode.clone(),
        req.buy_amount,
        req.take_profit,
        req.stop_loss,
        req.p_max_sol_cost,
        req.p_spendable_sol_in,
        req.p_max_concurrent_tokens,
        req.p_max_total_tokens,
        req.tolerance_pct,
        req.p_trailing_stop_pct,
        req.p_time_stop_secs,
        req.p_stall_secs,
        req.p_liquidity_drop_pct,
    );

    let repo = StrategyTPSLRuleRepo::new(app_state.db.clone());

    match repo.insert(&rule).await {
        Ok(_) => {
            if let Err(e) = app_state.tpsl_cache.reload_rules(&app_state.db).await {
                tracing::warn!("TPSL rule cache reload after create failed: {e}");
            }
            HttpResponse::Created().json(RuleResponse::from(rule))
        }
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
            // Captured before the update is applied to detect the activate /
            // deactivate transition that begins / ends a paper-test run.
            let was_active = rule.is_active;
            // Log incoming update request for debugging
            match serde_json::to_value(&req.0) {
                Ok(v) => tracing::debug!("UpdateRuleRequest JSON: {}", v),
                Err(e) => tracing::debug!("Failed to serialize UpdateRuleRequest: {e}"),
            }
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
            if let Some(trailing_stop_pct) = req.p_trailing_stop_pct {
                rule.p_trailing_stop_pct = Some(trailing_stop_pct);
            }
            if let Some(time_stop_secs) = req.p_time_stop_secs {
                rule.p_time_stop_secs = Some(time_stop_secs);
            }
            if let Some(stall_secs) = req.p_stall_secs {
                rule.p_stall_secs = Some(stall_secs);
            }
            if let Some(liquidity_drop_pct) = req.p_liquidity_drop_pct {
                rule.p_liquidity_drop_pct = Some(liquidity_drop_pct);
            }
            if let Some(initial_buy_sol_opt) = &req.p_initial_buy_sol {
                rule.p_initial_buy_sol = initial_buy_sol_opt.clone();
            }
            if let Some(cu_limit_opt) = &req.p_cu_limit {
                rule.p_cu_limit = cu_limit_opt.clone();
            }
            if let Some(cu_price_opt) = &req.p_cu_price {
                rule.p_cu_price = cu_price_opt.clone();
            }
            if let Some(ix_labels_opt) = &req.p_ix_labels {
                rule.p_ix_labels = ix_labels_opt
                    .clone()
                    .unwrap_or_else(|| serde_json::Value::Array(vec![]));
            }
            if let Some(max_sol_cost_opt) = &req.p_max_sol_cost {
                rule.p_max_sol_cost = max_sol_cost_opt.clone();
            }
            if let Some(spendable_sol_in_opt) = &req.p_spendable_sol_in {
                rule.p_spendable_sol_in = spendable_sol_in_opt.clone();
            }
            if let Some(max_concurrent_tokens_opt) = &req.p_max_concurrent_tokens {
                rule.p_max_concurrent_tokens = max_concurrent_tokens_opt.clone();
            }
            if let Some(max_total_tokens_opt) = &req.p_max_total_tokens {
                rule.p_max_total_tokens = max_total_tokens_opt.clone();
            }
            if let Some(tolerance_pct) = req.tolerance_pct {
                rule.tolerance_pct = tolerance_pct;
            }
            if let Some(is_active) = req.is_active {
                rule.is_active = is_active;
            }
            if let Some(trade_mode) = &req.trade_mode {
                rule.trade_mode = trade_mode.clone();
            }

            match repo.update(&rule).await {
                Ok(_) => {
                    // Paper-run lifecycle on the active toggle: activating starts a
                    // fresh run (resets per-run caps); deactivating stops the current
                    // one (open positions still drain).
                    if rule.trade_mode == "paper" {
                        if rule.is_active && !was_active {
                            let max_total = rule.p_max_total_tokens.filter(|v| *v > 0);
                            if let Err(e) = app_state
                                .tpsl_cache
                                .start_paper_run(&app_state.db, rule.id, max_total)
                                .await
                            {
                                tracing::warn!("Failed to start paper run for {}: {e}", rule.id);
                            }
                        } else if !rule.is_active && was_active {
                            if let Err(e) = app_state
                                .tpsl_cache
                                .stop_paper_run(&app_state.db, rule.id)
                                .await
                            {
                                tracing::warn!("Failed to stop paper run for {}: {e}", rule.id);
                            }
                        }
                    }
                    if let Err(e) = app_state.tpsl_cache.reload_rules(&app_state.db).await {
                        tracing::warn!("TPSL rule cache reload after update failed: {e}");
                    }
                    HttpResponse::Ok().json(RuleResponse::from(rule))
                }
                Err(e) => {
                    tracing::error!("Failed to update TPSL rule {rule_id}: {e}");
                    HttpResponse::InternalServerError()
                        .json(serde_json::json!({"error": "Failed to update rule"}))
                }
            }
        }
        Ok(None) => HttpResponse::NotFound().json(serde_json::json!({"error": "Rule not found"})),
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
        Ok(_) => {
            if let Err(e) = app_state.tpsl_cache.reload_rules(&app_state.db).await {
                tracing::warn!("TPSL rule cache reload after delete failed: {e}");
            }
            HttpResponse::NoContent().finish()
        }
        Err(e) => {
            tracing::error!("Failed to delete TPSL rule {rule_id}: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "Failed to delete rule"}))
        }
    }
}

// ---------------------------------------------------------------------------
// Matched tokens
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct MatchedTokenResult {
    pub mint: String,
    pub symbol: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub initial_buy_sol: Option<f64>,
    pub cu_limit: Option<u64>,
    pub cu_price: Option<u64>,
}

/// Return all tokens in the database that satisfy a rule's entry criteria.
///
/// GET /api/strategies/tpsl/rules/{rule_id}/matched
pub async fn get_matched_tokens(
    app_state: web::Data<Arc<AppState>>,
    rule_id: web::Path<Uuid>,
) -> impl Responder {
    let rule_id = rule_id.into_inner();
    let rule_repo = StrategyTPSLRuleRepo::new(app_state.db.clone());
    let token_repo = TokenRepo::new(app_state.db.clone());

    let rule = match rule_repo.find_by_id(rule_id).await {
        Ok(Some(r)) => r,
        Ok(None) => {
            return HttpResponse::NotFound()
                .json(serde_json::json!({"error": "Rule not found"}));
        }
        Err(e) => {
            tracing::error!("Failed to get rule {rule_id}: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "Failed to get rule"}));
        }
    };

    let tokens = match token_repo.find_all().await {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("Failed to load tokens for matched check: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "Failed to load tokens"}));
        }
    };

    let matched: Vec<MatchedTokenResult> = tokens
        .into_iter()
        .filter(|t| token_matches_rule(t, &rule))
        .map(|t| MatchedTokenResult {
            mint: t.mint_address,
            symbol: t.symbol,
            name: t.name,
            created_at: t.created_at,
            initial_buy_sol: t.initial_buy_sol,
            cu_limit: t.cu_limit,
            cu_price: t.cu_price,
        })
        .collect();

    HttpResponse::Ok().json(matched)
}

// ---------------------------------------------------------------------------
// Simulation
// ---------------------------------------------------------------------------

/// Simulate a TPSL rule against all historically matched tokens.
///
/// GET /api/strategies/tpsl/rules/{rule_id}/simulate
pub async fn simulate_tpsl_rule(
    app_state: web::Data<Arc<AppState>>,
    rule_id: web::Path<Uuid>,
) -> impl Responder {
    // delegate to the tpsl_sniper_1 simulation module (E1+ exit-walk engine)
    let rid = rule_id.into_inner();
    match crate::strategies::tpsl_sniper_1::run_simulation(app_state.clone(), rid).await {
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

// ---------------------------------------------------------------------------
// Paper-test result
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct PaperRunResponse {
    pub run_seq: i64,
    /// "Running" | "Finished" | "Stopped".
    pub status: String,
    pub max_total_tokens: Option<u64>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
pub struct PaperResultResponse {
    pub rule_name: String,
    /// None when the rule has never been run in paper mode.
    pub run: Option<PaperRunResponse>,
    /// Per-token outcomes for the latest run, shaped like a simulation result so
    /// the frontend renders them through the shared summary card / table.
    pub tokens: Vec<SimulatedTokenResult>,
}

/// Aggregate the latest paper-test run's recorded positions into a
/// simulation-shaped result.
///
/// GET /api/strategies/tpsl/rules/{rule_id}/paper-result
pub async fn paper_result_tpsl_rule(
    app_state: web::Data<Arc<AppState>>,
    rule_id: web::Path<Uuid>,
) -> impl Responder {
    let rule_id = rule_id.into_inner();
    let rule_repo = StrategyTPSLRuleRepo::new(app_state.db.clone());
    let paper_repo = PaperTradingRepo::new(app_state.db.clone());

    let rule = match rule_repo.find_by_id(rule_id).await {
        Ok(Some(r)) => r,
        Ok(None) => {
            return HttpResponse::NotFound().json(serde_json::json!({"error": "Rule not found"}));
        }
        Err(e) => {
            tracing::error!("Failed to get rule {rule_id}: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "Failed to get rule"}));
        }
    };

    let run = match paper_repo.current_run(rule_id).await {
        Ok(run) => run,
        Err(e) => {
            tracing::error!("Failed to load paper run for {rule_id}: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "Failed to load paper run"}));
        }
    };

    let Some(run) = run else {
        return HttpResponse::Ok().json(PaperResultResponse {
            rule_name: rule.rule_name,
            run: None,
            tokens: vec![],
        });
    };

    let positions = match paper_repo.find_by_run(run.id).await {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("Failed to load paper positions for run {}: {e}", run.id);
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "Failed to load paper positions"}));
        }
    };

    // Resolve token symbols for display (best-effort; blank if unknown).
    let symbols: std::collections::HashMap<String, String> =
        match TokenRepo::new(app_state.db.clone()).find_all().await {
            Ok(tokens) => tokens
                .into_iter()
                .map(|t| (t.mint_address, t.symbol))
                .collect(),
            Err(e) => {
                tracing::warn!("Failed to load token symbols for paper result: {e}");
                std::collections::HashMap::new()
            }
        };

    let tokens: Vec<SimulatedTokenResult> = positions
        .into_iter()
        .map(|p| {
            let closed = p.status == PositionStatus::End;
            let pnl_percent = p.pnl_percentage();
            // The live paper exit path only ever fires take-profit / stop-loss, so
            // a closed position's reason is recoverable from its realized PnL sign.
            let exit_reason = if closed {
                if pnl_percent.unwrap_or(0.0) >= 0.0 {
                    "TakeProfit"
                } else {
                    "StopLoss"
                }
            } else {
                "Open"
            }
            .to_string();
            // entry_amount is the SOL allocated per buy, so PnL in SOL is direct.
            let pnl_sol = pnl_percent.map(|pct| p.entry_amount * (pct / 100.0));
            let holding_secs = match (p.entry_time, p.exit_time) {
                (Some(e), Some(x)) => Some((x - e).num_seconds()),
                _ => None,
            };
            let ath_price = p
                .exit_price
                .map(|x| x.max(p.entry_price))
                .unwrap_or(p.entry_price);
            SimulatedTokenResult {
                symbol: symbols.get(&p.mint).cloned().unwrap_or_default(),
                mint: p.mint,
                entry_price: p.entry_price,
                ath_price,
                entry_amount: p.entry_amount,
                entry_tx: p.entry_tx,
                entry_time: p.entry_time.unwrap_or(p.created_at),
                exit_price: p.exit_price,
                exit_tx: p.exit_tx,
                exit_time: p.exit_time,
                holding_secs,
                pnl_percent,
                pnl_sol,
                exit_reason,
                total_trades: 0,
            }
        })
        .collect();

    HttpResponse::Ok().json(PaperResultResponse {
        rule_name: rule.rule_name,
        run: Some(PaperRunResponse {
            run_seq: run.run_seq,
            status: run.status.as_str().to_string(),
            max_total_tokens: run.max_total_tokens,
            started_at: run.started_at,
            finished_at: run.finished_at,
        }),
        tokens,
    })
}
