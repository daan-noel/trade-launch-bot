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
use trading_core::api::table_query::TableRequest;
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
///
/// SSOT NOTE: the frontend consumes this AND the sibling
/// [`trading_core::models::PositionResponse`] (the tpsl2 shape) through ONE shared
/// `RulePositionRecord` type. The two are intentionally kept as separate structs
/// (per-strategy clones), but their SERIALIZED field set must stay a consistent
/// superset — a field the FE type declares must be emitted by BOTH. If you add a
/// wire field here, add it (or its `None` placeholder) to the sibling too.
#[derive(Serialize)]
pub struct PositionResponse {
    pub id: Uuid,
    pub run_id: Uuid,
    pub mint_address: String,
    pub wallet: String,
    /// Target (trigger-trade) snapshot — the scalp-entry signal trade that armed
    /// this position, distinct from the actual entry fill. `None` for strategies
    /// that never arm (tpsl1 sniper). Emitted so the shared FE type's `target_*`
    /// keys are present on every strategy's rows (parity with the tpsl2 shape).
    pub target_price: Option<f64>,
    /// Raw token units (exact integer; the frontend scales for display).
    pub target_token_amount: Option<u64>,
    pub target_time: Option<DateTime<Utc>>,
    pub target_tx: Option<String>,
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
    /// Realized SOL PnL (`StrategyPosition::realized_pnl_sol`) — the canonical
    /// win/loss basis mirroring `positions_summary`/`is_win`.
    pub pnl_sol: Option<f64>,
    pub entry_time: Option<DateTime<Utc>>,
    pub exit_time: Option<DateTime<Utc>>,
    pub exit_reason: Option<String>,
    /// Owning run's monotonic sequence (`strategy_runs.run_seq`). Populated only by
    /// the run-history ("old runs") view — where it drives the run column + banding;
    /// `None` on the current-run/live paths (single run) and SSE deltas.
    pub run_seq: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Swing1-only: legs harvested from the live exit memo at close (see
    /// `runtime_cache::merge_swing_legs_into_extra`). `None` for tpsl1/tpsl2
    /// positions (no such key in `extra`) and for still-open swing1 positions
    /// (not harvested until the position leaves the holding index).
    pub swing_legs: Option<Vec<SwingLeg>>,
    /// Token symbol (row-owned identity; excluded from the shared `token` flatten).
    /// Empty until enriched.
    pub symbol: String,
    /// Token all-time-high price (`tokens_info`; row-owned, excluded from `token`).
    pub ath_price: Option<f64>,
    /// Full shared token enrichment (`name`, `market_cap`, `cu_price`, `trade_count`,
    /// `is_migrated`, …) — the same SSOT the Matched / Simulated / Sweep tables use,
    /// attached server-side from `strategy_positions LEFT JOIN tokens`'s mints so the
    /// positions table sorts/filters/searches on token columns with no client merge.
    /// Default (empty) on the SSE-delta / single-position paths (the client already
    /// holds the token there).
    #[serde(flatten)]
    pub token: trading_core::storage::token_enrichment::TokenEnrichment,
}

impl From<StrategyPosition> for PositionResponse {
    fn from(p: StrategyPosition) -> Self {
        let pnl_percent = p.pnl_pct();
        let pnl_sol = p.realized_pnl_sol();
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
            mint_address: p.mint_address,
            wallet: p.wallet,
            target_price: p.target_price,
            target_token_amount: p.target_token_amount,
            target_time: p.target_time,
            target_tx: p.target_tx,
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
            pnl_sol,
            entry_time: p.entry_time,
            exit_time: p.exit_time,
            exit_reason: p.exit_reason,
            // Stamped by the run-history handler from the run map; single-run views
            // leave it None.
            run_seq: None,
            created_at: p.created_at,
            updated_at: p.updated_at,
            swing_legs,
            // Enrichment is attached by the paged handler (`enrich_position_responses`);
            // default here so the SSE-delta / single-position paths stay unchanged.
            symbol: String::new(),
            ath_price: None,
            token: Default::default(),
        }
    }
}

