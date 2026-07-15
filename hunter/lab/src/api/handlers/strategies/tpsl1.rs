//! Local (analysis-box) `tpsl_sniper_1` handlers over the **unified**
//! `strategy_rules` / `strategy_runs` / `strategy_positions` schema — the same
//! [`StrategyRepo`] + registry the deploy box uses. Carries only the local wiring:
//!   - CRUD (create/update/delete/get/list) with **no** live-cache reload, **no**
//!     `rules_changed` SSE, and **no** live-freeze guard — the local box never runs
//!     a rule, so there's nothing live to keep consistent or freeze (every field
//!     stays editable).
//!   - `simulate` / `cancel_simulate` — the detached backtest over the shared
//!     decision layer.
//!   - `paper-result` (read) + `clear_paper-result` — read/clear the persisted
//!     paper runs (authored on the deploy box, restored here via a DB snapshot).
//!   - `matched` — the whole-`tokens`-table entry-criteria scan backing the page's
//!     "Matched Tokens" view. Pure DB analysis (no keys / no gRPC), so it lives on
//!     the analysis box alongside `simulate`, sharing its candidate predicate.
//!
//! Live-only concerns (lifecycle activate/pause/stop, real positions) are deploy's
//! and are not served here. The lab has no runtime cache, so the rule list's
//! position counters (open/pending/total/win/loss) are computed from the **synced**
//! `strategy_positions` rows (each rule's latest paper run) via
//! [`StrategyRepo::rule_counters_for_latest_paper_runs`] — not from live activity.

use actix_web::{web, HttpResponse, Responder};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    state::local_state::LocalState,
    strategies::token_enrich::TokenEnrichment,
    strategies::tpsl_sniper_1::backtest::BacktestTokenResult,
};

use trading_core::api::table_query::TableRequest;
use trading_core::models::{StrategyPosition, StrategyRule};
use trading_core::storage::repositories::strategy_repo::{PositionQuery, RuleCounters, StrategyRepo};
use trading_core::strategies::registry::StrategyImpl;
use trading_core::strategies::rules::{self, params_to_value, RuleDraft, RuleError};

/// The strategy this handler module serves (canonical `strategy_id`).
const STRATEGY_ID: &str = "tpsl_sniper_1";
const STRATEGY: StrategyImpl = StrategyImpl::Tpsl1;

// Universal (typed-column) request keys — everything else in a body is a
// strategy-specific param. Mirror the deploy edge so the shared frontend body
// shape is parsed identically.
const RULE_NAME: &str = "rule_name";
const BUY_AMOUNT: &str = "buy_amount_sol";
const TRADE_MODE: &str = "trade_mode";
const MAX_CONCURRENT: &str = "p_max_concurrent_tokens";
const MAX_TOTAL: &str = "p_max_total_tokens";

/// Pull `Some(i64)` from a JSON number, `None` for JSON null.
fn opt_i64(v: &Value) -> Option<i64> {
    v.as_i64()
}

/// Map a [`RuleError`] to its HTTP response.
fn rule_error(e: RuleError, ctx: &str) -> HttpResponse {
    match e {
        RuleError::Invalid(msg) => HttpResponse::BadRequest().json(json!({ "error": msg })),
        RuleError::Repo(err) => {
            tracing::error!("Failed to {ctx}: {err}");
            HttpResponse::InternalServerError().json(json!({"error": format!("Failed to {ctx}")}))
        }
    }
}

// ---------------------------------------------------------------------------
// Response shaping
// ---------------------------------------------------------------------------

