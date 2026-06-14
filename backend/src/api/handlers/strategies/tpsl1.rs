use actix_web::{web, HttpResponse, Responder};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    models::{ingest::SseEvent, PaperRunStatus, Position, Tpsl1Rule},
    state::app_state::AppState,
    strategies::tpsl_sniper_1::{
        self, backtest::BacktestTokenResult, entry::token_matches_buy_rule, PaperActivation,
    },
};

// ---------------------------------------------------------------------------
// Response / Request Types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct RuleResponse {
    pub id: Uuid,
    pub rule_name: String,
    pub p_token_initial_buy_sol: Option<f64>,
    pub p_token_cu_limit: Option<u64>,
    pub p_token_cu_price: Option<u64>,
    pub p_token_max_sol_cost: Option<f64>,
    pub p_token_spendable_sol_in: Option<f64>,
    pub p_max_concurrent_tokens: Option<u64>,
    pub p_max_total_tokens: Option<u64>,
    pub p_token_ix_labels: serde_json::Value,
    pub trade_mode: String,
    pub buy_amount: f64,
    pub p_exit_take_profit: f64,
    pub p_exit_stop_loss: f64,
    pub p_exit_trailing_stop_pct: Option<f64>,
    pub p_exit_time_stop_secs: Option<u64>,
    pub p_exit_stall_secs: Option<u64>,
    pub p_exit_liquidity_drop_pct: Option<f64>,
    pub tolerance_pct: f64,
    pub is_active: bool,
    /// Derived lifecycle state for the UI — one of `Active`, `Draining`,
    /// `Finished`, `Idle`. See [`lifecycle_label`].
    pub lifecycle: String,
    /// Number of positions this rule currently holds open (drives the
    /// `Draining (N open)` badge and gates the Stop & close action).
    pub open_positions: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Collapse `(is_active, open positions, paper-run status)` into the single
/// lifecycle label the frontend renders. The model: `is_active` gates **entries**
/// only, so an inactive rule with open positions is still *Draining* (its exits
/// run until they close). Paper rules that reached their cap read *Finished*;
/// everything else inactive-and-flat is *Idle*.
fn lifecycle_label(rule: &Tpsl1Rule, open_positions: i64, paper_status: Option<PaperRunStatus>) -> &'static str {
    if rule.is_active {
        "Active"
    } else if open_positions > 0 {
        "Draining"
    } else if rule.trade_mode == "paper" && paper_status == Some(PaperRunStatus::Finished) {
        "Finished"
    } else {
        "Idle"
    }
}

