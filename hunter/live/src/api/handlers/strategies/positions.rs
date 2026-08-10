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

use hunter_engine::event::{Fill, ManualExit};

use crate::state::deploy_state::DeployState;
use trading_core::api::handlers::strategies::rule_positions;
use trading_core::api::table_query::TableRequest;
use trading_core::config::constants::{sol_to_lamports, MAX_MANUAL_BUY_SOL};
use trading_core::models::wallet::validate_solana_address;
use trading_core::models::StrategyPosition;
use trading_core::storage::repositories::strategy_repo::StrategyRepo;

pub use rule_positions::{EpisodeModeParam, PositionResponse, PositionScope, ScopeParam};

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

/// GET /api/strategies/{strategy}/positions/mint/{mint}/episodes
///
/// Every entered episode on the mint (re-entry history) for the token chart's
/// marker overlay. Thin adapter over the shared core read — the lab bin serves the
/// same endpoint off the synced mirror.
pub async fn get_mint_episodes(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<(String, String)>,
    query: web::Query<EpisodeModeParam>,
) -> impl Responder {
    let (_strategy, mint) = path.into_inner();
    rule_positions::mint_episodes(repo(&app_state), &mint, query.mode.as_deref()).await
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

/// GET /api/strategies/{strategy}/positions/{position_id}/fills
///
/// Append-only fill ledger (entry + every sell leg) for the Console / Evidence
/// dialog. Thin adapter over the shared core read.
pub async fn get_position_fills(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<(String, Uuid)>,
) -> impl Responder {
    let (_strategy, position_id) = path.into_inner();
    rule_positions::position_fills(repo(&app_state), position_id).await
}

/// `?action=` selector for [`close_position`]. Legality matrix (backend-enforced;
/// status alone determines what is legal — the UI renders, never infers):
///
/// | Status            | retry/sell | dump | writeoff | verify |
/// |-------------------|------------|------|----------|--------|
/// | Holding           | yes        | —    | —        | —      |
/// | ExitPending       | — (busy)   | —    | —        | —      |
/// | ExitStuck         | yes (un-parks) | yes | yes   | —      |
/// | ExitUnconfirmed   | yes (re-sell, safe: a landed sell books NothingToSell→cleared) | yes | yes | yes |
/// | BuySubmitted      | —          | —    | —        | yes (adopt-or-drop) |
/// | End / EntryFailed | —          | —    | —        | —      |
///
/// - `retry` (default) — normal sell at the configured slippage.
/// - `dump` — force sell with NO slippage floor (accept dust); for a rugged /
///   near-drained pool where the floor reverts every attempt.
/// - `writeoff` — terminally book the position `Dead` (full loss) WITHOUT an
///   on-chain sell, for a pool with no sellable liquidity. Manual-only, forever.
/// - `verify` — on-demand resolve: `ExitUnconfirmed` → PG-net check (heal to End
///   or report still-held); `BuySubmitted` → the reaper's adopt-or-drop logic.
///
/// Optional `sell_bps` (query): basis points of the **initial** bag to sell
/// (`1..=9900` ⇒ partial / Console "Sell N%"; omit or `10000` ⇒ Sell ALL).
/// Partials are Holding + live-engine only — orphan / stuck / dump stay full.
#[derive(Debug, Deserialize)]
pub struct CloseParams {
    #[serde(default)]
    pub action: Option<String>,
    /// `None` / `10000` ⇒ `Portion::All`; `1..=9900` ⇒ `BpsOfInitial`.
    #[serde(default)]
    pub sell_bps: Option<u16>,
}

/// POST /api/strategies/{strategy}/positions/{position_id}/close
///
/// Force-close / resolve ONE open position now — backs the Console row actions.
/// Dual path (Helius-cheap):
/// 1. Live engine registry hit → `ManualClose` (SSE Holding→ExitPending→…).
/// 2. Registry miss (post-restart orphan / stuck / unconfirmed) → direct
///    `orphan_exit` sell via the same `run_exit` feed-confirm path, **or** PG
///    book-close when `trades` net already shows the bag cleared (no sell RPC).
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
    // Map optional sell_bps → Portion. Omit / 10000 = All; 1..=9900 = partial.
    // Reject anything else before touching the engine / orphan path.
    let portion = match query.sell_bps {
        None | Some(10_000) => hunter_engine::event::Portion::All,
        Some(bps) if (1..=hunter_engine::rule_params::MAX_SCALE_SELL_BPS).contains(&bps) => {
            hunter_engine::event::Portion::BpsOfInitial(bps)
        }
        Some(bps) => {
            return HttpResponse::BadRequest().json(serde_json::json!({
                "error": format!(
                    "sell_bps must be 1..={} (partial) or 10000/omit (all), got {bps}",
                    hunter_engine::rule_params::MAX_SCALE_SELL_BPS
                )
            }));
        }
    };
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
    if matches!(status, "End" | "EntryFailed") {
        return HttpResponse::Conflict().json(serde_json::json!({
            "error": format!("Position is already terminal ({status})")
        }));
    }

    match action {
        // Write-off: terminally book the bag `Dead` (full loss) with NO on-chain
        // sell — for a rugged pool with no sellable liquidity. Allowed on the two
        // attention statuses only, so it can never race a live engine arm.
        // Manual-only; the reaper never writes a position off.
        "writeoff" => {
            if portion.is_partial() {
                return HttpResponse::BadRequest().json(serde_json::json!({
                    "error": "write-off has no sell — omit sell_bps"
                }));
            }
            if !matches!(status, "ExitStuck" | "ExitUnconfirmed") {
                return HttpResponse::Conflict().json(serde_json::json!({
                    "error": format!(
                        "write-off applies only to a stuck/unconfirmed exit, not {status}"
                    )
                }));
            }
            let fill = Fill {
                price: 0.0,
                sol: 0.0,
                token_amount: pos.entry_token_amount.unwrap_or(0),
                at: chrono::Utc::now(),
            };
            match orphan_exit::book_externally_cleared_pg(
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
            }
        }

        // Verify: on-demand resolve, no sell.
        "verify" => {
            if portion.is_partial() {
                return HttpResponse::BadRequest().json(serde_json::json!({
                    "error": "verify has no sell — omit sell_bps"
                }));
            }
            verify_position(&app_state, pos).await
        },

        "retry" | "dump" => {
            let dump = action == "dump";
            if dump && !matches!(status, "ExitStuck" | "ExitUnconfirmed") {
                return HttpResponse::Conflict().json(serde_json::json!({
                    "error": format!("dump applies only to a stuck/unconfirmed exit, not {status}")
                }));
            }
            if dump && portion.is_partial() {
                return HttpResponse::BadRequest().json(serde_json::json!({
                    "error": "dump always sells the full remaining bag — omit sell_bps"
                }));
            }
            if status == "ExitPending" {
                return HttpResponse::Conflict().json(serde_json::json!({
                    "error": "Exit already in flight (ExitPending)"
                }));
            }
            if status == "BuySubmitted" {
                return HttpResponse::Conflict().json(serde_json::json!({
                    "error": "Buy still unresolved — use action=verify"
                }));
            }
            // Manual Retry on a parked bag un-parks it (fresh auto-redrive budget).
            if status == "ExitStuck" && pos.exit_parked {
                if let Err(e) = app_state.strategy_repo.unpark_exit(pos.id).await {
                    tracing::warn!(position_id = %pos.id, "unpark_exit failed: {e}");
                }
            }

            // Registry hit → engine ManualClose (live path). Stuck/unconfirmed rows
            // are never engine-owned (the arm was dropped), so this is Holding-only.
            if status == "Holding" && app_state.positions.engine_id(position_id).is_some() {
                if app_state.engine.manual_close(position_id, portion).await {
                    return HttpResponse::Accepted().json(serde_json::json!({ "closing": true }));
                }
                return HttpResponse::InternalServerError()
                    .json(serde_json::json!({"error": "Engine shutting down"}));
            }

            // Partial sells need the live engine (stage/sold_bps resume on fill).
            // Orphan / stuck paths always clear the full remaining bag.
            if portion.is_partial() {
                return HttpResponse::Conflict().json(serde_json::json!({
                    "error": "Partial sell (sell_bps) requires a live Holding position in the engine registry"
                }));
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
                        orphan_exit::fill_from_latest_sell(&app_state.trade_repo(), &wallet, &pos)
                            .await;
                    let deps = orphan_deps_from_state(&app_state);
                    return match orphan_exit::book_externally_cleared(&deps, &pos, fill).await {
                        Ok(()) => HttpResponse::Ok().json(serde_json::json!({ "closed": true })),
                        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({
                            "error": format!("Failed to book cleared position: {e}")
                        })),
                    };
                }
            }

            // Orphan / stuck / unconfirmed retry — direct sell (feed confirm), never
            // silent 202. A re-sell of ExitUnconfirmed is safe: if the original sell
            // landed, this one finds nothing to sell and the row books cleared.
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

        other => HttpResponse::BadRequest().json(serde_json::json!({
            "error": format!("Unknown action '{other}' (retry | dump | writeoff | verify)")
        })),
    }
}

