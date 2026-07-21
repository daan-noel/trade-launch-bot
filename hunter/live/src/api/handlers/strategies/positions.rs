//! Unified strategy position-read handlers.
//!
//! One set of handlers over [`StrategyRepo`] (the unified `strategy_positions`
//! table). The generic engine stamps every position with the `"generic"`
//! `strategy_id`, so the `{strategy}` path segment is retained only for URL
//! back-compat — it is ignored, and every query resolves to `"generic"`.

use actix_web::{web, HttpResponse, Responder};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

use crate::state::deploy_state::DeployState;
use trading_core::api::table_query::TableRequest;
use trading_core::models::{PositionsSummary, StrategyPosition};
use trading_core::storage::repositories::strategy_repo::{PositionQuery, StrategyRepo};

// ---------------------------------------------------------------------------
// Response type
// ---------------------------------------------------------------------------

/// Wire shape for a position read. Field set is kept stable for the frontend; the
/// JSONB signature arrays are decoded to `Vec<String>` and the single-address
/// display columns are the first entry leg / last exit leg.
///
/// SSOT NOTE: the frontend consumes this AND the sibling
/// the former core `PositionResponse` shape through ONE shared
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

/// The one `strategy_id` every generic-engine position carries. The `{strategy}`
/// path segment is ignored (kept only for URL back-compat); all queries resolve to
/// this. Mirrors `GENERIC_STRATEGY_ID` in `strategies/engine/sinks.rs`.
const GENERIC_STRATEGY_ID: &str = "generic";