impl RuleResponse {
    /// Build the response from a rule plus the live context needed to derive its
    /// lifecycle (`open_positions` from the runtime cache; `paper_status` from the
    /// rule's current run, or `None` for real rules / rules with no run).
    fn build(r: Tpsl1Rule, open_positions: i64, paper_status: Option<PaperRunStatus>) -> Self {
        let lifecycle = lifecycle_label(&r, open_positions, paper_status).to_string();
        Self {
            id: r.id,
            rule_name: r.rule_name,
            p_token_initial_buy_sol: r.p_token_initial_buy_sol,
            p_token_cu_limit: r.p_token_cu_limit,
            p_token_cu_price: r.p_token_cu_price,
            p_token_max_sol_cost: r.p_token_max_sol_cost,
            p_token_spendable_sol_in: r.p_token_spendable_sol_in,
            p_max_concurrent_tokens: r.p_max_concurrent_tokens,
            p_max_total_tokens: r.p_max_total_tokens,
            p_token_ix_labels: r.p_token_ix_labels,
            trade_mode: r.trade_mode,
            buy_amount: r.buy_amount,
            p_exit_take_profit: r.p_exit_take_profit,
            p_exit_stop_loss: r.p_exit_stop_loss,
            p_exit_trailing_stop_pct: r.p_exit_trailing_stop_pct,
            p_exit_time_stop_secs: r.p_exit_time_stop_secs,
            p_exit_stall_secs: r.p_exit_stall_secs,
            p_exit_liquidity_drop_pct: r.p_exit_liquidity_drop_pct,
            tolerance_pct: r.tolerance_pct,
            is_active: r.is_active,
            lifecycle,
            open_positions,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

/// Enrich a single rule into a [`RuleResponse`] (one paper-run query for paper
/// rules). The list endpoint avoids this per-rule query via a bulk run lookup.
/// Best-effort cold-lane signal that the tpsl1 rule list changed (create / update
/// / delete), so SSE clients refetch it instead of waiting on the fallback poll.
fn emit_rules_changed(app_state: &Arc<AppState>) {
    let _ = app_state.sse_tx.send(SseEvent::TpslRulesChanged {
        strategy: "tpsl1".to_string(),
    });
}

async fn rule_response(app_state: &Arc<AppState>, rule: Tpsl1Rule) -> RuleResponse {
    let open = app_state.tpsl1_cache.holding_count_by_rule(rule.id);
    let paper_status = if rule.trade_mode == "paper" {
        app_state.tpsl1_paper_repo()
            .current_run(rule.id)
            .await
            .ok()
            .flatten()
            .map(|run| run.status)
    } else {
        None
    };
    RuleResponse::build(rule, open, paper_status)
}

#[derive(Deserialize)]
pub struct CreateRuleRequest {
    pub rule_name: String,
    pub p_token_initial_buy_sol: Option<f64>,
    pub p_token_cu_limit: Option<u64>,
    pub p_token_cu_price: Option<u64>,
    pub p_token_max_sol_cost: Option<f64>,
    pub p_token_spendable_sol_in: Option<f64>,
    pub p_max_concurrent_tokens: Option<u64>,
    pub p_max_total_tokens: Option<u64>,
    pub p_token_ix_labels: serde_json::Value,
    pub trade_mode: String,
    pub buy_amount: f64,
    pub p_exit_take_profit: f64,
    pub p_exit_stop_loss: f64,
    pub p_exit_trailing_stop_pct: Option<f64>,
    pub p_exit_time_stop_secs: Option<u64>,
    pub p_exit_stall_secs: Option<u64>,
    pub p_exit_liquidity_drop_pct: Option<f64>,
    pub tolerance_pct: Option<f64>,
}

#[derive(Deserialize, Serialize)]
pub struct UpdateRuleRequest {
    pub rule_name: Option<String>,
    pub buy_amount: Option<f64>,
    pub p_exit_take_profit: Option<f64>,
    pub p_exit_stop_loss: Option<f64>,
    pub p_exit_trailing_stop_pct: Option<f64>,
    pub p_exit_time_stop_secs: Option<u64>,
    pub p_exit_stall_secs: Option<u64>,
    pub p_exit_liquidity_drop_pct: Option<f64>,
    #[serde(default)]
    pub p_token_initial_buy_sol: Option<Option<f64>>,
    #[serde(default)]
    pub p_token_cu_limit: Option<Option<u64>>,
    #[serde(default)]
    pub p_token_cu_price: Option<Option<u64>>,
    #[serde(default)]
    pub p_token_ix_labels: Option<Option<serde_json::Value>>,
    #[serde(default)]
    pub p_token_max_sol_cost: Option<Option<f64>>,
    #[serde(default)]
    pub p_token_spendable_sol_in: Option<Option<f64>>,
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
    let repo = app_state.tpsl1_rule_repo();

    match repo.find_all().await {
        Ok(rules) => {
            // One query for all latest runs → status map, so enriching N rules
            // with their lifecycle stays a single round-trip (no N+1).
            let paper_repo = app_state.tpsl1_paper_repo();
            let run_status: HashMap<Uuid, PaperRunStatus> = paper_repo
                .find_all_runs()
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|run| (run.rule_id, run.status))
                .collect();
            let responses: Vec<RuleResponse> = rules
                .into_iter()
                .map(|r| {
                    let open = app_state.tpsl1_cache.holding_count_by_rule(r.id);
                    let status = if r.trade_mode == "paper" {
                        run_status.get(&r.id).copied()
                    } else {
                        None
                    };
                    RuleResponse::build(r, open, status)
                })
                .collect();
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
    let repo = app_state.tpsl1_rule_repo();
    let rule_id = rule_id.into_inner();

    match repo.find_by_id(rule_id).await {
        Ok(Some(rule)) => HttpResponse::Ok().json(rule_response(&app_state, rule).await),
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
    let rule = Tpsl1Rule::new(
        req.rule_name.clone(),
        req.p_token_initial_buy_sol,
        req.p_token_cu_limit,
        req.p_token_cu_price,
        req.p_token_ix_labels.clone(),
        req.trade_mode.clone(),
        req.buy_amount,
        req.p_exit_take_profit,
        req.p_exit_stop_loss,
        req.p_token_max_sol_cost,
        req.p_token_spendable_sol_in,
        req.p_max_concurrent_tokens,
        req.p_max_total_tokens,
        req.tolerance_pct,
        req.p_exit_trailing_stop_pct,
        req.p_exit_time_stop_secs,
        req.p_exit_stall_secs,
        req.p_exit_liquidity_drop_pct,
    );

    let repo = app_state.tpsl1_rule_repo();

    match repo.insert(&rule).await {
        Ok(_) => {
            if let Err(e) = app_state.tpsl1_cache.reload_rules(&app_state.db).await {
                tracing::warn!("TPSL rule cache reload after create failed: {e}");
            }
            emit_rules_changed(&app_state);
            HttpResponse::Created().json(rule_response(&app_state, rule).await)
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
    let repo = app_state.tpsl1_rule_repo();

    match repo.find_by_id(rule_id).await {
        Ok(Some(mut rule)) => {
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
            if let Some(p_exit_take_profit) = req.p_exit_take_profit {
                rule.p_exit_take_profit = p_exit_take_profit;
            }
            if let Some(p_exit_stop_loss) = req.p_exit_stop_loss {
                rule.p_exit_stop_loss = p_exit_stop_loss;
            }
            if let Some(trailing_stop_pct) = req.p_exit_trailing_stop_pct {
                rule.p_exit_trailing_stop_pct = Some(trailing_stop_pct);
            }
            if let Some(time_stop_secs) = req.p_exit_time_stop_secs {
                rule.p_exit_time_stop_secs = Some(time_stop_secs);
            }
            if let Some(stall_secs) = req.p_exit_stall_secs {
                rule.p_exit_stall_secs = Some(stall_secs);
            }
            if let Some(liquidity_drop_pct) = req.p_exit_liquidity_drop_pct {
                rule.p_exit_liquidity_drop_pct = Some(liquidity_drop_pct);
            }
            if let Some(initial_buy_sol_opt) = &req.p_token_initial_buy_sol {
                rule.p_token_initial_buy_sol = initial_buy_sol_opt.clone();
            }
            if let Some(cu_limit_opt) = &req.p_token_cu_limit {
                rule.p_token_cu_limit = cu_limit_opt.clone();
            }
            if let Some(cu_price_opt) = &req.p_token_cu_price {
                rule.p_token_cu_price = cu_price_opt.clone();
            }
            if let Some(ix_labels_opt) = &req.p_token_ix_labels {
                rule.p_token_ix_labels = ix_labels_opt
                    .clone()
                    .unwrap_or_else(|| serde_json::Value::Array(vec![]));
            }
            if let Some(max_sol_cost_opt) = &req.p_token_max_sol_cost {
                rule.p_token_max_sol_cost = max_sol_cost_opt.clone();
            }
            if let Some(spendable_sol_in_opt) = &req.p_token_spendable_sol_in {
                rule.p_token_spendable_sol_in = spendable_sol_in_opt.clone();
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
            // `is_active` is intentionally NOT applied here: activation/pause is
            // owned by the dedicated lifecycle endpoints (`activate`/`pause`/
            // `stop`) so the paper-run side effects can't drift. This PUT only
            // edits rule fields.
            if let Some(trade_mode) = &req.trade_mode {
                rule.trade_mode = trade_mode.clone();
            }

            match repo.update(&rule).await {
                Ok(_) => {
                    if let Err(e) = app_state.tpsl1_cache.reload_rules(&app_state.db).await {
                        tracing::warn!("TPSL rule cache reload after update failed: {e}");
                    }
                    emit_rules_changed(&app_state);
                    HttpResponse::Ok().json(rule_response(&app_state, rule).await)
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
    let repo = app_state.tpsl1_rule_repo();

    match repo.delete(rule_id).await {
        Ok(_) => {
            if let Err(e) = app_state.tpsl1_cache.reload_rules(&app_state.db).await {
                tracing::warn!("TPSL rule cache reload after delete failed: {e}");
            }
            emit_rules_changed(&app_state);
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
// Lifecycle: activate / pause / stop-and-close
//
// These replace the old `is_active` toggle on the PUT update endpoint. All three
// route through `tpsl_sniper_1::lifecycle`, the single source of truth for the
// paper-run + cache side effects (so activation and auto-finish can't drift).
// ---------------------------------------------------------------------------

/// Body for `activate`. For paper rules, `paper_run` selects whether to start a
/// fresh run or continue the prior one; ignored for real rules. Absent / unknown
/// defaults to a fresh run.
#[derive(Deserialize)]
pub struct ActivateRequest {
    #[serde(default)]
    pub paper_run: Option<String>,
}

/// Translate `lifecycle::*` errors into the right HTTP status (404 for a missing
/// rule, 500 otherwise) — the helpers return `anyhow::Error` with `"Rule not
/// found"` for the not-found case.
fn lifecycle_error(action: &str, e: anyhow::Error) -> HttpResponse {
    let msg = e.to_string();
    if msg.contains("Rule not found") {
        HttpResponse::NotFound().json(serde_json::json!({"error": "Rule not found"}))
    } else {
        tracing::error!("Failed to {action} TPSL rule: {e}");
        HttpResponse::InternalServerError().json(serde_json::json!({"error": msg}))
    }
}

/// Activate a rule (entries on). For paper rules, `{ "paper_run": "fresh" |
/// "continue" }` chooses fresh-run vs resume.
///
/// POST /api/strategies/tpsl1/rules/{rule_id}/activate
pub async fn activate_tpsl_rule(
    app_state: web::Data<Arc<AppState>>,
    rule_id: web::Path<Uuid>,
    req: web::Json<ActivateRequest>,
) -> impl Responder {
    let rule_id = rule_id.into_inner();
    let paper = match req.paper_run.as_deref() {
        Some("continue") => PaperActivation::Continue,
        _ => PaperActivation::Fresh,
    };
    match tpsl_sniper_1::activate_rule(&app_state, rule_id, paper).await {
        Ok(rule) => HttpResponse::Ok().json(rule_response(&app_state, rule).await),
        Err(e) => lifecycle_error("activate", e),
    }
}

/// Pause a rule (entries off; open positions drain via the exit ladder).
///
/// POST /api/strategies/tpsl1/rules/{rule_id}/pause
pub async fn pause_tpsl_rule(
    app_state: web::Data<Arc<AppState>>,
    rule_id: web::Path<Uuid>,
) -> impl Responder {
    let rule_id = rule_id.into_inner();
    match tpsl_sniper_1::pause_rule(&app_state, rule_id).await {
        Ok(rule) => HttpResponse::Ok().json(rule_response(&app_state, rule).await),
        Err(e) => lifecycle_error("pause", e),
    }
}

/// Stop a rule and force-close every open position now. The response is the
/// enriched rule; `open_positions` reflects how many are still draining (real
/// sells finish in the background, so they may briefly remain).
///
/// POST /api/strategies/tpsl1/rules/{rule_id}/stop
pub async fn stop_tpsl_rule(
    app_state: web::Data<Arc<AppState>>,
    rule_id: web::Path<Uuid>,
) -> impl Responder {
    let rule_id = rule_id.into_inner();
    match tpsl_sniper_1::stop_and_close_rule(&app_state, rule_id).await {
        Ok((rule, closing)) => {
            tracing::info!("Stop & close rule {rule_id}: {closing} position(s) closing");
            HttpResponse::Ok().json(rule_response(&app_state, rule).await)
        }
        Err(e) => lifecycle_error("stop", e),
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
/// GET /api/strategies/tpsl1/rules/{rule_id}/matched
pub async fn get_matched_tokens(
    app_state: web::Data<Arc<AppState>>,
    rule_id: web::Path<Uuid>,
) -> impl Responder {
    let rule_id = rule_id.into_inner();
    let rule_repo = app_state.tpsl1_rule_repo();

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

    // Match against the in-memory token cache — the same source the live run
    // evaluates — instead of `SELECT *`-ing the continuously-growing `tokens`
    // table on every click. Filter by reference (non-matches are never cloned)
    // on the blocking pool so the CPU scan can't stall an HTTP worker, mirroring
    // the Tokens list endpoint.
    let state = app_state.get_ref().clone();
    let matched = web::block(move || -> Vec<MatchedTokenResult> {
        state
            .token_cache
            .iter()
            .filter(|e| token_matches_buy_rule(&e.value().token, &rule))
            .map(|e| {
                let t = &e.value().token;
                MatchedTokenResult {
                    mint: t.mint_address.clone(),
                    symbol: t.symbol.clone(),
                    name: t.name.clone(),
                    created_at: t.created_at,
                    initial_buy_sol: t.initial_buy_sol,
                    cu_limit: t.cu_limit,
                    cu_price: t.cu_price,
                }
            })
            .collect()
    })
    .await;

    match matched {
        Ok(matched) => HttpResponse::Ok().json(matched),
        Err(e) => {
            tracing::error!("matched-token build failed: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "Failed to build matched tokens"}))
        }
    }
}

// ---------------------------------------------------------------------------
// Simulation
// ---------------------------------------------------------------------------

/// Simulate a TPSL rule against all historically matched tokens.
///
/// GET /api/strategies/tpsl1/rules/{rule_id}/simulate
pub async fn simulate_tpsl_rule(
    app_state: web::Data<Arc<AppState>>,
    rule_id: web::Path<Uuid>,
) -> impl Responder {
    // delegate to the tpsl_sniper_1 simulation module (E1+ exit-walk engine)
    let rid = rule_id.into_inner();
    match crate::strategies::tpsl_sniper_1::run_backtest(app_state.clone(), rid).await {
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
    pub tokens: Vec<BacktestTokenResult>,
}

/// Aggregate the latest paper-test run's recorded positions into a
/// simulation-shaped result.
///
/// GET /api/strategies/tpsl1/rules/{rule_id}/paper-result
pub async fn paper_result_tpsl_rule(
    app_state: web::Data<Arc<AppState>>,
    rule_id: web::Path<Uuid>,
) -> impl Responder {
    let rule_id = rule_id.into_inner();
    let rule_repo = app_state.tpsl1_rule_repo();
    let paper_repo = app_state.tpsl1_paper_repo();

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

    // Resolve token symbols for display (best-effort; blank if unknown). Only the
    // run's own position mints are looked up — no full-table scan.
    let mints: Vec<String> = positions.iter().map(|p| p.mint.clone()).collect();
    let symbols = app_state.token_repo()
        .find_symbols_for(&mints)
        .await
        .unwrap_or_else(|e| {
            tracing::warn!("Failed to load token symbols for paper result: {e}");
            std::collections::HashMap::new()
        });

    let tokens: Vec<BacktestTokenResult> = positions
        .into_iter()
        .map(|p| paper_position_to_sim_result(p, &symbols))
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

/// Map one recorded paper position into the simulation-shaped result the shared
/// frontend card/table renders. Pure (no DB) so it stays unit-testable and so
/// the positions endpoint and this endpoint provably enumerate the same rows.
pub(crate) fn paper_position_to_sim_result(
    p: Position,
    symbols: &std::collections::HashMap<String, String>,
) -> BacktestTokenResult {
    let pnl_percent = p.pnl_percentage();
    // The reason recorded at exit time (the live path now fires the full E1–E4
    // ladder, so it can't be inferred from the PnL sign), with the legacy-row
    // fallback baked into the model. A still-open row shows "Open".
    let exit_reason = p
        .exit_reason_or_derived()
        .unwrap_or_else(|| "Open".to_string());
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
    BacktestTokenResult {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// A closed position entered at `entry`, exited at `exit` (no stored reason).
    fn closed(entry: f64, exit: f64) -> Position {
        let mut p = Position::new(
            "mint".into(),
            "wallet".into(),
            entry,
            "etx".into(),
            "TPSL1".into(),
            Uuid::new_v4(),
            0.05,
        );
        p.entry_time = Some(Utc::now());
        p.close(exit, "xtx".into(), 0.05, Utc::now());
        p
    }

    // The stored reason wins over the PnL-sign heuristic — the regression the
    // live E1–E4 exits introduced: a trailing stop that banks a gain must show
    // as TrailingStop, not TakeProfit.
    #[test]
    fn uses_stored_exit_reason_not_pnl_sign() {
        let mut p = closed(1.0, 1.5); // +50%, would heuristically read TakeProfit
        p.exit_reason = Some("TrailingStop".into());
        let r = paper_position_to_sim_result(p, &HashMap::new());
        assert_eq!(r.exit_reason, "TrailingStop");
    }

    // Legacy rows (column added after they closed) have no stored reason and
    // fall back to the PnL sign for a clean close.
    #[test]
    fn legacy_rows_fall_back_to_pnl_sign() {
        let win = paper_position_to_sim_result(closed(1.0, 1.5), &HashMap::new());
        assert_eq!(win.exit_reason, "TakeProfit");
        let loss = paper_position_to_sim_result(closed(1.0, 0.5), &HashMap::new());
        assert_eq!(loss.exit_reason, "StopLoss");
    }

    // A still-open position carries no exit reason.
    #[test]
    fn open_position_reads_as_open() {
        let mut p = Position::new(
            "mint".into(),
            "wallet".into(),
            1.0,
            "etx".into(),
            "TPSL1".into(),
            Uuid::new_v4(),
            0.05,
        );
        p.entry_time = Some(Utc::now());
        let r = paper_position_to_sim_result(p, &HashMap::new());
        assert_eq!(r.exit_reason, "Open");
    }
}