/// Attach shared token enrichment to a page of position responses via one bounded
/// batch fetch (`token_enrichment::fetch_by_mints` over the page's mints) — the same
/// SSOT the Matched / Simulated / Sweep tables use. Sets the row-owned `symbol` /
/// `ath_price` off the row too. A fetch error is logged and leaves rows un-enriched
/// (the table still renders; enrichment columns are just blank) rather than failing
/// the whole list.
async fn enrich_position_responses(repo: &StrategyRepo, responses: &mut [PositionResponse]) {
    if responses.is_empty() {
        return;
    }
    let mints: Vec<String> = responses.iter().map(|r| r.mint_address.clone()).collect();
    match trading_core::storage::token_enrichment::fetch_by_mints(repo.pool(), &mints).await {
        Ok(rows) => {
            let by_mint: std::collections::HashMap<String, _> =
                rows.into_iter().map(|r| (r.mint_address.clone(), r)).collect();
            for r in responses.iter_mut() {
                if let Some(row) = by_mint.get(&r.mint_address) {
                    r.symbol = row.symbol.clone();
                    r.ath_price = row.ath_price;
                    r.token = row.into();
                }
            }
        }
        Err(e) => tracing::warn!("positions enrichment fetch failed: {e}"),
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
/// The `list`/`by-mint`/`by-wallet` position handlers only page (limit/offset) —
/// they don't sort/filter/search. The **rule** positions table takes the unified
/// [`TableRequest`] JSON body instead (see [`get_positions_by_rule`]).
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

/// Run-split selector for the by-rule positions + summary views.
/// - `Current` — the rule's latest run only (the "Current run" section).
/// - `History` — every prior run (all runs except the latest; the "Old runs" section).
///
/// Absent (`None`) preserves the legacy behavior (paper = latest run, real = all runs)
/// for any caller that doesn't opt into the split.
#[derive(serde::Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PositionScope {
    Current,
    History,
}

#[derive(serde::Deserialize)]
pub struct ScopeParam {
    pub scope: Option<PositionScope>,
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
    json_positions_with_total_seq(positions, total, None)
}

/// Like [`json_positions_with_total`] but stamps each row's `run_seq` from a
/// `run_id → run_seq` map — the run-history view uses it so the client can label +
/// band positions by their originating run. A `None` map leaves `run_seq` unset.
fn json_positions_with_total_seq(
    positions: Vec<StrategyPosition>,
    total: i64,
    seq_map: Option<&std::collections::HashMap<Uuid, i64>>,
) -> HttpResponse {
    let responses: Vec<PositionResponse> = positions
        .into_iter()
        .map(|p| {
            let mut r = PositionResponse::from(p);
            if let Some(map) = seq_map {
                r.run_seq = map.get(&r.run_id).copied();
            }
            r
        })
        .collect();
    HttpResponse::Ok()
        .insert_header(("X-Total-Count", total.to_string()))
        // Expose the count header to the browser fetch (needed when the SPA is
        // served through the dev proxy / a different origin).
        .insert_header(("Access-Control-Expose-Headers", "X-Total-Count"))
        .json(responses)
}

/// Build + enrich + serialize a page of positions with the pager total. Mirrors
/// [`json_positions_with_total_seq`] but attaches shared token enrichment (one batch
/// fetch) before serializing — used by the rule-positions table (the only positions
/// view with token-enrichment columns).
async fn json_positions_enriched(
    repo: &StrategyRepo,
    positions: Vec<StrategyPosition>,
    total: i64,
    seq_map: Option<&std::collections::HashMap<Uuid, i64>>,
) -> HttpResponse {
    let mut responses: Vec<PositionResponse> = positions
        .into_iter()
        .map(|p| {
            let mut r = PositionResponse::from(p);
            if let Some(map) = seq_map {
                r.run_seq = map.get(&r.run_id).copied();
            }
            r
        })
        .collect();
    enrich_position_responses(repo, &mut responses).await;
    HttpResponse::Ok()
        .insert_header(("X-Total-Count", total.to_string()))
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

/// POST /api/strategies/{strategy}/rules/{rule_id}/positions
///
/// Paper rules retain only the current run's bag, so they're served from the
/// rule's latest paper run; real rules carry their full lifetime history. The JSON
/// body is the unified `TableRequest` (paging + server-side sort/search/filter),
/// shared byte-for-byte with the lab positions endpoint.
pub async fn get_positions_by_rule(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<(String, Uuid)>,
    scope: web::Query<ScopeParam>,
    body: web::Json<TableRequest>,
) -> impl Responder {
    let (strategy, rule_id) = path.into_inner();
    if let Err(resp) = strategy_id(&strategy) {
        return resp;
    }
    let scope = scope.into_inner().scope;
    let req = body.into_inner();
    let (limit, offset) = req.pagination.bounds();
    let pq = PositionQuery::from(req);
    let repo = repo(&app_state);

    let rule = match repo.find_rule(rule_id).await {
        Ok(Some(rule)) => rule,
        Ok(None) => return json_positions(Vec::new()),
        Err(e) => return list_error("load rule", e),
    };

    // Page the rows and count the (filtered) population for the pager. Sort/search/
    // filter apply to both list + count. The scope selects which run(s):
    //   `current` — the rule's latest run only (both modes);
    //   `history` — every prior run, each row stamped with its `run_seq`;
    //   absent    — legacy: paper = latest run, real = full lifetime history.
    let (result, total, seq_map) = match scope {
        Some(PositionScope::Current) => match repo.latest_run(rule_id, &rule.trade_mode).await {
            Ok(Some(run)) => (
                repo.find_positions_by_run_paged(run.id, limit, offset, &pq).await,
                repo.count_positions_by_run(run.id, &pq).await,
                None,
            ),
            Ok(None) => return json_positions_with_total(Vec::new(), 0),
            Err(e) => return list_error("load current run", e),
        },
        Some(PositionScope::History) => {
            let runs = match repo.run_seqs_for_rule(rule_id, &rule.trade_mode).await {
                Ok(runs) => runs,
                Err(e) => return list_error("load runs", e),
            };
            // Need a current run to exclude AND at least one prior run for there to
            // be any history — otherwise the "old runs" section is empty.
            let Some(&(latest_run_id, _)) = runs.first() else {
                return json_positions_with_total(Vec::new(), 0);
            };
            if runs.len() <= 1 {
                return json_positions_with_total(Vec::new(), 0);
            }
            let seq_map: std::collections::HashMap<Uuid, i64> = runs.into_iter().collect();
            (
                repo.find_positions_by_rule_excluding_run_paged(rule_id, latest_run_id, limit, offset, &pq)
                    .await,
                repo.count_positions_by_rule_excluding_run(rule_id, latest_run_id, &pq).await,
                Some(seq_map),
            )
        }
        None if rule.trade_mode == "paper" => match repo.latest_run(rule_id, "paper").await {
            Ok(Some(run)) => (
                repo.find_positions_by_run_paged(run.id, limit, offset, &pq).await,
                repo.count_positions_by_run(run.id, &pq).await,
                None,
            ),
            Ok(None) => (Ok(Vec::new()), Ok(0), None),
            Err(e) => return list_error("load paper run", e),
        },
        None => (
            repo.find_positions_by_rule_paged(rule_id, limit, offset, &pq).await,
            repo.count_positions_by_rule(rule_id, &pq).await,
            None,
        ),
    };

    match (result, total) {
        (Ok(positions), Ok(total)) => {
            json_positions_enriched(repo, positions, total, seq_map.as_ref()).await
        }
        (Err(e), _) | (_, Err(e)) => list_error("load positions for rule", e),
    }
}

/// POST /api/strategies/{strategy}/rules/{rule_id}/positions/summary
///
/// Position aggregates for the page's **Positions Summary** panel over the same
/// filtered population its table pages (pagination/sort ignored), with the same
/// win/closed/open semantics as the per-rule runtime counters.
pub async fn get_positions_summary_by_rule(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<(String, Uuid)>,
    scope: web::Query<ScopeParam>,
    body: web::Json<TableRequest>,
) -> impl Responder {
    let (strategy, rule_id) = path.into_inner();
    if let Err(resp) = strategy_id(&strategy) {
        return resp;
    }
    let scope = scope.into_inner().scope;
    let repo = repo(&app_state);
    let pq = PositionQuery::from(body.into_inner());

    let rule = match repo.find_rule(rule_id).await {
        Ok(Some(rule)) => rule,
        Ok(None) => return HttpResponse::Ok().json(PositionsSummary::default()),
        Err(e) => return list_error("load rule", e),
    };

    // Mirror the scope semantics of `get_positions_by_rule` so the summary card
    // aggregates exactly the population its table pages.
    let result = match scope {
        Some(PositionScope::Current) => match repo.latest_run(rule_id, &rule.trade_mode).await {
            Ok(Some(run)) => repo.positions_summary_by_run(run.id, &pq).await,
            Ok(None) => Ok(PositionsSummary::default()),
            Err(e) => return list_error("load current run", e),
        },
        Some(PositionScope::History) => match repo.latest_run(rule_id, &rule.trade_mode).await {
            // Exclude the current run; a lone run yields an empty (tokens=0) summary.
            Ok(Some(run)) => repo.positions_summary_by_rule_excluding_run(rule_id, run.id, &pq).await,
            Ok(None) => Ok(PositionsSummary::default()),
            Err(e) => return list_error("load current run", e),
        },
        None if rule.trade_mode == "paper" => match repo.latest_run(rule_id, "paper").await {
            Ok(Some(run)) => repo.positions_summary_by_run(run.id, &pq).await,
            Ok(None) => Ok(PositionsSummary::default()),
            Err(e) => return list_error("load paper run", e),
        },
        None => repo.positions_summary_by_rule(rule_id, &pq).await,
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

/// GET /api/strategies/{strategy}/rules/{rule_id}/armed-history
///
/// The current run's candidates that **armed but never fired** for a rule —
/// positions that reached `Arming` (matched, watching the feed for the entry
/// trigger) and left un-entered because the trigger never fired, the arming window
/// closed, the armer cap evicted them, or the rule was stopped. Read straight from
/// the in-memory runtime cache (not the DB — these rows are deleted on drop); the
/// list resets when a fresh run starts. Oldest first.
pub async fn get_armed_history_by_rule(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<(String, Uuid)>,
) -> impl Responder {
    let (strategy, rule_id) = path.into_inner();
    if let Err(resp) = strategy_id(&strategy) {
        return resp;
    }
    let records = app_state.strategy.runtime().armed_history_by_rule(rule_id);
    HttpResponse::Ok().json(records)
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

/// POST /api/strategies/{strategy}/positions/{position_id}/close
///
/// Force-close ONE open position now — backs the per-row "Sell ALL" button on the
/// rule positions table. Unlike the raw `POST /api/solana/wallet/sell`
/// (`manual_sell`, which sells the wallet's balance by mint and never touches the
/// `StrategyPosition`), this routes through the position-aware close path so the row
/// transitions `Holding → ExitPending → closed` over the existing
/// `tpsl_positions_changed` stream — the operator sees live, reload-proof status.
/// Real rows sell on-chain in a spawned task; the response returns as soon as the
/// close has begun (202-style semantics), the terminal state arrives over SSE.
pub async fn close_position(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<(String, Uuid)>,
) -> impl Responder {
    let (strategy, position_id) = path.into_inner();
    if let Err(resp) = strategy_id(&strategy) {
        return resp;
    }
    match app_state.strategy.close_position(position_id).await {
        Ok(true) => HttpResponse::Accepted().json(serde_json::json!({ "closing": true })),
        Ok(false) => HttpResponse::NotFound()
            .json(serde_json::json!({"error": "No open position to close"})),
        Err(e) => {
            tracing::error!("Failed to close position {position_id}: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "Failed to close position"}))
        }
    }
}
