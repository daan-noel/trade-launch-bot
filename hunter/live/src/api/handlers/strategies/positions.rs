//! Unified strategy position-read handlers.
//!
//! One set of handlers over [`StrategyRepo`] (the unified `strategy_positions`
//! table). The generic engine stamps every position with the `"generic"`
//! `strategy_id`, so the `{strategy}` path segment is retained only for URL
//! back-compat — it is ignored, and every query resolves to `"generic"`.
//!
//! The **by-rule** reads (page + summary) are thin adapters over
//! [`trading_core::api::handlers::strategies::rule_positions`] — the lab bin serves
//! the same two endpoints off the synced mirror, so the run-scope semantics and the
//! wire shape live in core and can't drift between the bins. Everything else here
//! (list / by-mint / by-wallet / single / close) is deploy-only.

use actix_web::{web, HttpResponse, Responder};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

use hunter_engine::event::Fill;

use crate::state::deploy_state::DeployState;
use trading_core::api::handlers::strategies::rule_positions;
use trading_core::api::table_query::TableRequest;
use trading_core::models::StrategyPosition;
use trading_core::storage::repositories::strategy_repo::StrategyRepo;

pub use rule_positions::{PositionResponse, PositionScope, ScopeParam};

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

fn list_error(what: &str, e: anyhow::Error) -> HttpResponse {
    tracing::error!("Failed to {what}: {e}");
    HttpResponse::InternalServerError().json(serde_json::json!({"error": "Failed to load positions"}))
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// POST /api/strategies/{strategy}/rules/{rule_id}/positions
///
/// Thin adapter over the shared core read — see
/// [`rule_positions::rule_positions_page`] for the run-scope semantics. The lab bin
/// serves the identical endpoint off the synced mirror.
pub async fn get_positions_by_rule(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<(String, Uuid)>,
    scope: web::Query<ScopeParam>,
    body: web::Json<TableRequest>,
) -> impl Responder {
    let (_strategy, rule_id) = path.into_inner();
    let ScopeParam { scope, run_seq } = scope.into_inner();
    rule_positions::rule_positions_page(
        repo(&app_state),
        &app_state.rule_repo,
        rule_id,
        scope,
        run_seq,
        body.into_inner(),
    )
    .await
}

/// POST /api/strategies/{strategy}/rules/{rule_id}/positions/summary
///
/// Thin adapter over the shared core read. Open positions are marked to market from
/// the live in-memory token cache (no DB or RPC round-trip).
pub async fn get_positions_summary_by_rule(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<(String, Uuid)>,
    scope: web::Query<ScopeParam>,
    body: web::Json<TableRequest>,
) -> impl Responder {
    let (_strategy, rule_id) = path.into_inner();
    let ScopeParam { scope, run_seq } = scope.into_inner();
    let token_cache = app_state.token_cache.clone();
    let price_of = |mint: &str| -> Option<f64> {
        token_cache
            .get(mint)
            .and_then(|e| e.value().current_price)
            .filter(|p| p.is_finite() && *p > 0.0)
    };
    rule_positions::rule_positions_summary(
        repo(&app_state),
        &app_state.rule_repo,
        rule_id,
        scope,
        run_seq,
        body.into_inner(),
        price_of,
    )
    .await
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

/// `?action=` selector for [`close_position`]:
/// - `retry` (default) — normal sell at the configured slippage.
/// - `dump` — force sell with NO slippage floor (accept dust); for a rugged /
///   near-drained pool where the floor reverts every attempt.
/// - `writeoff` — terminally book a **parked** `ExitFailed` position `Dead`
///   (full loss) WITHOUT an on-chain sell, for a pool with no sellable liquidity.
#[derive(Debug, Deserialize)]
pub struct CloseParams {
    #[serde(default)]
    pub action: Option<String>,
}

/// POST /api/strategies/{strategy}/positions/{position_id}/close
///
/// Force-close ONE open position now — backs the per-row "Sell ALL" button on Ops.
/// Dual path (Helius-cheap):
/// 1. Live engine registry hit → `ManualClose` (SSE Holding→ExitPending→…).
/// 2. Registry miss (post-restart orphan) → direct `orphan_exit` sell via the same
///    `run_exit` feed-confirm path, **or** PG book-close when `trades` net already
///    shows the bag cleared (no sell RPC).
///
/// Never returns 202 on a silent no-op.
pub async fn close_position(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<(String, Uuid)>,
    query: web::Query<CloseParams>,
) -> impl Responder {
    use crate::strategies::engine::orphan_exit::{self, OrphanStart, BAG_CLEARED_THRESHOLD_RAW};

    let (_strategy, position_id) = path.into_inner();
    let action = query.action.as_deref().unwrap_or("retry");
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

    // Write-off: terminally book a parked/failed exit `Dead` (full loss) with NO
    // on-chain sell — for a rugged pool with no sellable liquidity. Restricted to
    // `ExitFailed` so it can never race a live engine arm (those are never
    // ExitFailed). Manual-only; the reaper never writes a position off.
    if action == "writeoff" {
        if pos.status != "ExitFailed" {
            return HttpResponse::Conflict().json(serde_json::json!({
                "error": format!(
                    "write-off applies only to a failed/parked exit (status ExitFailed), not {}",
                    pos.status
                )
            }));
        }
        let fill = Fill {
            price: 0.0,
            sol: 0.0,
            token_amount: pos.entry_token_amount.unwrap_or(0),
            at: chrono::Utc::now(),
        };
        return match orphan_exit::book_externally_cleared_pg(
            &app_state.strategy_repo,
            pos.id,
            fill,
            "Dead",
        )
        .await
        {
            Ok(()) => HttpResponse::Ok().json(serde_json::json!({ "written_off": true })),
            Err(e) => HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": format!("write-off failed: {e}")})),
        };
    }
    let dump = action == "dump";

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
    // `dump` forces no-floor slippage so a near-drained pool still clears.
    let deps = orphan_deps_from_state(&app_state);
    match orphan_exit::spawn_orphan_sell(&deps, pos, "Manual", dump) {
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