/// Build the flat frontend rule shape (params JSONB merged with the universal
/// columns), the position counters, and a derived lifecycle. `paper_finished`
/// flags a paper rule whose latest run is `Finished`. `counters` carries the
/// per-rule position counts computed from the synced DB rows (the lab has no
/// runtime cache, so these come from `strategy_positions`, not live state); pass
/// `&RuleCounters::default()` for an all-zero row. The shape matches the deploy
/// edge's `rule_to_json`.
fn rule_to_json(rule: &StrategyRule, paper_finished: bool, counters: &RuleCounters) -> Value {
    let mut obj: Map<String, Value> = match &rule.params {
        Value::Object(m) => m.clone(),
        _ => Map::new(),
    };

    obj.insert("id".into(), json!(rule.id));
    obj.insert("strategy_id".into(), json!(rule.strategy_id));
    obj.insert(RULE_NAME.into(), json!(rule.rule_name));
    obj.insert(BUY_AMOUNT.into(), json!(rule.buy_amount_sol));
    obj.insert(TRADE_MODE.into(), json!(rule.trade_mode));
    obj.insert("is_active".into(), json!(rule.is_active));
    obj.insert(MAX_CONCURRENT.into(), json!(rule.max_concurrent_tokens));
    obj.insert(MAX_TOTAL.into(), json!(rule.max_total_tokens));
    obj.insert("created_at".into(), json!(rule.created_at));
    obj.insert("updated_at".into(), json!(rule.updated_at));

    // Counters from the synced DB positions (latest paper run per rule).
    obj.insert("open_positions".into(), json!(counters.open_positions));
    obj.insert("pending_positions".into(), json!(counters.pending_positions));
    obj.insert("total_positions".into(), json!(counters.total_positions));
    obj.insert("win_count".into(), json!(counters.win_count));
    obj.insert("loss_count".into(), json!(counters.loss_count));
    obj.insert("win_rate".into(), json!(counters.win_rate));
    obj.insert("avg_pnl_pct".into(), json!(counters.avg_pnl_pct));
    obj.insert("total_pnl_sol".into(), json!(counters.total_pnl_sol));

    // Locally a rule never runs, so lifecycle is `Active` if the (snapshot-restored)
    // flag is set, `Finished` for a completed paper run, else `Idle`.
    let lifecycle = if rule.is_active {
        "Active"
    } else if paper_finished {
        "Finished"
    } else {
        "Idle"
    };
    obj.insert("lifecycle".into(), json!(lifecycle));

    Value::Object(obj)
}

/// True when `rule` is a paper rule whose latest run is `Finished` (one query).
async fn paper_finished(repo: &StrategyRepo, rule: &StrategyRule) -> bool {
    if rule.trade_mode != "paper" {
        return false;
    }
    matches!(repo.latest_run(rule.id, "paper").await, Ok(Some(run)) if run.status == "Finished")
}

// ---------------------------------------------------------------------------
// Rule Handlers (CRUD) — domain core + local wiring (no cache reload / SSE)
// ---------------------------------------------------------------------------

/// List all TPSL rules.
pub async fn list_tpsl_rules(app_state: web::Data<Arc<LocalState>>) -> impl Responder {
    let repo = app_state.strategy_repo();
    let rules = match repo.find_rules_by_strategy(STRATEGY_ID).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Failed to list TPSL rules: {e}");
            return HttpResponse::InternalServerError()
                .json(json!({"error": "Failed to list rules"}));
        }
    };
    // One query for the latest paper run per rule → finished-set (no N+1).
    let finished: std::collections::HashSet<Uuid> = repo
        .latest_runs_by_rule("paper")
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|run| run.status == "Finished")
        .filter_map(|run| run.rule_id)
        .collect();
    // One batched query for the per-rule position counters (latest paper run each).
    let counters = repo
        .rule_counters_for_latest_paper_runs(STRATEGY_ID)
        .await
        .unwrap_or_default();
    let default_counters = RuleCounters::default();
    let out: Vec<Value> = rules
        .iter()
        .map(|r| {
            rule_to_json(
                r,
                finished.contains(&r.id),
                counters.get(&r.id).unwrap_or(&default_counters),
            )
        })
        .collect();
    HttpResponse::Ok().json(out)
}

