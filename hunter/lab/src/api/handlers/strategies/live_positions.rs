//! Traded-position reads on the analysis box — the lab twin of the live
//! `strategies/positions.rs` by-rule endpoints.
//!
//! These serve the **real/paper positions the live engine actually traded**, off the
//! local mirror that `scripts/db-incremental-sync.ps1` fills (it copies
//! `strategy_rules` / `strategy_runs` / `strategy_run_metrics` / `strategy_positions`
//! from EC2). That puts a live position next to the lab-only metric panes + replay,
//! so a real fill can be inspected against what every metric read at the moment the
//! engine fired.
//!
//! Read-only by design. Rule **lifecycle** (activate/pause/stop) is deliberately NOT
//! mirrored here: a write would land in the lab DB, never reach the live engine, and
//! be overwritten on the next sync (server wins). Keep/kill stays on the live app.
//!
//! Freshness: these rows are a snapshot as of the last sync — there is no ingest or
//! SSE on this box, so nothing here updates on its own.
//!
//! The scope semantics + wire shape are the shared SSOT in
//! [`trading_core::api::handlers::strategies::rule_positions`]; this module is state
//! plumbing only.

use std::sync::Arc;

use actix_web::{web, Responder};
use uuid::Uuid;

use trading_core::api::handlers::strategies::rule_positions::{
    self, EpisodeModeParam, ScopeParam,
};
use trading_core::api::table_query::TableRequest;
use trading_core::storage::repositories::rule_repo::RuleRepo;
use trading_core::storage::repositories::strategy_repo::StrategyRepo;

use crate::state::local_state::LocalState;

fn strategy_repo(state: &LocalState) -> StrategyRepo {
    state.core.strategy_repo()
}
fn rule_repo(state: &LocalState) -> RuleRepo {
    RuleRepo::new(state.core.db.clone())
}

/// POST `/api/strategies/{strategy}/rules/{rule_id}/positions` (lab twin).
///
/// Same wire contract as the live endpoint — the frontend's shared positions table
/// hits whichever bin it's built against with no branch.
pub async fn get_positions_by_rule(
    app_state: web::Data<Arc<LocalState>>,
    path: web::Path<(String, Uuid)>,
    scope: web::Query<ScopeParam>,
    body: web::Json<TableRequest>,
) -> impl Responder {
    let (_strategy, rule_id) = path.into_inner();
    rule_positions::rule_positions_page(
        &strategy_repo(&app_state),
        &rule_repo(&app_state),
        rule_id,
        scope.into_inner(),
        body.into_inner(),
    )
    .await
}

/// POST `/api/strategies/{strategy}/rules/{rule_id}/positions/summary` (lab twin).
///
/// Open positions mark to market off the lab's seeded token cache. That cache is a
/// snapshot, so `open_pnl_sol` here is "last synced mark", not a live mark — a mint
/// with no cached price is simply left out of the mark rather than guessed at.
pub async fn get_positions_summary_by_rule(
    app_state: web::Data<Arc<LocalState>>,
    path: web::Path<(String, Uuid)>,
    scope: web::Query<ScopeParam>,
    body: web::Json<TableRequest>,
) -> impl Responder {
    let (_strategy, rule_id) = path.into_inner();
    let token_cache = app_state.core.token_cache.clone();
    let mark_of = |mint: &str| trading_core::state::token_cache::mark_quote(&token_cache, mint);
    rule_positions::rule_positions_summary(
        &strategy_repo(&app_state),
        &rule_repo(&app_state),
        rule_id,
        scope.into_inner(),
        body.into_inner(),
        mark_of,
    )
    .await
}

/// GET `/api/strategies/{strategy}/positions/mint/{mint}/episodes` (lab twin) —
/// the mint's re-entry history for the inspect chart's marker overlay.
pub async fn get_mint_episodes(
    app_state: web::Data<Arc<LocalState>>,
    path: web::Path<(String, String)>,
    query: web::Query<EpisodeModeParam>,
) -> impl Responder {
    let (_strategy, mint) = path.into_inner();
    rule_positions::mint_episodes(&strategy_repo(&app_state), &mint, query.mode.as_deref()).await
}

/// GET `/api/strategies/{strategy}/positions/{position_id}/fills` (lab twin) —
/// Evidence dialog ledger over the synced mirror.
pub async fn get_position_fills(
    app_state: web::Data<Arc<LocalState>>,
    path: web::Path<(String, Uuid)>,
) -> impl Responder {
    let (_strategy, position_id) = path.into_inner();
    rule_positions::position_fills(&strategy_repo(&app_state), position_id).await
}

/// GET `/api/strategy-rules/{id}/runs` (lab twin) — the run navigator's chips +
/// cross-run PnL trend over the synced run history.
pub async fn list_rule_runs(
    app_state: web::Data<Arc<LocalState>>,
    path: web::Path<Uuid>,
) -> impl Responder {
    rule_positions::rule_runs(
        &strategy_repo(&app_state),
        &rule_repo(&app_state),
        path.into_inner(),
    )
    .await
}
