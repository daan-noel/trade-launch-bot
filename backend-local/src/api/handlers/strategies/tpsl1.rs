//! Local (analysis-box) `tpsl_sniper_1` handlers — the **local runtime edge** of
//! the rule family. Shares the rule domain (`tpsl_rules_core`: DTOs + validation +
//! repo write) with the deploy edge, but carries only the local wiring:
//!   - CRUD (create/update/delete/get/list) with **no** live-cache reload, **no**
//!     `emit_rules_changed`, and **no** live-freeze guard — the local box never
//!     runs a rule, so there's nothing live to keep consistent or freeze.
//!   - `simulate` / `cancel_simulate` — the detached backtest over the shared
//!     decision layer.
//!   - `paper-result` (read) + `clear_paper-result` (no `is_live` guard).
//!
//! Live-only concerns (lifecycle activate/pause/stop, matched tokens, positions)
//! are deploy's and are not served here. Runtime performance counters
//! (open/total/win/loss) are always zero locally — there is no runtime cache — so
//! the rule list reflects authoring state, not live activity.

use actix_web::{web, HttpResponse, Responder};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    models::{ingest::SseEvent, PaperRunStatus, Position, Tpsl1Rule},
    state::local_state::LocalState,
    state::sim_results::SimOutcome,
    strategies::tpsl_sniper_1::backtest::BacktestTokenResult,
};

use backend_core::api::handlers::strategies::tpsl_rules_core::{tpsl1 as rules_core, RuleWriteError};
use rules_core::{CreateRuleRequest, UpdateRuleRequest};