/// Get a specific TPSL rule.
pub async fn get_tpsl_rule(
    app_state: web::Data<Arc<LocalState>>,
    rule_id: web::Path<Uuid>,
) -> impl Responder {
    let repo = app_state.strategy_repo();
    let rule_id = rule_id.into_inner();
    match repo.find_rule(rule_id).await {
        Ok(Some(rule)) if rule.strategy_id == STRATEGY_ID => {
            let pf = paper_finished(&repo, &rule).await;
            let counters = repo
                .rule_counters_for_latest_paper_runs(STRATEGY_ID)
                .await
                .ok()
                .and_then(|m| m.get(&rule.id).cloned())
                .unwrap_or_default();
            HttpResponse::Ok().json(rule_to_json(&rule, pf, &counters))
        }
        Ok(_) => HttpResponse::NotFound().json(json!({"error": "Rule not found"})),
        Err(e) => {
            tracing::error!("Failed to get TPSL rule {rule_id}: {e}");
            HttpResponse::InternalServerError().json(json!({"error": "Failed to get rule"}))
        }
    }
}

/// Read the [`RuleDraft`] inputs from the flat request body (params parse off the
/// body; the universal columns are read out explicitly).
fn draft_from_body(body: &Value) -> Result<RuleDraft, HttpResponse> {
    let params = STRATEGY
        .parse_params(body)
        .map_err(|e| HttpResponse::BadRequest().json(json!({"error": format!("invalid params: {e}")})))?;
    Ok(RuleDraft {
        strategy: STRATEGY,
        rule_name: body.get(RULE_NAME).and_then(Value::as_str).unwrap_or("").to_string(),
        buy_amount_sol: body.get(BUY_AMOUNT).and_then(Value::as_f64).unwrap_or(0.0),
        trade_mode: body.get(TRADE_MODE).and_then(Value::as_str).unwrap_or("paper").to_string(),
        max_concurrent_tokens: body.get(MAX_CONCURRENT).and_then(opt_i64),
        max_total_tokens: body.get(MAX_TOTAL).and_then(opt_i64),
        params,
    })
}

/// Create a new TPSL rule (local edge: domain write only, no live-cache reload).
pub async fn create_tpsl_rule(
    app_state: web::Data<Arc<LocalState>>,
    body: web::Json<Value>,
) -> impl Responder {
    let body = body.into_inner();
    let draft = match draft_from_body(&body) {
        Ok(d) => d,
        Err(resp) => return resp,
    };
    let repo = app_state.strategy_repo();
    match rules::create(&repo, &draft).await {
        // A just-created rule has no positions yet → zero counters.
        Ok(rule) => HttpResponse::Created().json(rule_to_json(&rule, false, &RuleCounters::default())),
        Err(e) => rule_error(e, "create rule"),
    }
}