/// `?action=verify` — resolve a row on demand without selling.
///
/// - `ExitUnconfirmed`: PG-net check — bag gone ⇒ heal to `End` (the sell DID
///   land); bag present ⇒ report "still held" so the operator can Re-sell / Dump /
///   Write off.
/// - `BuySubmitted`: the reaper's adopt-or-drop logic, one shot (B3 resolve).
async fn verify_position(
    app_state: &web::Data<Arc<DeployState>>,
    pos: StrategyPosition,
) -> HttpResponse {
    use crate::strategies::engine::orphan_exit::{self, BAG_CLEARED_THRESHOLD_RAW};
    use crate::strategies::engine::reapers::{self, BuyResolution};

    match pos.status.as_str() {
        "ExitUnconfirmed" => {
            let wallet = app_state.trader.wallet_pubkey();
            let net = match app_state
                .trade_repo()
                .net_token_amount_by_wallet_and_mint(&wallet, &pos.mint_address)
                .await
            {
                Ok(n) => n,
                Err(e) => {
                    return HttpResponse::InternalServerError()
                        .json(serde_json::json!({"error": format!("net check failed: {e}")}));
                }
            };
            if net <= BAG_CLEARED_THRESHOLD_RAW {
                let fill =
                    orphan_exit::fill_from_latest_sell(&app_state.trade_repo(), &wallet, &pos)
                        .await;
                let deps = orphan_deps_from_state(app_state);
                match orphan_exit::book_externally_cleared(&deps, &pos, fill).await {
                    Ok(()) => HttpResponse::Ok()
                        .json(serde_json::json!({ "verified": true, "cleared": true })),
                    Err(e) => HttpResponse::InternalServerError()
                        .json(serde_json::json!({"error": format!("heal failed: {e}")})),
                }
            } else {
                HttpResponse::Ok().json(serde_json::json!({
                    "verified": true,
                    "cleared": false,
                    "still_held": true,
                    "net_token_amount": net,
                }))
            }
        }
        "BuySubmitted" => {
            let Some(_guard) = app_state.inflight.try_begin_entry(pos.id) else {
                return HttpResponse::Conflict()
                    .json(serde_json::json!({"error": "Buy resolve already in flight"}));
            };
            let deps = reaper_deps_from_state(app_state);
            match reapers::resolve_buy_submitted(&deps, &pos).await {
                BuyResolution::Adopted => HttpResponse::Ok()
                    .json(serde_json::json!({ "verified": true, "adopted": true })),
                BuyResolution::Dropped => HttpResponse::Ok()
                    .json(serde_json::json!({ "verified": true, "dropped": true })),
                BuyResolution::Wait => HttpResponse::Ok().json(serde_json::json!({
                    "verified": true,
                    "unresolved": true,
                })),
            }
        }
        other => HttpResponse::Conflict().json(serde_json::json!({
            "error": format!("verify applies only to ExitUnconfirmed/BuySubmitted, not {other}")
        })),
    }
}

