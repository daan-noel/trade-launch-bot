pub mod handlers;

use actix_web::web;

/// Register deploy-only routes (live trading, token sync, live-mode toggle,
/// position reads, on-chain Solana queries, cashback). Call alongside
/// `backend_core::api::configure_core_routes` to build the full deploy route set.
///
/// Per the T11/T12 split (Option A), the tpsl1/tpsl2 **rule CRUD + simulate**
/// handlers also take `LocalState` and stay in `backend` (later `backend-local`),
/// so they are not registered here — only the live position-read routes are.
pub fn configure_deploy_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api")
            // Token sync
            .route("/token/sync", web::post().to(handlers::tokens::sync_token))
            .route("/token/sync/preview", web::post().to(handlers::tokens::preview_sync))
            // Live mode toggle
            .route("/system/live", web::get().to(handlers::system::get_live_mode))
            .route("/system/live", web::put().to(handlers::system::set_live_mode))
            // Strategy position reads — tpsl1
            .route(
                "/strategies/tpsl1/rules/{rule_id}/positions",
                web::get().to(handlers::strategies::tpsl1_positions::get_positions_by_rule),
            )
            .route(
                "/strategies/tpsl1/positions",
                web::get().to(handlers::strategies::tpsl1_positions::list_positions),
            )
            .route(
                "/strategies/tpsl1/positions/mint/{mint}",
                web::get().to(handlers::strategies::tpsl1_positions::get_positions_by_mint),
            )
            .route(
                "/strategies/tpsl1/positions/wallet/{wallet}",
                web::get().to(handlers::strategies::tpsl1_positions::get_positions_by_wallet),
            )
            .route(
                "/strategies/tpsl1/positions/{position_id}",
                web::get().to(handlers::strategies::tpsl1_positions::get_position),
            )
            // Strategy position reads — tpsl2
            .route(
                "/strategies/tpsl2/rules/{rule_id}/positions",
                web::get().to(handlers::strategies::tpsl2_positions::get_positions_by_rule),
            )
            .route(
                "/strategies/tpsl2/positions",
                web::get().to(handlers::strategies::tpsl2_positions::list_positions),
            )
            .route(
                "/strategies/tpsl2/positions/mint/{mint}",
                web::get().to(handlers::strategies::tpsl2_positions::get_positions_by_mint),
            )
            .route(
                "/strategies/tpsl2/positions/wallet/{wallet}",
                web::get().to(handlers::strategies::tpsl2_positions::get_positions_by_wallet),
            )
            .route(
                "/strategies/tpsl2/positions/{position_id}",
                web::get().to(handlers::strategies::tpsl2_positions::get_position),
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