/// Update an existing TPSL rule. Local edge: no live-freeze guard (the box never
/// runs a rule) — present body keys overlay the rule's params, universal columns
/// update if present.
pub async fn update_tpsl_rule(
    app_state: web::Data<Arc<LocalState>>,
    rule_id: web::Path<Uuid>,
    body: web::Json<Value>,
) -> impl Responder {
    let rule_id = rule_id.into_inner();
    let body = body.into_inner();
    let repo = app_state.strategy_repo();

    let mut rule = match repo.find_rule(rule_id).await {
        Ok(Some(r)) if r.strategy_id == STRATEGY_ID => r,
        Ok(_) => return HttpResponse::NotFound().json(json!({"error": "Rule not found"})),
        Err(e) => {
            tracing::error!("Failed to get TPSL rule {rule_id}: {e}");
            return HttpResponse::InternalServerError().json(json!({"error": "Failed to get rule"}));
        }
    };

    // Merge present body keys onto the existing params, then re-parse (validates
    // types) and clean back to the canonical params JSONB (drops universal keys).
    let mut merged: Map<String, Value> = match &rule.params {
        Value::Object(m) => m.clone(),
        _ => Map::new(),
    };
    if let Value::Object(m) = &body {
        for (k, v) in m {
            merged.insert(k.clone(), v.clone());
        }
    }
    let params = match STRATEGY.parse_params(&Value::Object(merged)) {
        Ok(p) => p,
        Err(e) => {
            return HttpResponse::BadRequest().json(json!({"error": format!("invalid params: {e}")}))
        }
    };
    rule.params = params_to_value(&params);

    // Universal columns (apply only the keys present in the body).
    if let Value::Object(m) = &body {
        if let Some(name) = m.get(RULE_NAME).and_then(Value::as_str) {
            rule.rule_name = name.to_string();
        }
        if let Some(amount) = m.get(BUY_AMOUNT).and_then(Value::as_f64) {
            rule.buy_amount_sol = amount;
        }
        if let Some(mode) = m.get(TRADE_MODE).and_then(Value::as_str) {
            rule.trade_mode = mode.to_string();
        }
        if let Some(v) = m.get(MAX_CONCURRENT) {
            rule.max_concurrent_tokens = opt_i64(v);
        }
        if let Some(v) = m.get(MAX_TOTAL) {
            rule.max_total_tokens = opt_i64(v);
        }
    }

    match rules::save(&repo, &rule).await {
        Ok(()) => {
            let pf = paper_finished(&repo, &rule).await;
            let counters = repo
                .rule_counters_for_latest_paper_runs(STRATEGY_ID)
                .await
                .ok()
                .and_then(|m| m.get(&rule.id).cloned())
                .unwrap_or_default();
            HttpResponse::Ok().json(rule_to_json(&rule, pf, &counters))
        }
        Err(e) => rule_error(e, "update rule"),
    }
}