/// Body of `POST /api/positions/manual-buy` — a Console manual buy that becomes a
/// full tracked position (M1 fix). TP/SL are optional; absent ⇒ tracked-only.
#[derive(Debug, Deserialize)]
pub struct ManualBuyRequest {
    pub mint_address: String,
    pub amount_sol: f64,
    #[serde(default)]
    pub tp_pct: Option<f64>,
    #[serde(default)]
    pub sl_pct: Option<f64>,
}

/// POST /api/positions/manual-buy
///
/// Replaces the old synchronous `POST /api/solana/wallet/buy`: inserts a
/// `BuySubmitted` position (`origin='manual'`) via the engine and returns **202
/// `{position_id}` immediately** — all further truth arrives over the
/// `strategy_position_update` SSE stream exactly like a bot buy (M2 fix: there is
/// no sync "green submitted" to lie with). Bypasses entry gates/caps (the user's
/// call) but keeps the one-open-position-per-mint guard and `MAX_MANUAL_BUY_SOL`.
pub async fn manual_buy_position(
    app_state: web::Data<Arc<DeployState>>,
    body: web::Json<ManualBuyRequest>,
) -> impl Responder {
    let amount_sol = body.amount_sol;
    if !amount_sol.is_finite() || amount_sol <= 0.0 {
        return HttpResponse::BadRequest()
            .json(serde_json::json!({ "error": "amount_sol must be a finite, positive number" }));
    }
    if amount_sol > MAX_MANUAL_BUY_SOL {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "error": format!(
                "amount_sol {amount_sol} exceeds the per-trade limit of {MAX_MANUAL_BUY_SOL} SOL"
            )
        }));
    }
    if let Err(e) = validate_solana_address(&body.mint_address) {
        return HttpResponse::BadRequest()
            .json(serde_json::json!({ "error": format!("invalid mint: {e}") }));
    }
    for (name, v) in [("tp_pct", body.tp_pct), ("sl_pct", body.sl_pct)] {
        if let Some(v) = v {
            if !v.is_finite() || v <= 0.0 {
                return HttpResponse::BadRequest().json(serde_json::json!({
                    "error": format!("{name} must be a finite, positive percentage")
                }));
            }
        }
    }

    // One open position per mint (double-click double-buys included, M3): the
    // serialized engine loop also dedups, but reject early with an honest 409.
    match app_state.strategy_repo.has_open_real_position_on_mint(&body.mint_address).await {
        Ok(true) => {
            return HttpResponse::Conflict().json(serde_json::json!({
                "error": "An open position already exists for this mint — use its row actions"
            }));
        }
        Ok(false) => {}
        Err(e) => {
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": format!("open-position check failed: {e}") }));
        }
    }

    let exit = (body.tp_pct.is_some() || body.sl_pct.is_some())
        .then_some(ManualExit { tp_pct: body.tp_pct, sl_pct: body.sl_pct });
    let pg_id = Uuid::new_v4();
    let lamports = sol_to_lamports(amount_sol).max(0) as u64;
    if !app_state
        .engine
        .manual_buy(pg_id, body.mint_address.clone(), lamports, exit)
        .await
    {
        return HttpResponse::InternalServerError()
            .json(serde_json::json!({ "error": "Engine shutting down" }));
    }
    HttpResponse::Accepted().json(serde_json::json!({ "position_id": pg_id }))
}

