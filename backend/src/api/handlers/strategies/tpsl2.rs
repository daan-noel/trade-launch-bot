use actix_web::{web, HttpResponse, Responder};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    models::{ingest::SseEvent, PaperRunStatus, Position, Tpsl2Rule},
    state::app_state::AppState,
    state::sim_results::SimOutcome,
    strategies::tpsl_sniper_2::{
        self, backtest::BacktestTokenResult, entry::token_matches_buy_rule,
        runtime_cache::RuleClosedStats, PaperActivation,
    },
};

use super::tpsl_rules_core::{tpsl2 as rules_core, RuleWriteError};

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
    // Scalp-continuation gates (see tpsl-scalp-continuation-plan.md).
    pub p_entry_min_age_secs: Option<u64>,
    pub p_entry_max_age_secs: Option<u64>,
    pub p_entry_min_alive_sol: Option<f64>,
    pub p_entry_min_organic_sol: Option<f64>,
    pub p_entry_pullback_pct: Option<f64>,
    pub p_entry_higher_low_secs: Option<u64>,
    pub p_entry_max_cohort_held: Option<f64>,
    pub p_entry_min_liquidity_sol: Option<f64>,
    pub p_entry_min_organic_liq: Option<f64>,
    pub p_exit_cohort_ratio: Option<f64>,
    pub tolerance_pct: f64,
    pub is_active: bool,
    /// Derived lifecycle state for the UI — one of `Active`, `Draining`,
    /// `Finished`, `Idle`. See [`lifecycle_label`].
    pub lifecycle: String,
    /// Number of positions this rule currently holds open (drives the
    /// `Draining (N open)` badge and gates the Stop & close action).
    pub open_positions: i64,
    /// Realized-performance stats (all-time for real rules, current-run for
    /// paper). `total_positions` = entered positions; `win/loss_count` cover
    /// closed positions only; `win_rate`/`avg_pnl_pct` are 0 until something
    /// closes. All sourced from the runtime cache — no per-request DB query.
    pub total_positions: i64,
    pub win_count: i64,
    pub loss_count: i64,
    pub win_rate: f64,
    pub avg_pnl_pct: f64,
    pub total_pnl_sol: f64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Collapse `(is_active, open positions, paper-run status)` into the single
