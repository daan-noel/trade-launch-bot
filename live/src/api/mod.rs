pub mod handlers;

use actix_web::web;

/// Register deploy-only routes (live trading, token sync, live-mode toggle,
/// rule CRUD + lifecycle, position reads, on-chain Solana queries, cashback).
/// Call alongside `trading_core::api::configure_core_routes` to build the full
/// deploy route set.
///
/// Rule CRUD + lifecycle run over the unified [`StrategyService`] +
/// `strategies::rules` domain, keyed by a `{strategy}` path segment. (The
/// analysis box's `simulate` / `paper-result` handlers take `LocalState` and stay
/// in `lab` — they are not registered here.)
pub fn configure_deploy_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api")
            // Token list (no swing-chain stats on the live bin)
            .route("/tokens", web::get().to(handlers::tokens::list_tokens))
            // Token sync
            .route("/token/sync", web::post().to(handlers::tokens::sync_token))
            .route("/token/sync/preview", web::post().to(handlers::tokens::preview_sync))
            // Live mode toggle
            .route("/system/live", web::get().to(handlers::system::get_live_mode))
            .route("/system/live", web::put().to(handlers::system::set_live_mode))
            // Strategy rule CRUD + lifecycle — one unified handler set over the
            // `StrategyService` + `strategies::rules` domain, keyed by `{strategy}`.
            .route(
                "/strategies/{strategy}/rules",
                web::get().to(handlers::strategies::rules::list_rules),
            )
            .route(
                "/strategies/{strategy}/rules",
                web::post().to(handlers::strategies::rules::create_rule),
            )
            .route(
                "/strategies/{strategy}/rules/{rule_id}/activate",
                web::post().to(handlers::strategies::rules::activate_rule),
            )
            .route(
                "/strategies/{strategy}/rules/{rule_id}/pause",
                web::post().to(handlers::strategies::rules::pause_rule),
            )
            .route(
                "/strategies/{strategy}/rules/{rule_id}/stop",
                web::post().to(handlers::strategies::rules::stop_rule),
            )
            .route(
                "/strategies/{strategy}/rules/{rule_id}",
                web::get().to(handlers::strategies::rules::get_rule),
            )
            .route(
                "/strategies/{strategy}/rules/{rule_id}",
                web::put().to(handlers::strategies::rules::update_rule),
            )
            .route(
                "/strategies/{strategy}/rules/{rule_id}",
                web::delete().to(handlers::strategies::rules::delete_rule),
            )
            // Strategy position reads — one unified handler set over the
            // `strategy_positions` table, keyed by the `{strategy}` path segment
            // (`tpsl1`/`tpsl2` aliases or canonical ids). More-specific
            // `positions/mint|wallet/...` are registered before the catch-all
            // `positions/{position_id}` so they win the match.
            // Run/rule-wide aggregates for the Positions Summary panel — registered
            // before the paginated list route (distinct `/summary` suffix, no clash).
            .route(
                "/strategies/{strategy}/rules/{rule_id}/positions/summary",
                web::get().to(handlers::strategies::positions::get_positions_summary_by_rule),
            )
            .route(
                "/strategies/{strategy}/rules/{rule_id}/positions",
                web::post().to(handlers::strategies::positions::get_positions_by_rule),
            )
            .route(
                "/strategies/{strategy}/positions",
                web::get().to(handlers::strategies::positions::list_positions),
            )
            .route(
                "/strategies/{strategy}/positions/mint/{mint}",
                web::get().to(handlers::strategies::positions::get_positions_by_mint),
            )
            .route(
                "/strategies/{strategy}/positions/wallet/{wallet}",
                web::get().to(handlers::strategies::positions::get_positions_by_wallet),
            )
            .route(
                "/strategies/{strategy}/positions/{position_id}",
                web::get().to(handlers::strategies::positions::get_position),
            )
            // On-chain Solana queries
            .route("/solana/wallet/tokens", web::get().to(handlers::trading::get_wallet_tokens))
            .route("/solana/wallet/tokens/{mint}", web::get().to(handlers::trading::get_wallet_token))
            .route("/solana/prices", web::get().to(handlers::trading::get_prices))
            .route("/solana/wallet/buy", web::post().to(handlers::trading::manual_buy))
            .route("/solana/wallet/sell", web::post().to(handlers::trading::manual_sell))
            .route(
                "/solana/wallet/{wallet}/token/{mint}",
                web::get().to(handlers::trading::get_wallet_token_balance),
            )
            // Cashback
            .route("/cashback/status", web::get().to(handlers::trading::get_cashback_status))
            .route("/cashback/claim", web::post().to(handlers::trading::claim_cashback)),
    );
}