/// Body of the manual-exit PATCH — both absent/null clears the config
/// (tracked-only again).
#[derive(Debug, Deserialize)]
pub struct ManualExitRequest {
    #[serde(default)]
    pub tp_pct: Option<f64>,
    #[serde(default)]
    pub sl_pct: Option<f64>,
}

/// POST /api/strategies/{strategy}/positions/{position_id}/manual-exit
///
/// Set / replace / clear a **manual** position's TP/SL. Persists the JSONB, then
/// tells the engine to (re)synthesize the one-off exit rule — the position gains
/// the full engine exit stack (TP/SL + Dead) or drops back to tracked-only.
pub async fn set_manual_exit(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<(String, Uuid)>,
    body: web::Json<ManualExitRequest>,
) -> impl Responder {
    let (_strategy, position_id) = path.into_inner();
    for (name, v) in [("tp_pct", body.tp_pct), ("sl_pct", body.sl_pct)] {
        if let Some(v) = v {
            if !v.is_finite() || v <= 0.0 {
                return HttpResponse::BadRequest().json(serde_json::json!({
                    "error": format!("{name} must be a finite, positive percentage")
                }));
            }
        }
    }
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
    if pos.origin != "manual" {
        return HttpResponse::Conflict()
            .json(serde_json::json!({"error": "TP/SL config applies to manual positions only"}));
    }
    if !matches!(pos.status.as_str(), "BuySubmitted" | "Holding") {
        return HttpResponse::Conflict().json(serde_json::json!({
            "error": format!("Cannot change TP/SL from status {}", pos.status)
        }));
    }

    let exit = (body.tp_pct.is_some() || body.sl_pct.is_some())
        .then_some(ManualExit { tp_pct: body.tp_pct, sl_pct: body.sl_pct });
    let json = exit.map(|e| serde_json::json!({ "tp_pct": e.tp_pct, "sl_pct": e.sl_pct }));
    if let Err(e) = app_state
        .strategy_repo
        .set_manual_exit(position_id, json.as_ref())
        .await
    {
        return HttpResponse::InternalServerError()
            .json(serde_json::json!({"error": format!("persist failed: {e}")}));
    }
    // Engine-held (Holding) → resynthesize now; a BuySubmitted row picks the
    // config up when the fill adopts (the JSONB is read at adopt time).
    let _ = app_state.engine.set_manual_exit(position_id, exit).await;
    HttpResponse::Ok().json(serde_json::json!({ "updated": true, "manual_exit": json }))
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

fn reaper_deps_from_state(app_state: &DeployState) -> crate::strategies::engine::reapers::ReaperDeps {
    crate::strategies::engine::reapers::ReaperDeps {
        strategy_repo: app_state.strategy_repo.clone(),
        trade_repo: app_state.trade_repo(),
        trader: app_state.trader.clone(),
        token_cache: app_state.token_cache.clone(),
        trade_signals: app_state.trade_signals.clone(),
        inflight: app_state.inflight.clone(),
        registry: app_state.positions.clone(),
        fill_tx: app_state.engine_fill_tx.clone(),
        settings: app_state.settings.subscribe(),
        sse_tx: app_state.sse_tx.clone(),
        onchain_bag_check:
            crate::strategies::engine::reapers::onchain_bag_check_from_env(),
    }
}
