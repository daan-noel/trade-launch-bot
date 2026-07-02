//! Unified strategy position-read handlers.
//!
//! One set of handlers over [`StrategyRepo`] (the unified `strategy_positions`
//! table), keyed by a `{strategy}` path segment. The legacy per-strategy URLs
//! (`/strategies/tpsl1/...`, `/strategies/tpsl2/...`) stay valid — the segment
//! maps to a canonical `strategy_id` via [`StrategyImpl::from_id`], which accepts
//! both the short alias (`tpsl1`) and the canonical id (`tpsl_sniper_1`).

use actix_web::{web, HttpResponse, Responder};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

use crate::state::deploy_state::DeployState;
use trading_core::api::table_query::{FilterOp, FilterSpec};
use trading_core::models::{PositionsSummary, StrategyPosition};
use trading_core::storage::repositories::strategy_repo::{PositionQuery, StrategyRepo};
use trading_core::strategies::registry::StrategyImpl;
use trading_core::strategies::swing_1::swing::SwingLeg;

// ---------------------------------------------------------------------------
// Response type
// ---------------------------------------------------------------------------

/// Wire shape for a position read. Field set is kept stable for the frontend; the
/// JSONB signature arrays are decoded to `Vec<String>` and the single-address
/// display columns are the first entry leg / last exit leg.
#[derive(Serialize)]
pub struct PositionResponse {
    pub id: Uuid,
    pub run_id: Uuid,
    pub mint: String,
    pub wallet: String,
    pub entry_price: Option<f64>,
    pub exit_price: Option<f64>,
    /// First entry leg's signature (display/back-compat); empty until the fill is
    /// adopted. The full per-leg list is `entry_tx_signatures`.
    pub entry_tx: String,
    /// Last exit leg's signature (display/back-compat); `None` until a sell lands.
    /// The full per-leg list is `exit_tx_signatures`.
    pub exit_tx: Option<String>,
    pub entry_tx_signatures: Vec<String>,
    pub exit_tx_signatures: Vec<String>,
    pub status: String,
    pub strategy: String,
    /// Owning rule (`None` if the rule was deleted — `ON DELETE SET NULL`).
    pub rule_id: Option<Uuid>,
    /// Raw token units (exact integer; the frontend scales for display).
    pub entry_token_amount: Option<u64>,
    /// Raw token units (exact integer; the frontend scales for display).
    pub exit_token_amount: Option<u64>,
    pub pnl_percent: Option<f64>,
    pub entry_time: Option<DateTime<Utc>>,
    pub exit_time: Option<DateTime<Utc>>,
    pub exit_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Swing1-only: legs harvested from the live exit memo at close (see
    /// `runtime_cache::merge_swing_legs_into_extra`). `None` for tpsl1/tpsl2
    /// positions (no such key in `extra`) and for still-open swing1 positions
    /// (not harvested until the position leaves the holding index).
    pub swing_legs: Option<Vec<SwingLeg>>,
}

impl From<StrategyPosition> for PositionResponse {
    fn from(p: StrategyPosition) -> Self {
        let pnl_percent = p.pnl_pct();
        let entry_sigs = p.entry_tx_sigs();
        let exit_sigs = p.exit_tx_sigs();
        // Missing key or malformed JSON just means "no legs" (tpsl1/tpsl2 rows, or
        // a swing1 row that hasn't closed yet) — never an error.
        let swing_legs = p
            .extra
            .get("swing_legs")
            .and_then(|v| serde_json::from_value::<Vec<SwingLeg>>(v.clone()).ok());
        Self {
            id: p.id,
            run_id: p.run_id,
            mint: p.mint,
            wallet: p.wallet,
            entry_price: p.entry_price,
            exit_price: p.exit_price,
            entry_tx: entry_sigs.first().cloned().unwrap_or_default(),
            exit_tx: exit_sigs.last().cloned(),
            entry_tx_signatures: entry_sigs,
            exit_tx_signatures: exit_sigs,
            status: p.status,
            strategy: p.strategy_id,
            rule_id: p.rule_id,
            entry_token_amount: p.entry_token_amount,
            exit_token_amount: p.exit_token_amount,
            pnl_percent,
            entry_time: p.entry_time,
            exit_time: p.exit_time,
            exit_reason: p.exit_reason,
            created_at: p.created_at,
            updated_at: p.updated_at,
            swing_legs,
        }
    }
}

/// Query params for the list views. Bounds every list query so a growing
/// `strategy_positions` table can't be fetched whole in one request.
///
/// `sort`, `q` (search), and `filter` carry the table's server-side view-state:
///   - `sort` = comma-separated `key:dir` (`entry_time:desc,pnl_pct:asc`);
///   - `q`    = free-text search (mint / symbol / name);
///   - `filter` = `|`-separated `key:value` per-column filters (`status:End|mint:abc`).
///
/// The repo whitelist drops any non-sortable/filterable key, so unknown/enrichment
/// keys are simply ignored (never interpolated).
#[derive(serde::Deserialize)]
pub struct PositionListParams {
    #[serde(default = "default_positions_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
    #[serde(default)]
    pub sort: String,
    #[serde(default)]
    pub q: String,
    #[serde(default)]
    pub filter: String,
}

fn default_positions_limit() -> i64 {
    200
}

impl PositionListParams {
    /// Clamp to a sane window: limit in 1..=1000, offset >= 0.
    fn bounds(&self) -> (i64, i64) {
        (self.limit.clamp(1, 1000), self.offset.max(0))
    }

