use actix_web::{web, HttpResponse, Responder};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    models::{Position, StrategyTPSLRule},
    models::trade::TradeType,
    state::app_state::AppState,
    storage::repositories::{
        position_repo::PositionRepo,
        strategy_tpsl_rule_repo::StrategyTPSLRuleRepo,
        token_repo::TokenRepo,
        trade_repo::TradeRepo,
    },
};

// ---------------------------------------------------------------------------
// Response Types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct RuleResponse {
    pub id: Uuid,
    pub rule_name: String,
    pub p_initial_buy_sol: f64,
    pub p_cu_limit: Option<u64>,
    pub p_cu_price: Option<u64>,
    pub p_ix_labels: serde_json::Value,
    pub buy_amount: f64,
    pub take_profit: f64,
    pub stop_loss: f64,
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
            p_ix_labels: r.p_ix_labels,
            buy_amount: r.buy_amount,
            take_profit: r.take_profit,
            stop_loss: r.stop_loss,
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
    pub p_initial_buy_sol: f64,
    pub p_cu_limit: Option<u64>,
    pub p_cu_price: Option<u64>,
    pub p_ix_labels: serde_json::Value,
    pub buy_amount: f64,
    pub take_profit: f64,
    pub stop_loss: f64,
}

#[derive(Deserialize)]
pub struct UpdateRuleRequest {
    pub rule_name: Option<String>,
    pub buy_amount: Option<f64>,
    pub take_profit: Option<f64>,
    pub stop_loss: Option<f64>,
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

/// Per-token simulation result.
#[derive(Serialize)]
pub struct SimulatedTokenResult {
    pub mint: String,
    pub symbol: String,
    pub entry_price: f64,
    pub entry_tx: String,
    pub entry_time: DateTime<Utc>,
    pub exit_price: Option<f64>,
    pub exit_tx: Option<String>,
    pub exit_time: Option<DateTime<Utc>>,
    /// Seconds from entry to exit (None if still open).
    pub holding_secs: Option<i64>,
    pub pnl_percent: Option<f64>,
    /// PnL in SOL based on the rule's buy_amount.
    pub pnl_sol: Option<f64>,
    /// "TakeProfit", "StopLoss", or "Open"
    pub exit_reason: String,
    pub total_trades: usize,
}

/// Aggregate statistics for the whole simulation run.
#[derive(Serialize)]
pub struct SimulationSummary {
    pub rule_id: Uuid,
    pub rule_name: String,
    pub tokens_matched: usize,
    pub win_count: usize,
    pub loss_count: usize,
    pub open_count: usize,
    pub win_rate_pct: f64,
    pub total_pnl_sol: f64,
    pub avg_pnl_pct: Option<f64>,
    pub avg_holding_secs: Option<f64>,
    pub best_pnl_pct: Option<f64>,
    pub worst_pnl_pct: Option<f64>,
    pub tokens: Vec<SimulatedTokenResult>,
}

// ── Entry / exit helpers ─────────────────────────────────────────────────────

/// Determine the simulated entry price for a token.
///
/// Candidates: all BUY trades in the first slot + the first `second_block_cap`
/// BUY trades in the second slot. Returns the highest candidate price (worst
/// case / conservative entry).
fn find_entry(
    trades: &[crate::models::trade::Trade],
    second_block_cap: usize,
) -> Option<(f64, String, DateTime<Utc>)> {
    if trades.is_empty() {
        return None;
    }

    let first_slot = trades[0].slot;
    // Find the second distinct slot
    let second_slot = trades.iter().find(|t| t.slot > first_slot).map(|t| t.slot)?;

    // Collect candidate buy trades
    let mut candidates: Vec<&crate::models::trade::Trade> = Vec::new();

    for t in trades.iter() {
        if t.trade_type != TradeType::Buy {
            continue;
        }
        if t.slot == first_slot {
            candidates.push(t);
        } else if t.slot == second_slot {
            // Only take the first `second_block_cap` from this slot
            let already = candidates.iter().filter(|c| c.slot == second_slot).count();
            if already < second_block_cap {
                candidates.push(t);
            }
        }
    }

    candidates
        .into_iter()
        .max_by(|a, b| a.price_per_token.partial_cmp(&b.price_per_token).unwrap_or(std::cmp::Ordering::Equal))
        .map(|t| (t.price_per_token, t.tx_signature.clone(), t.block_time))
}

/// Walk trades in chronological order and find the first point where the price
/// triggers take_profit or stop_loss relative to `entry_price`.
///
/// When triggered, the **exit price** = lowest price in that same slot (worst
/// case for our sell). Returns `(exit_price, exit_tx, exit_time, reason)`.
fn find_exit(
    trades: &[crate::models::trade::Trade],
    entry_time: DateTime<Utc>,
    entry_price: f64,
    take_profit_pct: f64,
    stop_loss_pct: f64,
) -> Option<(f64, String, DateTime<Utc>, String)> {
    // Only examine trades that happened after entry
    let later: Vec<&crate::models::trade::Trade> = trades
        .iter()
        .filter(|t| t.block_time > entry_time)
        .collect();

    for t in later.iter() {
        if entry_price <= 0.0 {
            break;
        }
        let pct = ((t.price_per_token - entry_price) / entry_price) * 100.0;
        let triggered = pct >= take_profit_pct || pct <= -stop_loss_pct;
        if !triggered {
            continue;
        }

        let reason = if pct >= take_profit_pct { "TakeProfit" } else { "StopLoss" };
        let exit_slot = t.slot;

        // Exit price = lowest price in the exit slot (worst-case fill)
        let exit_candidates: Vec<&crate::models::trade::Trade> =
            later.iter().copied().filter(|t| t.slot == exit_slot).collect();

        let exit_trade = exit_candidates
            .into_iter()
            .min_by(|a, b| a.price_per_token.partial_cmp(&b.price_per_token).unwrap_or(std::cmp::Ordering::Equal));

        if let Some(et) = exit_trade {
            return Some((et.price_per_token, et.tx_signature.clone(), et.block_time, reason.to_string()));
        }
    }
    None
}

// ── Handler ──────────────────────────────────────────────────────────────────

/// Simulate a TPSL rule against all historically matched tokens.
///
/// GET /api/strategies/tpsl/rules/{rule_id}/simulate
pub async fn simulate_tpsl_rule(
    app_state: web::Data<Arc<AppState>>,
    rule_id: web::Path<Uuid>,
) -> impl Responder {
    let rule_id = rule_id.into_inner();
    let rule_repo = StrategyTPSLRuleRepo::new(app_state.db.clone());

    // 1. Fetch the rule
    let rule: StrategyTPSLRule = match rule_repo.find_by_id(rule_id).await {
        Ok(Some(r)) => r,
        Ok(None) => {
            return HttpResponse::NotFound()
                .json(serde_json::json!({"error": "Rule not found"}));
        }
        Err(e) => {
            tracing::error!("Failed to fetch rule {rule_id}: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "DB error"}));
        }
    };

    // 2. Find all matched tokens (no limit — None = unlimited)
    let token_repo = TokenRepo::new(app_state.db.clone());

    // Guard: refuse to simulate if every criteria field is empty — that would
    // match the entire token table and produce meaningless results.
    let has_initial_buy = rule.p_initial_buy_sol != 0.0;
    let has_cu_limit    = rule.p_cu_limit.is_some();
    let has_cu_price    = rule.p_cu_price.is_some();
    let has_ix_labels   = rule.p_ix_labels
        .as_array()
        .map_or(false, |a| !a.is_empty());
    if !has_initial_buy && !has_cu_limit && !has_cu_price && !has_ix_labels {
        return HttpResponse::BadRequest()
            .json(serde_json::json!({"error": "All rule criteria are empty — simulation would match every token"}));
    }

    let tokens = match token_repo
        .find_by_rule_criteria(
            Some(rule.p_initial_buy_sol),
            rule.p_cu_limit,
            rule.p_cu_price,
            Some(&rule.p_ix_labels),
            None,
        )
        .await
    {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("Failed to query tokens for rule {rule_id}: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "DB error fetching tokens"}));
        }
    };

    // 3. Simulate each token
    let trade_repo = TradeRepo::new(app_state.db.clone());
    let mut results: Vec<SimulatedTokenResult> = Vec::with_capacity(tokens.len());

    for token in &tokens {
        let trades = match trade_repo.find_by_mint_all(&token.mint_address).await {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("Skipping {}: trade fetch failed: {e}", token.mint_address);
                continue;
            }
        };

        // Entry: highest buy price from block-1 (all buys) + first 5 buys in block-2
        let Some((entry_price, entry_tx, entry_time)) = find_entry(&trades, 1) else {
            continue; // no usable entry
        };

        // Exit: TP/SL scan
        let exit = find_exit(&trades, entry_time, entry_price, rule.take_profit, rule.stop_loss);

        let (exit_price, exit_tx, exit_time, exit_reason, holding_secs, pnl_percent, pnl_sol) =
            match exit {
                Some((ep, et, etime, reason)) => {
                    let secs = (etime - entry_time).num_seconds();
                    let pct = ((ep - entry_price) / entry_price) * 100.0;
                    let sol = rule.buy_amount * (pct / 100.0);
                    (Some(ep), Some(et), Some(etime), reason, Some(secs), Some(pct), Some(sol))
                }
                None => (None, None, None, "Open".to_string(), None, None, None),
            };

        results.push(SimulatedTokenResult {
            mint: token.mint_address.clone(),
            symbol: token.symbol.clone(),
            entry_price,
            entry_tx,
            entry_time,
            exit_price,
            exit_tx,
            exit_time,
            holding_secs,
            pnl_percent,
            pnl_sol,
            exit_reason,
            total_trades: trades.len(),
        });
    }

    // 4. Aggregate stats
    let tokens_matched = results.len();
    let win_count = results.iter().filter(|r| r.exit_reason == "TakeProfit").count();
    let loss_count = results.iter().filter(|r| r.exit_reason == "StopLoss").count();
    let open_count = results.iter().filter(|r| r.exit_reason == "Open").count();

    let closed: Vec<&SimulatedTokenResult> = results
        .iter()
        .filter(|r| r.exit_reason != "Open")
        .collect();

    let win_rate_pct = if !closed.is_empty() {
        (win_count as f64 / closed.len() as f64) * 100.0
    } else {
        0.0
    };

    let total_pnl_sol: f64 = results.iter().filter_map(|r| r.pnl_sol).sum();

    let avg_pnl_pct = if !closed.is_empty() {
        let sum: f64 = closed.iter().filter_map(|r| r.pnl_percent).sum();
        Some(sum / closed.len() as f64)
    } else {
        None
    };

    let avg_holding_secs = if !closed.is_empty() {
        let sum: f64 = closed.iter().filter_map(|r| r.holding_secs).map(|s| s as f64).sum();
        Some(sum / closed.len() as f64)
    } else {
        None
    };

    let best_pnl_pct = results
        .iter()
        .filter_map(|r| r.pnl_percent)
        .reduce(f64::max);

    let worst_pnl_pct = results
        .iter()
        .filter_map(|r| r.pnl_percent)
        .reduce(f64::min);

    // Sort results: TP first, then SL, then Open; within each group by pnl desc
    results.sort_by(|a, b| {
        let rank = |r: &str| match r {
            "TakeProfit" => 0,
            "StopLoss" => 1,
            _ => 2,
        };
        rank(&a.exit_reason)
            .cmp(&rank(&b.exit_reason))
            .then_with(|| b.pnl_percent.unwrap_or(0.0).partial_cmp(&a.pnl_percent.unwrap_or(0.0)).unwrap_or(std::cmp::Ordering::Equal))
    });

    let summary = SimulationSummary {
        rule_id,
        rule_name: rule.rule_name,
        tokens_matched,
        win_count,
        loss_count,
        open_count,
        win_rate_pct,
        total_pnl_sol,
        avg_pnl_pct,
        avg_holding_secs,
        best_pnl_pct,
        worst_pnl_pct,
        tokens: results,
    };

    HttpResponse::Ok().json(summary)
}