fn repo(app_state: &DeployState) -> &StrategyRepo {
    &app_state.strategy_repo
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
    let (_strategy, rule_id) = path.into_inner();
    let scope = scope.into_inner().scope;
    let req = body.into_inner();
    let (limit, offset) = req.pagination.bounds();
    let pq = PositionQuery::from(req);
    let repo = repo(&app_state);

    // The rule's trade_mode drives run selection. Rules live in the generic
    // `strategy_rules` table (RuleRepo), not the retired legacy table.
    let rule = match app_state.rule_repo.find(rule_id).await {
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
    let (_strategy, rule_id) = path.into_inner();
    let scope = scope.into_inner().scope;
    let repo = repo(&app_state);
    let pq = PositionQuery::from(body.into_inner());

    // Rules live in the generic `strategy_rules` table (RuleRepo).
    let rule = match app_state.rule_repo.find(rule_id).await {
        Ok(Some(rule)) => rule,
        Ok(None) => return HttpResponse::Ok().json(PositionsSummary::default()),
        Err(e) => return list_error("load rule", e),
    };

    // Marks the still-open positions to market for `open_pnl_sol` — an in-memory
    // cache read per open position (bounded by the rule's concurrency cap), no DB
    // or RPC round-trip. `None` for a token with no price yet leaves that position
    // out of the mark rather than inventing one.
    let token_cache = app_state.token_cache.clone();
    let price_of = |mint: &str| -> Option<f64> {
        token_cache
            .get(mint)
            .and_then(|e| e.value().current_price)
            .filter(|p| p.is_finite() && *p > 0.0)
    };

    // Mirror the scope semantics of `get_positions_by_rule` so the summary card
    // aggregates exactly the population its table pages.
    let result = match scope {
        Some(PositionScope::Current) => match repo.latest_run(rule_id, &rule.trade_mode).await {
            Ok(Some(run)) => repo.positions_summary_by_run(run.id, &pq, price_of).await,
            Ok(None) => Ok(PositionsSummary::default()),
            Err(e) => return list_error("load current run", e),
        },
        Some(PositionScope::History) => match repo.latest_run(rule_id, &rule.trade_mode).await {
            // Exclude the current run; a lone run yields an empty (tokens=0) summary.
            Ok(Some(run)) => {
                repo.positions_summary_by_rule_excluding_run(rule_id, run.id, &pq, price_of).await
            }
            Ok(None) => Ok(PositionsSummary::default()),
            Err(e) => return list_error("load current run", e),
        },
        None if rule.trade_mode == "paper" => match repo.latest_run(rule_id, "paper").await {
            Ok(Some(run)) => repo.positions_summary_by_run(run.id, &pq, price_of).await,
            Ok(None) => Ok(PositionsSummary::default()),
            Err(e) => return list_error("load paper run", e),
        },
        None => repo.positions_summary_by_rule(rule_id, &pq, price_of).await,
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
    let _strategy = path.into_inner();
    let sid = GENERIC_STRATEGY_ID;
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
    let (_strategy, mint) = path.into_inner();
    let sid = GENERIC_STRATEGY_ID;
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
    let (_strategy, wallet) = path.into_inner();
    let sid = GENERIC_STRATEGY_ID;
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
    let (_strategy, position_id) = path.into_inner();
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
/// Force-close ONE open position now — backs the per-row "Sell ALL" button on Ops.
/// Dual path (Helius-cheap):
/// 1. Live engine registry hit → `ManualClose` (SSE Holding→ExitPending→…).
/// 2. Registry miss (post-restart orphan) → direct `orphan_exit` sell via the same
///    `run_exit` feed-confirm path, **or** PG book-close when `trades` net already
///    shows the bag cleared (no sell RPC).
/// Never returns 202 on a silent no-op.
pub async fn close_position(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<(String, Uuid)>,
) -> impl Responder {
    use crate::strategies::engine::orphan_exit::{self, OrphanStart, BAG_CLEARED_THRESHOLD_RAW};

    let (_strategy, position_id) = path.into_inner();
    let pos = match app_state.strategy_repo.find_position(position_id).await {
        Ok(Some(p)) => p,
        Ok(None) => {
            return HttpResponse::NotFound()
                .json(serde_json::json!({"error": "Position not found"}));
        }
        Err(e) => {
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": format!("Failed to load position: {e}")}));
        }
    };

    if pos.mode != "real" {
        return HttpResponse::Conflict()
            .json(serde_json::json!({"error": "Only real positions can be sold on-chain"}));
    }

    let status = pos.status.as_str();
    let retry_failed = status == "ExitFailed";
    if matches!(status, "End" | "ExitUnconfirmed")
        || (status == "ExitFailed" && {
            // ExitFailed with bag already gone → just book if needed; with bag → retry.
            let wallet = app_state.trader.wallet_pubkey();
            matches!(
                app_state
                    .trade_repo()
                    .net_token_amount_by_wallet_and_mint(&wallet, &pos.mint_address)
                    .await,
                Ok(net) if net <= BAG_CLEARED_THRESHOLD_RAW
            )
        })
    {
        if status == "ExitFailed" {
            // Bag cleared — heal the row without a sell.
            let wallet = app_state.trader.wallet_pubkey();
            let fill = orphan_exit::fill_from_latest_sell(
                &app_state.trade_repo(),
                &wallet,
                &pos,
            )
            .await;
            let deps = orphan_deps_from_state(&app_state);
            let _ = orphan_exit::book_externally_cleared(&deps, &pos, fill).await;
            return HttpResponse::Ok().json(serde_json::json!({ "closed": true }));
        }
        return HttpResponse::Conflict().json(serde_json::json!({
            "error": format!("Position is already terminal ({status})")
        }));
    }

    if !matches!(status, "Holding" | "ExitPending" | "ExitFailed") {
        return HttpResponse::Conflict().json(serde_json::json!({
            "error": format!("Cannot sell from status {status}")
        }));
    }

    // Registry hit → engine ManualClose (live path).
    if app_state.positions.engine_id(position_id).is_some() && !retry_failed {
        if app_state.engine.manual_close(position_id).await {
            return HttpResponse::Accepted().json(serde_json::json!({ "closing": true }));
        }
        return HttpResponse::InternalServerError()
            .json(serde_json::json!({"error": "Engine shutting down"}));
    }

    // Bag already gone (Trade sell / sibling) — book closed, no sell RPC.
    let wallet = app_state.trader.wallet_pubkey();
    if let Ok(net) = app_state
        .trade_repo()
        .net_token_amount_by_wallet_and_mint(&wallet, &pos.mint_address)
        .await
    {
        if net <= BAG_CLEARED_THRESHOLD_RAW {
            let fill =
                orphan_exit::fill_from_latest_sell(&app_state.trade_repo(), &wallet, &pos).await;
            let deps = orphan_deps_from_state(&app_state);
            match orphan_exit::book_externally_cleared(&deps, &pos, fill).await {
                Ok(()) => {
                    return HttpResponse::Ok().json(serde_json::json!({ "closed": true }));
                }
                Err(e) => {
                    return HttpResponse::InternalServerError().json(serde_json::json!({
                        "error": format!("Failed to book cleared position: {e}")
                    }));
                }
            }
        }
    }

    // Orphan / ExitFailed retry — direct sell (feed confirm), never silent 202.
    let deps = orphan_deps_from_state(&app_state);
    match orphan_exit::spawn_orphan_sell(&deps, pos, "Manual") {
        OrphanStart::Started => {
            HttpResponse::Accepted().json(serde_json::json!({ "closing": true }))
        }
        OrphanStart::Busy => HttpResponse::Conflict().json(serde_json::json!({
            "error": "Exit already in flight for this position or mint"
        })),
        OrphanStart::NothingToSell => HttpResponse::Conflict().json(serde_json::json!({
            "error": "Position has zero token amount to sell"
        })),
    }
}

fn orphan_deps_from_state(app_state: &DeployState) -> crate::strategies::engine::orphan_exit::OrphanExitDeps {
    use crate::strategies::engine::orphan_exit::OrphanExitDeps;
    OrphanExitDeps {
        strategy_repo: app_state.strategy_repo.clone(),
        trade_repo: app_state.trade_repo(),
        trader: app_state.trader.clone(),
        token_cache: app_state.token_cache.clone(),
        trade_signals: app_state.trade_signals.clone(),
        inflight: app_state.inflight.clone(),
        registry: app_state.positions.clone(),
        fill_tx: app_state.engine_fill_tx.clone(),
        settings: app_state.settings.subscribe(),
    }
}