/// Delete a TPSL rule (local edge: domain write only).
pub async fn delete_tpsl_rule(
    app_state: web::Data<Arc<LocalState>>,
    rule_id: web::Path<Uuid>,
) -> impl Responder {
    let rule_id = rule_id.into_inner();
    let repo = app_state.strategy_repo();
    match repo.delete_rule(rule_id).await {
        Ok(()) => HttpResponse::NoContent().finish(),
        Err(e) => {
            tracing::error!("Failed to delete TPSL rule {rule_id}: {e}");
            HttpResponse::InternalServerError().json(json!({"error": "Failed to delete rule"}))
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

// ---------------------------------------------------------------------------
// Matched tokens (whole-table entry-criteria scan)
// ---------------------------------------------------------------------------

/// A matched token sent to the page. Carries the **full** shared token enrichment
/// (`#[serde(flatten)] token`) in the response body — no client-side merge — plus
/// the row-owned identity/divergent fields (`mint`/`symbol`/`ath_price`/`created_at`)
/// the shared struct excludes. Field names match the frontend's `MatchedTokenRecord`.
#[derive(Serialize)]
pub struct MatchedTokenResult {
    pub mint_address: String,
    pub symbol: String,
    /// Token creation time (from `tokens.created_at`).
    pub created_at: DateTime<Utc>,
    pub ath_price: Option<f64>,
    #[serde(flatten)]
    pub token: TokenEnrichment,
}

#[derive(Serialize)]
pub struct MatchedTokensResponse {
    pub tokens: Vec<MatchedTokenResult>,
}

/// Return one server-side page of the tokens satisfying a rule's **entry** criteria.
///
/// The match predicate is a Rust closure (`token_matches_buy_rule` + `!is_mayhem`),
/// not SQL, so we **materialize then page**: the first POST for `(rule_id, range)`
/// scans the **whole** `tokens` table (keyset-streamed via
/// [`analysis::collect_matching_tokens`], decoupled from any live cache so an old,
/// evicted-but-matching mint is still found) to get the matched **mint set** and
/// caches it on `LocalState`; this and later pages/sorts/filters re-query the DB
/// restricted to `mint = ANY(set)` — no re-scan, no 5,000-row display cap.
///
/// POST /api/strategies/tpsl1/rules/{rule_id}/matched — body is the unified
/// `TableRequest` (paging + sort/filter/search + the analysis `range`).
pub async fn get_matched_tokens(
    app_state: web::Data<Arc<LocalState>>,
    rule_id: web::Path<Uuid>,
    body: web::Json<TableRequest>,
) -> impl Responder {
    let rule_id = rule_id.into_inner();
    let req = body.into_inner();
    let (from, to) = req.range.as_ref().map_or((None, None), |r| (r.from, r.to));

    let strategy_rule = match app_state.strategy_repo().find_rule(rule_id).await {
        Ok(Some(r)) if r.strategy_id == STRATEGY_ID => r,
        Ok(_) => return HttpResponse::NotFound().json(json!({"error": "Rule not found"})),
        Err(e) => {
            tracing::error!("Failed to get rule {rule_id}: {e}");
            return HttpResponse::InternalServerError().json(json!({"error": "Failed to get rule"}));
        }
    };
    let rule = match trading_core::strategies::registry::tpsl1_decision_rule(&strategy_rule) {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("invalid tpsl1 rule params for {rule_id}: {e}");
            return HttpResponse::InternalServerError().json(json!({"error": "Invalid rule params"}));
        }
    };

    let batch_db = app_state.batch_db.clone();
    let rule_scan = rule.clone();
    let mints = match crate::strategies::matched_mints::resolve_matched_mints(
        &app_state,
        &strategy_rule,
        from,
        to,
        Box::pin(async move {
            let repo = trading_core::storage::repositories::token_repo::TokenRepo::new(batch_db);
            trading_core::strategies::analysis::collect_matching_tokens(
                &repo,
                from,
                to,
                |t| {
                    !t.is_mayhem_mode
                        && trading_core::strategies::tpsl_sniper_1::entry::token_matches_buy_rule(
                            t, &rule_scan,
                        )
                },
            )
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
        }),
    )
    .await
    {
        Ok(m) => m,
        Err(e) => {
            tracing::error!("matched-token scan failed: {e}");
            return HttpResponse::InternalServerError()
                .json(json!({"error": "Failed to build matched tokens"}));
        }
    };

    matched_page_response(app_state.strategy_repo(), mints.as_ref(), req).await
}

/// Page + count the materialized matched mint set through the shared strategy-repo
/// token query, returning the sparse projection + `X-Total-Count`. Shared by the
/// tpsl1/tpsl2 matched handlers.
pub(super) async fn matched_page_response(
    repo: trading_core::storage::repositories::strategy_repo::StrategyRepo,
    mints: &[String],
    req: TableRequest,
) -> HttpResponse {
    use trading_core::storage::repositories::strategy_repo::PositionQuery;
    let (limit, offset) = req.pagination.bounds();
    let pq = PositionQuery::from(req);
    match (
        repo.find_tokens_by_mints_paged(mints, limit, offset, &pq).await,
        repo.count_tokens_by_mints(mints, &pq).await,
    ) {
        (Ok(rows), Ok(total)) => {
            let tokens: Vec<MatchedTokenResult> = rows
                .iter()
                .map(|r| MatchedTokenResult {
                    mint_address: r.mint_address.clone(),
                    symbol: r.symbol.clone(),
                    created_at: r.token_created_at,
                    ath_price: r.ath_price,
                    token: TokenEnrichment::from(r),
                })
                .collect();
            HttpResponse::Ok()
                .insert_header(("X-Total-Count", total.to_string()))
                .insert_header(("Access-Control-Expose-Headers", "X-Total-Count"))
                .json(MatchedTokensResponse { tokens })
        }
        (Err(e), _) | (_, Err(e)) => {
            tracing::error!("matched-token page query failed: {e}");
            HttpResponse::InternalServerError().json(json!({"error": "Failed to build matched tokens"}))
        }
    }
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
    let strategy_rule = match app_state.strategy_repo().find_rule(rid).await {
        Ok(Some(r)) if r.strategy_id == STRATEGY_ID => r,
        Ok(_) => {
            return HttpResponse::NotFound().json(json!({"error": "Rule not found"}));
        }
        Err(e) => {
            tracing::error!("Failed to get rule {rid}: {e}");
            return HttpResponse::InternalServerError().json(json!({"error": "Failed to get rule"}));
        }
    };

    crate::strategies::sim_spawn::spawn_rule_simulation(
        app_state,
        rid,
        since,
        until,
        &strategy_rule,
        |state, rid, since, until, cancel, cell| {
            Box::pin(async move {
                let rows = crate::strategies::tpsl_sniper_1::run_backtest(
                    state, rid, since, until, cancel, cell,
                )
                .await?;
                crate::strategies::sim_spawn::rows_to_json(rows)
            })
        },
    )
    .await
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
// Positions (lab view: latest paper run's bag)
// ---------------------------------------------------------------------------

/// POST /api/strategies/tpsl1/rules/{rule_id}/positions
///
/// Lab twin of the live `get_positions_by_rule`: one page of the latest paper run's
/// bag (`X-Total-Count` header for the pager) via the shared `PositionResponse`. The
/// JSON body is the unified `TableRequest` (paging + server-side sort/search/filter).
pub async fn get_positions_by_rule_tpsl1(
    app_state: web::Data<Arc<LocalState>>,
    rule_id: web::Path<Uuid>,
    scope: web::Query<super::positions::ScopeParam>,
    body: web::Json<TableRequest>,
) -> impl Responder {
    let repo = app_state.strategy_repo();
    super::positions::positions_by_rule_paged(
        &repo, rule_id.into_inner(), STRATEGY_ID, scope.into_inner().scope, body.into_inner(),
    )
    .await
}

/// GET /api/strategies/tpsl1/rules/{rule_id}/positions/summary — filtered aggregates.
pub async fn get_positions_summary_tpsl1(
    app_state: web::Data<Arc<LocalState>>,
    rule_id: web::Path<Uuid>,
    scope: web::Query<super::positions::ScopeParam>,
    body: web::Json<TableRequest>,
) -> impl Responder {
    let repo = app_state.strategy_repo();
    super::positions::positions_summary_by_rule(
        &repo, rule_id.into_inner(), STRATEGY_ID, scope.into_inner().scope, body.into_inner(),
    )
    .await
}

/// POST /api/strategies/tpsl1/rules/{rule_id}/simulate/result — one server-side
/// page of the finished backtest's per-token rows (in-memory, unified TableRequest).
pub async fn get_simulate_result_tpsl1(
    app_state: web::Data<Arc<LocalState>>,
    rule_id: web::Path<Uuid>,
    body: web::Json<TableRequest>,
) -> impl Responder {
    super::positions::sim_result_page(&app_state, rule_id.into_inner(), body.into_inner())
}

/// POST /api/strategies/tpsl1/rules/{rule_id}/simulate/result/summary — filtered
/// aggregate over the finished backtest's rows for the Simulated summary card.
pub async fn get_simulate_result_summary_tpsl1(
    app_state: web::Data<Arc<LocalState>>,
    rule_id: web::Path<Uuid>,
    body: web::Json<TableRequest>,
) -> impl Responder {
    super::positions::sim_result_summary(&app_state, rule_id.into_inner(), body.into_inner())
}

// ---------------------------------------------------------------------------
// Paper-test result (over the unified strategy_runs / strategy_positions)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct PaperRunResponse {
    pub run_seq: i64,
    /// "Running" | "Finished" | "Stopped" | "Cancelled".
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
    let repo = app_state.strategy_repo();

    let rule = match repo.find_rule(rule_id).await {
        Ok(Some(r)) if r.strategy_id == STRATEGY_ID => r,
        Ok(_) => return HttpResponse::NotFound().json(json!({"error": "Rule not found"})),
        Err(e) => {
            tracing::error!("Failed to get rule {rule_id}: {e}");
            return HttpResponse::InternalServerError().json(json!({"error": "Failed to get rule"}));
        }
    };

    let run = match repo.latest_run(rule_id, "paper").await {
        Ok(run) => run,
        Err(e) => {
            tracing::error!("Failed to load paper run for {rule_id}: {e}");
            return HttpResponse::InternalServerError()
                .json(json!({"error": "Failed to load paper run"}));
        }
    };

    let Some(run) = run else {
        return HttpResponse::Ok().json(PaperResultResponse {
            rule_name: rule.rule_name,
            run: None,
            tokens: vec![],
        });
    };

    let positions = match repo
        .find_positions_by_run_paged(run.id, PAPER_RESULT_MAX_TOKENS, 0, &PositionQuery::default())
        .await
    {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("Failed to load paper positions for run {}: {e}", run.id);
            return HttpResponse::InternalServerError()
                .json(json!({"error": "Failed to load paper positions"}));
        }
    };

    let mints: Vec<String> = positions.iter().map(|p| p.mint_address.clone()).collect();
    let token_info = app_state
        .token_repo()
        .find_symbols_for(&mints)
        .await
        .unwrap_or_else(|e| {
            tracing::warn!("Failed to load token info for paper result: {e}");
            HashMap::new()
        });

    let tokens: Vec<BacktestTokenResult> = positions
        .into_iter()
        .map(|p| paper_position_to_sim_result(p, &token_info))
        .collect();

    HttpResponse::Ok().json(PaperResultResponse {
        rule_name: rule.rule_name,
        run: Some(PaperRunResponse {
            run_seq: run.run_seq,
            status: run.status,
            max_total_tokens: run.max_total_tokens.map(|v| v as u64),
            started_at: run.started_at,
            finished_at: run.finished_at,
        }),
        tokens,
    })
}

/// Clear a paper rule's recorded run history (runs + positions, via cascade).
/// Paper-only. The local box never runs a rule, so a clear always proceeds.
///
/// DELETE /api/strategies/tpsl1/rules/{rule_id}/paper-result
pub async fn clear_paper_result_tpsl_rule(
    app_state: web::Data<Arc<LocalState>>,
    rule_id: web::Path<Uuid>,
) -> impl Responder {
    let rule_id = rule_id.into_inner();
    let repo = app_state.strategy_repo();
    let rule = match repo.find_rule(rule_id).await {
        Ok(Some(r)) if r.strategy_id == STRATEGY_ID => r,
        Ok(_) => return HttpResponse::NotFound().json(json!({"error": "Rule not found"})),
        Err(e) => {
            tracing::error!("Failed to get rule {rule_id}: {e}");
            return HttpResponse::InternalServerError().json(json!({"error": "Failed to get rule"}));
        }
    };

    if rule.trade_mode != "paper" {
        return HttpResponse::BadRequest()
            .json(json!({"error": "Only paper rules have results to clear"}));
    }

    match repo.delete_runs_by_rule(rule_id, "paper").await {
        Ok(n) => {
            tracing::info!("Cleared {n} paper run(s) for rule {rule_id}");
            HttpResponse::NoContent().finish()
        }
        Err(e) => {
            tracing::error!("Failed to clear paper results for {rule_id}: {e}");
            HttpResponse::InternalServerError()
                .json(json!({"error": "Failed to clear paper results"}))
        }
    }
}

/// The exit reason a closed position carries, or `None` while still open. A
/// closed row with no stored reason (legacy) falls back to the realized-PnL sign.
fn exit_reason_or_derived(p: &StrategyPosition) -> Option<String> {
    p.exit_price?; // None → still open
    if let Some(r) = &p.exit_reason {
        return Some(r.clone());
    }
    Some(
        match p.pnl_pct() {
            Some(pct) if pct < 0.0 => "StopLoss",
            _ => "TakeProfit",
        }
        .to_string(),
    )
}

/// Map one recorded paper [`StrategyPosition`] into the simulation-shaped result
/// the shared frontend card/table renders. Pure (no DB) so it stays unit-testable.
pub(crate) fn paper_position_to_sim_result(
    p: StrategyPosition,
    token_info: &HashMap<String, (String, DateTime<Utc>)>,
) -> BacktestTokenResult {
    let pnl_percent = p.pnl_pct();
    let exit_reason = exit_reason_or_derived(&p).unwrap_or_else(|| "Open".to_string());
    // entry_token_amount is the token count bought; SOL invested = entry_price ×
    // tokens, so PnL in SOL = (entry_price × entry_token_amount) × pct/100.
    let pnl_sol = pnl_percent.and_then(|pct| match (p.entry_price, p.entry_token_amount) {
        (Some(price), Some(tokens)) => Some(price * tokens as f64 * (pct / 100.0)),
        _ => None,
    });
    let holding_secs = match (p.entry_time, p.exit_time) {
        (Some(e), Some(x)) => Some((x - e).num_seconds()),
        _ => None,
    };
    let ath_price = p
        .exit_price
        .map(|x| x.max(p.entry_price.unwrap_or(0.0)))
        .or(p.entry_price);
    let info = token_info.get(&p.mint_address);
    let symbol = info.map(|(s, _)| s.clone()).unwrap_or_default();
    let created_at = info.map(|(_, c)| *c).unwrap_or(p.created_at);
    let entry_tx = p.entry_tx_sigs().first().cloned().unwrap_or_default();
    let exit_tx = p.exit_tx_sigs().last().cloned();
    BacktestTokenResult {
        symbol,
        mint_address: p.mint_address,
        created_at,
        entry_price: p.entry_price.unwrap_or(0.0),
        ath_price,
        entry_token_amount: p.entry_token_amount.map(|a| a as f64).unwrap_or(0.0),
        entry_tx,
        entry_time: p.entry_time.unwrap_or(p.created_at),
        exit_price: p.exit_price,
        exit_tx,
        exit_time: p.exit_time,
        holding_secs,
        pnl_percent,
        pnl_sol,
        exit_reason,
        // Paper-position rows aren't backed by a batch enrichment fetch (a small,
        // one-off preview, not a full backtest).
        token: TokenEnrichment::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A closed paper position with no stored exit reason (legacy row) — entry/exit
    /// fills set directly so the derived-reason fallback is exercised.
    fn closed(entry: f64, exit: f64) -> StrategyPosition {
        let mut p = StrategyPosition::new(
            Uuid::new_v4(),
            STRATEGY_ID.to_string(),
            Uuid::new_v4(),
            "paper".to_string(),
            "mint".to_string(),
            "wallet".to_string(),
        );
        p.entry_price = Some(entry);
        p.entry_tx_signatures = json!(["etx"]);
        p.entry_token_amount = Some(1);
        p.entry_time = Some(Utc::now());
        p.exit_price = Some(exit);
        p.exit_sol = Some(0.05);
        p.exit_tx_signatures = json!(["xtx"]);
        p.exit_time = Some(Utc::now());
        p.status = "End".to_string();
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
        let mut p = StrategyPosition::new(
            Uuid::new_v4(),
            STRATEGY_ID.to_string(),
            Uuid::new_v4(),
            "paper".to_string(),
            "mint".to_string(),
            "wallet".to_string(),
        );
        p.entry_price = Some(1.0);
        p.entry_tx_signatures = json!(["etx"]);
        p.entry_token_amount = Some(1);
        p.entry_time = Some(Utc::now());
        let r = paper_position_to_sim_result(p, &HashMap::new());
        assert_eq!(r.exit_reason, "Open");
    }
}