// ---------------------------------------------------------------------------
// Response Types
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
    /// Derived lifecycle label. Locally a rule never runs, so this is `Finished`
    /// for a completed paper run and `Idle` otherwise.
    pub lifecycle: String,
    /// Live runtime counters — always 0 on the local box (no runtime cache). Kept
    /// in the payload so the shape matches the deploy edge's response.
    pub open_positions: i64,
    pub total_positions: i64,
    pub win_count: i64,
    pub loss_count: i64,
    pub win_rate: f64,
    pub avg_pnl_pct: f64,
    pub total_pnl_sol: f64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl RuleResponse {
    /// Build the response for a rule on the local box, where there is no runtime
    /// cache: all live counters are 0 and the lifecycle is derived from the rule's
    /// `is_active` flag + (for paper rules) its latest run status.
    fn build(r: Tpsl1Rule, paper_status: Option<PaperRunStatus>) -> Self {
        let lifecycle = if r.trade_mode == "paper" && paper_status == Some(PaperRunStatus::Finished)
        {
            "Finished"
        } else {
            "Idle"
        }
        .to_string();
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
            open_positions: 0,
            total_positions: 0,
            win_count: 0,
            loss_count: 0,
            win_rate: 0.0,
            avg_pnl_pct: 0.0,
            total_pnl_sol: 0.0,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

/// Enrich a single rule into a [`RuleResponse`] (one paper-run query for paper
/// rules, to derive the `Finished` lifecycle).
async fn rule_response(app_state: &Arc<LocalState>, rule: Tpsl1Rule) -> RuleResponse {
    let paper_status = if rule.trade_mode == "paper" {
        app_state
            .tpsl1_paper_repo()
            .current_run(rule.id)
            .await
            .ok()
            .flatten()
            .map(|run| run.status)
    } else {
        None
    };
    RuleResponse::build(rule, paper_status)
}

// ---------------------------------------------------------------------------
// Rule Handlers (CRUD) — domain core + local wiring (no cache reload)
// ---------------------------------------------------------------------------

/// List all TPSL rules.
pub async fn list_tpsl_rules(app_state: web::Data<Arc<LocalState>>) -> impl Responder {
    let repo = app_state.tpsl1_rule_repo();
    match repo.find_all().await {
        Ok(rules) => {
            // One query for all latest runs → status map (no N+1).
            let run_status: std::collections::HashMap<Uuid, PaperRunStatus> = app_state
                .tpsl1_paper_repo()
                .find_all_runs()
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|run| (run.rule_id, run.status))
                .collect();
            let responses: Vec<RuleResponse> = rules
                .into_iter()
                .map(|r| {
                    let status = if r.trade_mode == "paper" {
                        run_status.get(&r.id).copied()
                    } else {
                        None
                    };
                    RuleResponse::build(r, status)
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

/// Get a specific TPSL rule.
pub async fn get_tpsl_rule(
    app_state: web::Data<Arc<LocalState>>,
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

/// Create a new TPSL rule (local edge: domain write only, no live-cache reload).
pub async fn create_tpsl_rule(
    app_state: web::Data<Arc<LocalState>>,
    req: web::Json<CreateRuleRequest>,
) -> impl Responder {
    let repo = app_state.tpsl1_rule_repo();
    match rules_core::create(&repo, &req).await {
        Ok(rule) => HttpResponse::Created().json(rule_response(&app_state, rule).await),
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

/// Update an existing TPSL rule (local edge: no live-freeze guard, no cache reload).
pub async fn update_tpsl_rule(
    app_state: web::Data<Arc<LocalState>>,
    rule_id: web::Path<Uuid>,
    req: web::Json<UpdateRuleRequest>,
) -> impl Responder {
    let rule_id = rule_id.into_inner();
    let repo = app_state.tpsl1_rule_repo();

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

    match rules_core::apply_and_persist(&repo, rule, &req.0).await {
        Ok((rule, _mode_changed)) => HttpResponse::Ok().json(rule_response(&app_state, rule).await),
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

/// Delete a TPSL rule (local edge: domain write only).
pub async fn delete_tpsl_rule(
    app_state: web::Data<Arc<LocalState>>,
    rule_id: web::Path<Uuid>,
) -> impl Responder {
    let rule_id = rule_id.into_inner();
    let repo = app_state.tpsl1_rule_repo();
    match rules_core::delete(&repo, rule_id).await {
        Ok(()) => HttpResponse::NoContent().finish(),
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
// Simulation
// ---------------------------------------------------------------------------

/// Transient creation-time window for the simulate scan. Both bounds optional;
/// empty = all-time. `from` → `since` (inclusive), `to` → `until` (exclusive).
#[derive(serde::Deserialize)]
pub struct AnalysisRange {
    #[serde(default, deserialize_with = "de_opt_wallclock_utc")]
    pub from: Option<DateTime<Utc>>,
    #[serde(default, deserialize_with = "de_opt_wallclock_utc")]
    pub to: Option<DateTime<Utc>>,
}

/// Lenient deserializer for the optional analysis-window bounds: accepts a full
/// RFC3339 instant or the frontend's offset-less UTC wall-clock
/// (`YYYY-MM-DDTHH:MM[:SS]`), appending `Z`/`:00Z` to the latter.
fn de_opt_wallclock_utc<'de, D>(de: D) -> Result<Option<DateTime<Utc>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    let raw: Option<String> = Option::deserialize(de)?;
    let Some(v) = raw.as_deref().map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    if let Ok(d) = DateTime::parse_from_rfc3339(v) {
        return Ok(Some(d.with_timezone(&Utc)));
    }
    let iso = if v.len() == 16 { format!("{v}:00Z") } else { format!("{v}Z") };
    DateTime::parse_from_rfc3339(&iso)
        .map(|d| Some(d.with_timezone(&Utc)))
        .map_err(serde::de::Error::custom)
}

/// Start a TPSL1 rule backtest as a detached background job and return at once.
/// The run stores its terminal outcome in `sim_results`; the client collects it
/// via `GET /api/jobs/simulations/{rule_id}/result` once the `simulation_finished`
/// SSE fires — there is no long-held connection to cut.
///
/// POST /api/strategies/tpsl1/rules/{rule_id}/simulate
pub async fn simulate_tpsl_rule(
    app_state: web::Data<Arc<LocalState>>,
    rule_id: web::Path<Uuid>,
    range: web::Query<AnalysisRange>,
) -> impl Responder {
    let rid = rule_id.into_inner();
    let (since, until) = (range.from, range.to);
    // Register a cooperative cancel flag + readable progress snapshot so the
    // global jobs endpoints can observe and abort this run. Both are removed on
    // every exit path (RAII guard), which broadcasts the terminal
    // `SimulationFinished`. Drop any stale result so only this run's is collectable.
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let cell = std::sync::Arc::new(crate::state::job_progress::ProgressCell::default());
    app_state.sim_cancels.insert(rid, cancel.clone());
    app_state.sim_progress.insert(rid, cell.clone());
    app_state.sim_results.clear(&rid);

    actix_web::rt::spawn(async move {
        struct SimGuard {
            state: web::Data<Arc<LocalState>>,
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

        let outcome = match crate::strategies::tpsl_sniper_1::run_backtest(
            app_state.clone(),
            rid,
            since,
            until,
            cancel,
            cell,
        )
        .await
        {
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
                    SimOutcome::Cancelled
                } else if msg.contains("Rule not found") {
                    SimOutcome::Failed { status: 404, message: msg }
                } else if msg.contains("All rule criteria are empty") {
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

/// `POST /api/strategies/tpsl1/rules/{rule_id}/simulate/cancel` — cooperative
/// cancel of an in-flight simulation. A no-op when none is running for the rule.
pub async fn cancel_simulate_tpsl_rule(
    app_state: web::Data<Arc<LocalState>>,
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
    pub run: Option<PaperRunResponse>,
    pub tokens: Vec<BacktestTokenResult>,
}

/// Upper bound on positions pulled for the paper-result summary (mirrors the
/// matched-tokens cap). Larger runs should aggregate server-side.
const PAPER_RESULT_MAX_TOKENS: i64 = 5000;

/// Aggregate the latest paper-test run's recorded positions into a
/// simulation-shaped result.
///
/// GET /api/strategies/tpsl1/rules/{rule_id}/paper-result
pub async fn paper_result_tpsl_rule(
    app_state: web::Data<Arc<LocalState>>,
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

    let positions = match paper_repo.find_by_run(run.id, PAPER_RESULT_MAX_TOKENS, 0).await {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("Failed to load paper positions for run {}: {e}", run.id);
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "Failed to load paper positions"}));
        }
    };

    let mints: Vec<String> = positions.iter().map(|p| p.mint.clone()).collect();
    let symbols = app_state
        .token_repo()
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

/// Clear a paper rule's recorded run history (runs + positions). Paper-only.
/// Unlike the deploy edge there is **no** live-cache guard: the local box never
/// runs a rule, so a clear always proceeds.
///
/// DELETE /api/strategies/tpsl1/rules/{rule_id}/paper-result
pub async fn clear_paper_result_tpsl_rule(
    app_state: web::Data<Arc<LocalState>>,
    rule_id: web::Path<Uuid>,
) -> impl Responder {
    let rule_id = rule_id.into_inner();
    let rule = match app_state.tpsl1_rule_repo().find_by_id(rule_id).await {
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

    match app_state.tpsl1_paper_repo().clear_runs(rule_id).await {
        Ok(n) => {
            tracing::info!("Cleared {n} paper run(s) for rule {rule_id}");
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
/// frontend card/table renders. Pure (no DB) so it stays unit-testable.
pub(crate) fn paper_position_to_sim_result(
    p: Position,
    symbols: &std::collections::HashMap<String, String>,
) -> BacktestTokenResult {
    let pnl_percent = p.pnl_percentage();
    let exit_reason = p
        .exit_reason_or_derived()
        .unwrap_or_else(|| "Open".to_string());
    let pnl_sol = pnl_percent.and_then(|pct| p.entry_token_amount.map(|a| a * (pct / 100.0)));
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

    fn closed(entry: f64, exit: f64) -> Position {
        let mut p = Position::new("mint".into(), "wallet".into(), "TPSL1".into(), Uuid::new_v4());
        p.entry_price = Some(entry);
        p.entry_tx_signatures = vec!["etx".into()];
        p.entry_token_amount = Some(0.05);
        p.entry_time = Some(Utc::now());
        p.close(exit, vec!["xtx".into()], 0.05, Utc::now());
        p
    }

    #[test]
    fn uses_stored_exit_reason_not_pnl_sign() {
        let mut p = closed(1.0, 1.5);
        p.exit_reason = Some("TrailingStop".into());
        let r = paper_position_to_sim_result(p, &HashMap::new());
        assert_eq!(r.exit_reason, "TrailingStop");
    }

    #[test]
    fn legacy_rows_fall_back_to_pnl_sign() {
        let win = paper_position_to_sim_result(closed(1.0, 1.5), &HashMap::new());
        assert_eq!(win.exit_reason, "TakeProfit");
        let loss = paper_position_to_sim_result(closed(1.0, 0.5), &HashMap::new());
        assert_eq!(loss.exit_reason, "StopLoss");
    }

    #[test]
    fn open_position_reads_as_open() {
        let mut p = Position::new("mint".into(), "wallet".into(), "TPSL1".into(), Uuid::new_v4());
        p.entry_price = Some(1.0);
        p.entry_tx_signatures = vec!["etx".into()];
        p.entry_token_amount = Some(0.05);
        p.entry_time = Some(Utc::now());
        let r = paper_position_to_sim_result(p, &HashMap::new());
        assert_eq!(r.exit_reason, "Open");
    }
}