/// lifecycle label the frontend renders. The model: `is_active` gates **entries**
/// only, so an inactive rule with open positions is still *Draining* (its exits
/// run until they close). Paper rules that reached their cap read *Finished*;
/// everything else inactive-and-flat is *Idle*.
fn lifecycle_label(rule: &Tpsl2Rule, open_positions: i64, paper_status: Option<PaperRunStatus>) -> &'static str {
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
    /// lifecycle (`open_positions` + `total_positions` + `stats` from the runtime
    /// cache; `paper_status` from the rule's current run, or `None` for real
    /// rules / rules with no run). Win rate and average PnL % are derived here
    /// from the cache's raw sums (0 when nothing has closed).
    fn build(
        r: Tpsl2Rule,
        open_positions: i64,
        total_positions: i64,
        stats: RuleClosedStats,
        paper_status: Option<PaperRunStatus>,
    ) -> Self {
        let lifecycle = lifecycle_label(&r, open_positions, paper_status).to_string();
        let closed = stats.closed();
        let (win_rate, avg_pnl_pct) = if closed > 0 {
            (
                stats.wins as f64 / closed as f64 * 100.0,
                stats.sum_pnl_pct / closed as f64,
            )
        } else {
            (0.0, 0.0)
        };
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
            p_entry_min_age_secs: r.p_entry_min_age_secs,
            p_entry_max_age_secs: r.p_entry_max_age_secs,
            p_entry_min_alive_sol: r.p_entry_min_alive_sol,
            p_entry_min_organic_sol: r.p_entry_min_organic_sol,
            p_entry_pullback_pct: r.p_entry_pullback_pct,
            p_entry_higher_low_secs: r.p_entry_higher_low_secs,
            p_entry_max_cohort_held: r.p_entry_max_cohort_held,
            p_entry_min_liquidity_sol: r.p_entry_min_liquidity_sol,
            p_entry_min_organic_liq: r.p_entry_min_organic_liq,
            p_exit_cohort_ratio: r.p_exit_cohort_ratio,
            tolerance_pct: r.tolerance_pct,
            is_active: r.is_active,
            lifecycle,
            open_positions,
            total_positions,
            win_count: stats.wins,
            loss_count: stats.losses,
            win_rate,
            avg_pnl_pct,
            total_pnl_sol: stats.sum_pnl_sol,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

/// Enrich a single rule into a [`RuleResponse`] (one paper-run query for paper
/// rules). The list endpoint avoids this per-rule query via a bulk run lookup.
/// Best-effort cold-lane signal that the tpsl2 rule list changed (create / update
/// / delete), so SSE clients refetch it instead of waiting on the fallback poll.
fn emit_rules_changed(app_state: &Arc<AppState>) {
    let _ = app_state.sse_tx.send(SseEvent::TpslRulesChanged {
        strategy: "tpsl2".to_string(),
    });
}

async fn rule_response(app_state: &Arc<AppState>, rule: Tpsl2Rule) -> RuleResponse {
    let open = app_state.tpsl2_cache.holding_count_by_rule(rule.id);
    let total = app_state.tpsl2_cache.total_count_by_rule(rule.id);
    let stats = app_state.tpsl2_cache.closed_stats_by_rule(rule.id);
    let paper_status = if rule.trade_mode == "paper" {
        app_state.tpsl2_paper_repo()
            .current_run(rule.id)
            .await
            .ok()
            .flatten()
            .map(|run| run.status)
    } else {
        None
    };
    RuleResponse::build(rule, open, total, stats, paper_status)
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
    // Scalp-continuation gates; absent/0 = disabled.
    #[serde(default)]
    pub p_entry_min_age_secs: Option<u64>,
    #[serde(default)]
    pub p_entry_max_age_secs: Option<u64>,
    #[serde(default)]
    pub p_entry_min_alive_sol: Option<f64>,
    #[serde(default)]
    pub p_entry_min_organic_sol: Option<f64>,
    #[serde(default)]
    pub p_entry_pullback_pct: Option<f64>,
    #[serde(default)]
    pub p_entry_higher_low_secs: Option<u64>,
    #[serde(default)]
    pub p_entry_max_cohort_held: Option<f64>,
    #[serde(default)]
    pub p_entry_min_liquidity_sol: Option<f64>,
    #[serde(default)]
    pub p_entry_min_organic_liq: Option<f64>,
    #[serde(default)]
    pub p_exit_cohort_ratio: Option<f64>,
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
    // Scalp-continuation gates; present → set (0 disables, per ignore_zero).
    pub p_entry_min_age_secs: Option<u64>,
    pub p_entry_max_age_secs: Option<u64>,
    pub p_entry_min_alive_sol: Option<f64>,
    pub p_entry_min_organic_sol: Option<f64>,
    pub p_entry_pullback_pct: Option<f64>,
    pub p_entry_higher_low_secs: Option<u64>,
    pub p_entry_max_cohort_held: Option<f64>,
    pub p_entry_min_liquidity_sol: Option<f64>,
    pub p_entry_min_organic_liq: Option<f64>,
    pub p_exit_cohort_ratio: Option<f64>,
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

impl UpdateRuleRequest {
    /// True if the request would change any field that is FROZEN while a rule is
    /// live (running or holding positions) — the token fingerprint, scalp entry
    /// gates, exit ladder, and matching tolerance, i.e. anything that redefines
    /// which tokens the rule takes or when it exits. The only fields editable
    /// mid-run are the "hot" set: `buy_amount` + concurrency caps + the
    /// administrative `rule_name`. Note `trade_mode` is also frozen while live,
    /// but is always present in the PUT body, so it's checked separately (by
    /// value) rather than here. Mirrors the per-group lock in `RuleFormModal`;
    /// defense-in-depth against a non-UI caller.
    fn touches_frozen_fields(&self) -> bool {
        self.p_exit_take_profit.is_some()
            || self.p_exit_stop_loss.is_some()
            || self.p_exit_trailing_stop_pct.is_some()
            || self.p_exit_time_stop_secs.is_some()
            || self.p_exit_stall_secs.is_some()
            || self.p_exit_liquidity_drop_pct.is_some()
            || self.p_exit_cohort_ratio.is_some()
            || self.p_entry_min_age_secs.is_some()
            || self.p_entry_max_age_secs.is_some()
            || self.p_entry_min_alive_sol.is_some()
            || self.p_entry_min_organic_sol.is_some()
            || self.p_entry_pullback_pct.is_some()
            || self.p_entry_higher_low_secs.is_some()
            || self.p_entry_max_cohort_held.is_some()
            || self.p_entry_min_liquidity_sol.is_some()
            || self.p_entry_min_organic_liq.is_some()
            || self.p_token_initial_buy_sol.is_some()
            || self.p_token_cu_limit.is_some()
            || self.p_token_cu_price.is_some()
            || self.p_token_ix_labels.is_some()
            || self.p_token_max_sol_cost.is_some()
            || self.p_token_spendable_sol_in.is_some()
            || self.tolerance_pct.is_some()
    }
}

// ---------------------------------------------------------------------------
// Rule Handlers
// ---------------------------------------------------------------------------

/// List all TPSL rules
pub async fn list_tpsl_rules(app_state: web::Data<Arc<AppState>>) -> impl Responder {
    let repo = app_state.tpsl2_rule_repo();

    match repo.find_all().await {
        Ok(rules) => {
            // One query for all latest runs → status map, so enriching N rules
            // with their lifecycle stays a single round-trip (no N+1).
            let paper_repo = app_state.tpsl2_paper_repo();
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
                    let open = app_state.tpsl2_cache.holding_count_by_rule(r.id);
                    let total = app_state.tpsl2_cache.total_count_by_rule(r.id);
                    let stats = app_state.tpsl2_cache.closed_stats_by_rule(r.id);
                    let status = if r.trade_mode == "paper" {
                        run_status.get(&r.id).copied()
                    } else {
                        None
                    };
                    RuleResponse::build(r, open, total, stats, status)
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
    let repo = app_state.tpsl2_rule_repo();
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

// ---------------------------------------------------------------------------
// --- DEPLOY --- CRUD write edges
//
// Thin wrappers over `tpsl_rules_core::tpsl2` (validation + repo write); the only
// thing they add is the runtime side effect — refresh the live `tpsl2_cache` and
// nudge SSE clients. The domain logic is shared with the local crate's wrappers.
// ---------------------------------------------------------------------------

/// Create a new TPSL rule
pub async fn create_tpsl_rule(
    app_state: web::Data<Arc<AppState>>,
    req: web::Json<CreateRuleRequest>,
) -> impl Responder {
    let repo = app_state.tpsl2_rule_repo();
    match rules_core::create(&repo, &req).await {
        Ok(rule) => {
            if let Err(e) = app_state.tpsl2_cache.reload_rules(&app_state.db).await {
                tracing::warn!("TPSL rule cache reload after create failed: {e}");
            }
            emit_rules_changed(&app_state);
            HttpResponse::Created().json(rule_response(&app_state, rule).await)
        }
        Err(RuleWriteError::Invalid(msg)) => {
            HttpResponse::BadRequest().json(serde_json::json!({ "error": msg }))
        }
        Err(RuleWriteError::Repo(e)) => {
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
    let repo = app_state.tpsl2_rule_repo();

    let rule = match repo.find_by_id(rule_id).await {
        Ok(Some(rule)) => rule,
        Ok(None) => {
            return HttpResponse::NotFound().json(serde_json::json!({"error": "Rule not found"}));
        }
        Err(e) => {
            tracing::error!("Failed to get TPSL rule {rule_id}: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "Failed to get rule"}));
        }
    };

    // Log incoming update request for debugging
    match serde_json::to_value(&req.0) {
        Ok(v) => tracing::debug!("UpdateRuleRequest JSON: {}", v),
        Err(e) => tracing::debug!("Failed to serialize UpdateRuleRequest: {e}"),
    }

    // Deploy-only live-freeze guard (reads the runtime cache): while the rule is
    // live (running, or still draining open positions) only the "hot" fields may
    // change — Rule Name, Buy Amount, and the concurrency caps. Everything else
    // (match/entry/exit criteria, and the paper/real mode) redefines the rule, so
    // it's frozen and the run can't shift under itself. The UI enforces this via
    // per-group locks; this guards non-UI callers. (Dropped in the local crate,
    // where rules never run.)
    let live = rule.is_active || app_state.tpsl2_cache.holding_count_by_rule(rule_id) > 0;
    let changes_mode = req
        .trade_mode
        .as_deref()
        .is_some_and(|m| m != rule.trade_mode);
    if live && (req.touches_frozen_fields() || changes_mode) {
        return HttpResponse::Conflict().json(serde_json::json!({
            "error": "Rule is live: only Rule Name, Buy Amount, and concurrency caps can be edited while running or holding positions",
        }));
    }

    match rules_core::apply_and_persist(&repo, rule, &req.0).await {
        Ok((rule, mode_changed)) => {
            // Switching real<->paper changes which table this rule's stats come
            // from, so the cached per-rule counters must be fully recomputed
            // (`reload_rules` only swaps the rule list, leaving stale stats).
            let reload = if mode_changed {
                app_state.tpsl2_cache.load_from_db(&app_state.db).await
            } else {
                app_state.tpsl2_cache.reload_rules(&app_state.db).await
            };
            if let Err(e) = reload {
                tracing::warn!("TPSL rule cache reload after update failed: {e}");
            }
            emit_rules_changed(&app_state);
            HttpResponse::Ok().json(rule_response(&app_state, rule).await)
        }
        Err(RuleWriteError::Invalid(msg)) => {
            HttpResponse::BadRequest().json(serde_json::json!({ "error": msg }))
        }
        Err(RuleWriteError::Repo(e)) => {
            tracing::error!("Failed to update TPSL rule {rule_id}: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "Failed to update rule"}))
        }
    }
}

/// Delete a TPSL rule
pub async fn delete_tpsl_rule(
    app_state: web::Data<Arc<AppState>>,
    rule_id: web::Path<Uuid>,
) -> impl Responder {
    let rule_id = rule_id.into_inner();
    let repo = app_state.tpsl2_rule_repo();

    match rules_core::delete(&repo, rule_id).await {
        Ok(()) => {
            if let Err(e) = app_state.tpsl2_cache.reload_rules(&app_state.db).await {
                tracing::warn!("TPSL rule cache reload after delete failed: {e}");
            }
            emit_rules_changed(&app_state);
            HttpResponse::NoContent().finish()
        }
        Err(RuleWriteError::Invalid(msg)) => {
            HttpResponse::BadRequest().json(serde_json::json!({ "error": msg }))
        }
        Err(RuleWriteError::Repo(e)) => {
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
// route through `tpsl_sniper_2::lifecycle`, the single source of truth for the
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
/// POST /api/strategies/tpsl2/rules/{rule_id}/activate
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
    match tpsl_sniper_2::activate_rule(&app_state, rule_id, paper).await {
        Ok(rule) => HttpResponse::Ok().json(rule_response(&app_state, rule).await),
        Err(e) => lifecycle_error("activate", e),
    }
}

/// Pause a rule (entries off; open positions drain via the exit ladder).
///
/// POST /api/strategies/tpsl2/rules/{rule_id}/pause
pub async fn pause_tpsl_rule(
    app_state: web::Data<Arc<AppState>>,
    rule_id: web::Path<Uuid>,
) -> impl Responder {
    let rule_id = rule_id.into_inner();
    match tpsl_sniper_2::pause_rule(&app_state, rule_id).await {
        Ok(rule) => HttpResponse::Ok().json(rule_response(&app_state, rule).await),
        Err(e) => lifecycle_error("pause", e),
    }
}

/// Stop a rule and force-close every open position now. The response is the
/// enriched rule; `open_positions` reflects how many are still draining (real
/// sells finish in the background, so they may briefly remain).
///
/// POST /api/strategies/tpsl2/rules/{rule_id}/stop
pub async fn stop_tpsl_rule(
    app_state: web::Data<Arc<AppState>>,
    rule_id: web::Path<Uuid>,
) -> impl Responder {
    let rule_id = rule_id.into_inner();
    match tpsl_sniper_2::stop_and_close_rule(&app_state, rule_id).await {
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

/// Transient creation-time window for the analysis endpoints (matched + simulate).
/// Both bounds optional; empty = all-time. Not persisted on the rule — it's a
/// per-request scope the page supplies to bound an otherwise full-table scan.
/// `from` → `since` (inclusive), `to` → `until` (exclusive).
#[derive(Deserialize)]
pub struct AnalysisRange {
    #[serde(default, deserialize_with = "de_opt_wallclock_utc")]
    pub from: Option<DateTime<Utc>>,
    #[serde(default, deserialize_with = "de_opt_wallclock_utc")]
    pub to: Option<DateTime<Utc>>,
}

/// Lenient deserializer for the optional analysis-window bounds. The frontend's
/// `datetimeLocalToUtcWallClock` helper emits an offset-less UTC wall-clock string
/// (`YYYY-MM-DDTHH:MM[:SS]`) — the same shape the tokens / creation-stats
/// endpoints accept via their `parse_dt`. Strict `DateTime<Utc>` serde rejects the
/// missing offset (chrono `TooShort` → "premature end of input", surfaced as a
/// `Query deserialize error`), so we append a `Z` to the bare wall-clock here while
/// still accepting a full RFC3339 instant. Keeps the field type unchanged so the
/// downstream `since`/`until` plumbing is untouched.
fn de_opt_wallclock_utc<'de, D>(de: D) -> Result<Option<DateTime<Utc>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw: Option<String> = Option::deserialize(de)?;
    let Some(v) = raw.as_deref().map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    // A full RFC3339 string (carries an offset) parses as-is; a bare wall-clock
    // gets `:00Z`/`Z` appended to make it a valid UTC instant.
    if let Ok(d) = DateTime::parse_from_rfc3339(v) {
        return Ok(Some(d.with_timezone(&Utc)));
    }
    let iso = if v.len() == 16 { format!("{v}:00Z") } else { format!("{v}Z") };
    DateTime::parse_from_rfc3339(&iso)
        .map(|d| Some(d.with_timezone(&Utc)))
        .map_err(serde::de::Error::custom)
}

/// Upper bound on matched rows returned to the page. Matches are sparse, so this
/// is normally far from binding; when it is hit we log and expose the true total
/// in the response so the frontend can inform the user.
const MATCHED_RESULT_CAP: usize = 5_000;

#[derive(Serialize)]
pub struct MatchedTokensResponse {
    pub tokens: Vec<MatchedTokenResult>,
    /// True count BEFORE the cap was applied. Equal to `tokens.len()` when
    /// the cap was not reached; greater when it was (use `capped` to detect).
    pub total: usize,
    pub capped: bool,
}

/// Return the tokens in the database that satisfy a rule's entry criteria.
///
/// Scans the **whole** `tokens` table (keyset-streamed, decoupled from the live
/// `token_cache` so an old, evicted-but-matching mint is still found), optionally
/// bounded to `?from=&to=` by `created_at`. Only the sparse matches are held in
/// memory — never the table.
///
/// GET /api/strategies/tpsl/rules/{rule_id}/matched
pub async fn get_matched_tokens(
    app_state: web::Data<Arc<AppState>>,
    rule_id: web::Path<Uuid>,
    range: web::Query<AnalysisRange>,
) -> impl Responder {
    let rule_id = rule_id.into_inner();
    let rule_repo = app_state.tpsl2_rule_repo();

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

    let repo = app_state.token_repo();
    let matched = crate::strategies::analysis::collect_matching_tokens(
        &repo,
        range.from,
        range.to,
        |t| token_matches_buy_rule(t, &rule),
    )
    .await;

    let mut tokens = match matched {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("matched-token scan failed: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "Failed to build matched tokens"}));
        }
    };

    let total = tokens.len();
    let capped = total > MATCHED_RESULT_CAP;
    if capped {
        tracing::warn!(
            rule_id = %rule_id,
            matched = total,
            cap = MATCHED_RESULT_CAP,
            "matched-token result capped; narrow the from/to range to see the rest"
        );
        tokens.truncate(MATCHED_RESULT_CAP);
    }

    let results: Vec<MatchedTokenResult> = tokens
        .into_iter()
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

    HttpResponse::Ok().json(MatchedTokensResponse { tokens: results, total, capped })
}

// ---------------------------------------------------------------------------
// --- LOCAL --- Simulation
// ---------------------------------------------------------------------------

/// Start a TPSL2 rule backtest as a detached background job and return at once.
///
/// POST /api/strategies/tpsl2/rules/{rule_id}/simulate
///
/// The simulation runs uncapped and can take minutes; holding the HTTP connection
/// open for the whole run (the old design) meant any mid-run drop — dev proxy /
/// browser idle cut / the ingest watchdog restarting the process under load —
/// severed the socket and surfaced on the client as a `FETCH_ERROR`, even though
/// the detached run finished fine. Instead the run stores its terminal outcome in
/// `sim_results` and the client collects it via
/// `GET /api/jobs/simulations/{rule_id}/result` once the `simulation_finished`
/// SSE fires — there is no long-held connection to cut.
pub async fn simulate_tpsl_rule(
    app_state: web::Data<Arc<AppState>>,
    rule_id: web::Path<Uuid>,
    range: web::Query<AnalysisRange>,
) -> impl Responder {
    // delegate to the tpsl_sniper_2 simulation module (E1+ exit-walk engine)
    let rid = rule_id.into_inner();
    // Optional creation-time window scoping the candidate scan (empty = all-time).
    let (since, until) = (range.from, range.to);
    // Register a cooperative cancel flag + a readable progress snapshot for this
    // run so the global jobs endpoints (`/api/jobs/status`, the generic sim-cancel)
    // can observe and abort it. Both are removed on every exit path (RAII guard),
    // which also broadcasts the terminal `SimulationFinished` so a global progress
    // indicator clears itself without polling. Registered synchronously here so an
    // immediate cancel finds the entry before the task is scheduled. Drop any stale
    // result so only this run's outcome is collectable.
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let cell = std::sync::Arc::new(crate::state::job_progress::ProgressCell::default());
    app_state.sim_cancels.insert(rid, cancel.clone());
    app_state.sim_progress.insert(rid, cell.clone());
    app_state.sim_results.clear(&rid);

    // Detach the backtest and return immediately. `rt::spawn` keeps the task on the
    // worker (no `Send` bound) independent of this request; a client disconnect
    // never cancels it. The task stores its outcome BEFORE the guard drops (which
    // fires `SimulationFinished`), so a client reacting to that SSE always finds
    // the result present.
    actix_web::rt::spawn(async move {
        struct SimGuard {
            state: web::Data<Arc<AppState>>,
            rule_id: Uuid,
            cancel: Arc<std::sync::atomic::AtomicBool>,
        }
        impl Drop for SimGuard {
            fn drop(&mut self) {
                self.state.sim_cancels.remove(&self.rule_id);
                self.state.sim_progress.remove(&self.rule_id);
                let _ = self.state.sse_tx.send(SseEvent::SimulationFinished {
                    rule_id: self.rule_id,
                    cancelled: self.cancel.load(std::sync::atomic::Ordering::Acquire),
                });
            }
        }
        let _guard = SimGuard {
            state: app_state.clone(),
            rule_id: rid,
            cancel: cancel.clone(),
        };

        let outcome = match crate::strategies::tpsl_sniper_2::run_backtest(
            app_state.clone(),
            rid,
            since,
            until,
            cancel,
            cell,
        )
        .await
        {
            // Serialize once here so the result endpoint serves the bytes verbatim.
            Ok(summary) => match serde_json::to_string(&summary) {
                Ok(json) => SimOutcome::Done(json),
                Err(e) => SimOutcome::Failed {
                    status: 500,
                    message: format!("result serialization failed: {e}"),
                },
            },
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("cancelled") {
                    // User-requested abort — benign, not a failure.
                    SimOutcome::Cancelled
                } else if msg.contains("Rule not found") {
                    SimOutcome::Failed { status: 404, message: msg }
                } else if msg.contains("no scalp entry gate") {
                    SimOutcome::Failed { status: 400, message: msg }
                } else {
                    tracing::error!("Simulation failed: {e}");
                    SimOutcome::Failed { status: 500, message: msg }
                }
            }
        };
        app_state.sim_results.insert(rid, outcome);
    });

    HttpResponse::Accepted().json(serde_json::json!({ "started": true }))
}

/// `POST /api/strategies/tpsl2/rules/{rule_id}/simulate/cancel` — request
/// cancellation of an in-flight simulation for this rule (see the tpsl1 twin).
pub async fn cancel_simulate_tpsl_rule(
    app_state: web::Data<Arc<AppState>>,
    rule_id: web::Path<Uuid>,
) -> impl Responder {
    let rid = rule_id.into_inner();
    let cancelling = match app_state.sim_cancels.get(&rid) {
        Some(flag) => {
            flag.store(true, std::sync::atomic::Ordering::Release);
            true
        }
        None => false,
    };
    HttpResponse::Ok().json(serde_json::json!({ "cancelling": cancelling }))
}

// ---------------------------------------------------------------------------
// --- LOCAL --- Paper-test result
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

/// Upper bound on positions pulled for the paper-result summary. The summary
/// card aggregates over every row, so this can't use the 200-row table default;
/// it mirrors the 5,000-row matched-tokens cap. Runs larger than this should move
/// the aggregation server-side (SUM/COUNT) rather than raise the cap.
const PAPER_RESULT_MAX_TOKENS: i64 = 5000;

/// Aggregate the latest paper-test run's recorded positions into a
/// simulation-shaped result.
///
/// GET /api/strategies/tpsl/rules/{rule_id}/paper-result
pub async fn paper_result_tpsl_rule(
    app_state: web::Data<Arc<AppState>>,
    rule_id: web::Path<Uuid>,
) -> impl Responder {
    let rule_id = rule_id.into_inner();
    let rule_repo = app_state.tpsl2_rule_repo();
    let paper_repo = app_state.tpsl2_paper_repo();

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

    let positions = match paper_repo.find_by_run(run.id, PAPER_RESULT_MAX_TOKENS, 0).await {
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

/// Clear a paper rule's recorded run history (runs + positions). Paper-only, and
/// only while the rule is idle (not active, no open positions) — an in-flight run
/// must not be wiped under itself. After clearing, the rule reads `Idle` and its
/// paper-result view is empty.
///
/// DELETE /api/strategies/tpsl2/rules/{rule_id}/paper-result
pub async fn clear_paper_result_tpsl_rule(
    app_state: web::Data<Arc<AppState>>,
    rule_id: web::Path<Uuid>,
) -> impl Responder {
    let rule_id = rule_id.into_inner();
    let rule = match app_state.tpsl2_rule_repo().find_by_id(rule_id).await {
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

    if rule.trade_mode != "paper" {
        return HttpResponse::BadRequest()
            .json(serde_json::json!({"error": "Only paper rules have results to clear"}));
    }
    // NOTE (crate split): this live-cache guard is a deploy-only concern. In the
    // local crate (no live runtime) rules are never live, so this guard is dropped
    // there (T14) and the clear always proceeds. Left intact here for now.
    let live = rule.is_active || app_state.tpsl2_cache.holding_count_by_rule(rule_id) > 0;
    if live {
        return HttpResponse::Conflict().json(serde_json::json!({
            "error": "Rule is live: stop it before clearing its results",
        }));
    }

    match app_state.tpsl2_paper_repo().clear_runs(rule_id).await {
        Ok(n) => {
            tracing::info!("Cleared {n} paper run(s) for rule {rule_id}");
            emit_rules_changed(&app_state);
            HttpResponse::NoContent().finish()
        }
        Err(e) => {
            tracing::error!("Failed to clear paper results for {rule_id}: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "Failed to clear paper results"}))
        }
    }
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
    // entry_token_amount is the token count bought; SOL invested = entry_price ×
    // tokens, so PnL in SOL = (entry_price × entry_token_amount) × pct/100.
    let pnl_sol = pnl_percent.and_then(|pct| match (p.entry_price, p.entry_token_amount) {
        (Some(price), Some(tokens)) => Some(price * tokens * (pct / 100.0)),
        _ => None,
    });
    let holding_secs = match (p.entry_time, p.exit_time) {
        (Some(e), Some(x)) => Some((x - e).num_seconds()),
        _ => None,
    };
    let ath_price = p
        .exit_price
        .map(|x| x.max(p.entry_price.unwrap_or(0.0)))
        .or(p.entry_price)
        .unwrap_or(0.0);
    BacktestTokenResult {
        symbol: symbols.get(&p.mint).cloned().unwrap_or_default(),
        target_price: p.target_price,
        target_token_amount: p.target_token_amount,
        target_time: p.target_time,
        target_tx: p.target_tx,
        mint: p.mint,
        entry_price: p.entry_price.unwrap_or(0.0),
        ath_price,
        entry_token_amount: p.entry_token_amount.unwrap_or(0.0),
        entry_tx: p.entry_tx_signatures.first().cloned().unwrap_or_default(),
        entry_time: p.entry_time.unwrap_or(p.created_at),
        exit_price: p.exit_price,
        exit_tx: p.exit_tx_signatures.last().cloned(),
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
            "TPSL2".into(),
            Uuid::new_v4(),
        );
        p.entry_price = Some(entry);
        p.entry_tx_signatures = vec!["etx".into()];
        p.entry_token_amount = Some(0.05);
        p.entry_time = Some(Utc::now());
        p.close(exit, vec!["xtx".into()], 0.05, Utc::now());
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
            "TPSL2".into(),
            Uuid::new_v4(),
        );
        p.entry_price = Some(1.0);
        p.entry_tx_signatures = vec!["etx".into()];
        p.entry_token_amount = Some(0.05);
        p.entry_time = Some(Utc::now());
        let r = paper_position_to_sim_result(p, &HashMap::new());
        assert_eq!(r.exit_reason, "Open");
    }
}