    /// Parse the sort/search/filter fields into a repo [`PositionQuery`].
    pub fn to_query(&self) -> PositionQuery {
        let sort = self
            .sort
            .split(',')
            .filter_map(|part| {
                let (key, dir) = part.split_once(':')?;
                let key = key.trim();
                if key.is_empty() {
                    return None;
                }
                Some((key.to_string(), dir.trim().eq_ignore_ascii_case("desc")))
            })
            .collect();
        let filters = self
            .filter
            .split('|')
            .filter_map(|part| {
                let (key, val) = part.split_once(':')?;
                let key = key.trim();
                if key.is_empty() || val.trim().is_empty() {
                    return None;
                }
                // Live keeps the flat GET query-string contract: every per-column
                // filter is a substring match, so lower each into a `Contains`
                // `FilterSpec` (the only op the SQL builder honors on text cols).
                let spec = FilterSpec {
                    op: FilterOp::Contains,
                    val: serde_json::Value::String(val.trim().to_string()),
                    ..Default::default()
                };
                Some((key.to_string(), spec))
            })
            .collect();
        PositionQuery { search: self.q.clone(), filters, sort }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Resolve the `{strategy}` path segment to its canonical `strategy_id`, or a
/// `400` if it names no known strategy.
fn strategy_id(seg: &str) -> Result<&'static str, HttpResponse> {
    StrategyImpl::from_id(seg).map(|s| s.id()).ok_or_else(|| {
        HttpResponse::BadRequest().json(serde_json::json!({"error": "Unknown strategy"}))
    })
}

fn repo(app_state: &DeployState) -> &StrategyRepo {
    app_state.strategy.repo()
}

fn json_positions(positions: Vec<StrategyPosition>) -> HttpResponse {
    let responses: Vec<PositionResponse> =
        positions.into_iter().map(PositionResponse::from).collect();
    HttpResponse::Ok().json(responses)
}

/// Like [`json_positions`] but also stamps the run/rule-wide total on an
/// `X-Total-Count` header so the client pager can size itself without the JSON
/// body shape changing (the array-of-positions contract is preserved).
fn json_positions_with_total(positions: Vec<StrategyPosition>, total: i64) -> HttpResponse {
    let responses: Vec<PositionResponse> =
        positions.into_iter().map(PositionResponse::from).collect();
    HttpResponse::Ok()
        .insert_header(("X-Total-Count", total.to_string()))
        // Expose the count header to the browser fetch (needed when the SPA is
        // served through the dev proxy / a different origin).
        .insert_header(("Access-Control-Expose-Headers", "X-Total-Count"))
        .json(responses)
}

fn list_error(what: &str, e: anyhow::Error) -> HttpResponse {
    tracing::error!("Failed to {what}: {e}");
    HttpResponse::InternalServerError().json(serde_json::json!({"error": "Failed to load positions"}))
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/strategies/{strategy}/rules/{rule_id}/positions
///
/// Paper rules retain only the current run's bag, so they're served from the
/// rule's latest paper run; real rules carry their full lifetime history.
pub async fn get_positions_by_rule(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<(String, Uuid)>,
    query: web::Query<PositionListParams>,
) -> impl Responder {
    let (strategy, rule_id) = path.into_inner();
    if let Err(resp) = strategy_id(&strategy) {
        return resp;
    }
    let (limit, offset) = query.bounds();
    let pq = query.to_query();
    let repo = repo(&app_state);

    let rule = match repo.find_rule(rule_id).await {
        Ok(Some(rule)) => rule,
        Ok(None) => return json_positions(Vec::new()),
        Err(e) => return list_error("load rule", e),
    };

    // Page the rows and count the (filtered) population for the pager. Paper rules
    // page their latest run (they retain only the current run's bag); real rules page
    // their full lifetime history. Sort/search/filter apply to both list + count.
    let (result, total) = if rule.trade_mode == "paper" {
        match repo.latest_run(rule_id, "paper").await {
            Ok(Some(run)) => (
                repo.find_positions_by_run_paged(run.id, limit, offset, &pq).await,
                repo.count_positions_by_run(run.id, &pq).await,
            ),
            Ok(None) => (Ok(Vec::new()), Ok(0)),
            Err(e) => return list_error("load paper run", e),
        }
    } else {
        (
            repo.find_positions_by_rule_paged(rule_id, limit, offset, &pq).await,
            repo.count_positions_by_rule(rule_id, &pq).await,
        )
    };

    match (result, total) {
        (Ok(positions), Ok(total)) => json_positions_with_total(positions, total),
        (Err(e), _) | (_, Err(e)) => list_error("load positions for rule", e),
    }
}

/// GET /api/strategies/{strategy}/rules/{rule_id}/positions/summary
///
/// Run/rule-wide position aggregates for the page's **Positions Summary** panel,
/// computed server-side in SQL over the *entire* population (never a page) with the
/// same win/closed/open semantics as the per-rule runtime counters — so the summary
/// panel and the strategy-table row always agree. Paper rules aggregate their latest
/// run; real rules aggregate all their positions.
pub async fn get_positions_summary_by_rule(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<(String, Uuid)>,
) -> impl Responder {
    let (strategy, rule_id) = path.into_inner();
    if let Err(resp) = strategy_id(&strategy) {
        return resp;
    }
    let repo = repo(&app_state);

    let rule = match repo.find_rule(rule_id).await {
        Ok(Some(rule)) => rule,
        Ok(None) => return HttpResponse::Ok().json(PositionsSummary::default()),
        Err(e) => return list_error("load rule", e),
    };

    let result = if rule.trade_mode == "paper" {
        match repo.latest_run(rule_id, "paper").await {
            Ok(Some(run)) => repo.positions_summary_by_run(run.id).await,
            Ok(None) => Ok(PositionsSummary::default()),
            Err(e) => return list_error("load paper run", e),
        }
    } else {
        repo.positions_summary_by_rule(rule_id).await
    };

    match result {
        Ok(summary) => HttpResponse::Ok().json(summary),
        Err(e) => list_error("load positions summary", e),
    }
}

/// GET /api/strategies/{strategy}/positions
pub async fn list_positions(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<String>,
    query: web::Query<PositionListParams>,
) -> impl Responder {
    let strategy = path.into_inner();
    let sid = match strategy_id(&strategy) {
        Ok(sid) => sid,
        Err(resp) => return resp,
    };
    let (limit, offset) = query.bounds();
    match repo(&app_state).find_positions_by_strategy(sid, limit, offset).await {
        Ok(positions) => json_positions(positions),
        Err(e) => list_error("list positions", e),
    }
}

/// GET /api/strategies/{strategy}/positions/mint/{mint}
pub async fn get_positions_by_mint(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<(String, String)>,
    query: web::Query<PositionListParams>,
) -> impl Responder {
    let (strategy, mint) = path.into_inner();
    let sid = match strategy_id(&strategy) {
        Ok(sid) => sid,
        Err(resp) => return resp,
    };
    let (limit, offset) = query.bounds();
    match repo(&app_state).find_holding_by_mint(sid, &mint, limit, offset).await {
        Ok(positions) => json_positions(positions),
        Err(e) => list_error("load positions for mint", e),
    }
}

/// GET /api/strategies/{strategy}/positions/wallet/{wallet}
pub async fn get_positions_by_wallet(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<(String, String)>,
    query: web::Query<PositionListParams>,
) -> impl Responder {
    let (strategy, wallet) = path.into_inner();
    let sid = match strategy_id(&strategy) {
        Ok(sid) => sid,
        Err(resp) => return resp,
    };
    let (limit, offset) = query.bounds();
    match repo(&app_state).find_holding_by_wallet(sid, &wallet, limit, offset).await {
        Ok(positions) => json_positions(positions),
        Err(e) => list_error("load positions for wallet", e),
    }
}

/// GET /api/strategies/{strategy}/positions/{position_id}
pub async fn get_position(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<(String, Uuid)>,
) -> impl Responder {
    let (strategy, position_id) = path.into_inner();
    if let Err(resp) = strategy_id(&strategy) {
        return resp;
    }
    match repo(&app_state).find_position(position_id).await {
        Ok(Some(position)) => HttpResponse::Ok().json(PositionResponse::from(position)),
        Ok(None) => HttpResponse::NotFound().json(serde_json::json!({"error": "Position not found"})),
        Err(e) => {
            tracing::error!("Failed to get position {position_id}: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "Failed to get position"}))
        }
    }
}
