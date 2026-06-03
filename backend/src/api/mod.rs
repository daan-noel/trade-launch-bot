pub mod handlers;

use actix_web::web;

/// Register all `/api` routes onto the Actix service config.
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api")
            // Token endpoints
            .route("/tokens", web::get().to(handlers::tokens::list_tokens))
            .route("/tokens/{mint}", web::get().to(handlers::tokens::get_token))
            .route(
                "/tokens/{mint}/trades",
                web::get().to(handlers::tokens::get_trades),
            )
            .route(
                "/tokens/{mint}/analysis",
                web::get().to(handlers::tokens::get_token_analysis),
            )
            .route(
                "/tokens/{mint}/swings",
                web::post().to(handlers::tokens::detect_token_swings),
            )
            // Creator endpoints
            .route(
                "/creators",
                web::get().to(handlers::tokens::list_creators),
            )
            .route(
                "/creators/{wallet}",
                web::get().to(handlers::tokens::get_creator),
            )
            // Analysis list
            .route(
                "/analysis",
                web::get().to(handlers::tokens::list_analysis_results),
            )
            // Real-time SSE stream
            .route("/stream", web::get().to(handlers::system::stream_events))
            // System endpoints
            .route(
                "/system/live",
                web::get().to(handlers::system::get_live_mode),
            )
            .route(
                "/system/live",
                web::put().to(handlers::system::set_live_mode),
            )
            .route(
                "/system/price",
                web::get().to(handlers::system::get_sol_price),
            )
            // Wallet endpoints
            .route(
                "/wallets/{address}",
                web::get().to(handlers::system::get_wallet),
            )
            .route(
                "/wallets/{address}/flag",
                web::post().to(handlers::system::flag_wallet),
            )
            .route(
                "/wallets/{address}/flag",
                web::delete().to(handlers::system::unflag_wallet),
            )
            // Strategy endpoints
            .route(
                "/strategies/tpsl/rules",
                web::get().to(handlers::strategies::list_tpsl_rules),
            )
            .route(
                "/strategies/tpsl/rules",
                web::post().to(handlers::strategies::create_tpsl_rule),
            )
            .route(
                "/strategies/tpsl/rules/{rule_id}",
                web::get().to(handlers::strategies::get_tpsl_rule),
            )
            .route(
                "/strategies/tpsl/rules/{rule_id}",
                web::put().to(handlers::strategies::update_tpsl_rule),
            )
            .route(
                "/strategies/tpsl/rules/{rule_id}",
                web::delete().to(handlers::strategies::delete_tpsl_rule),
            )
            .route(
                "/strategies/tpsl/rules/{rule_id}/positions",
                web::get().to(handlers::strategies::get_positions_by_rule),
            )
            .route(
                "/strategies/tpsl/rules/{rule_id}/simulate",
                web::get().to(handlers::strategies::simulate_tpsl_rule),
            )
            .route(
                "/strategies/tpsl/rules/{rule_id}/matched",
                web::get().to(handlers::strategies::get_matched_tokens),
            )
            // Position endpoints
            .route(
                "/positions",
                web::get().to(handlers::strategies::list_positions),
            )
            .route(
                "/positions/{position_id}",
                web::get().to(handlers::strategies::get_position),
            )
            .route(
                "/positions/mint/{mint}",
                web::get().to(handlers::strategies::get_positions_by_mint),
            )
            .route(
                "/positions/wallet/{wallet}",
                web::get().to(handlers::strategies::get_positions_by_wallet),
            )
            // On-chain Solana queries — bypass local DB entirely
            .route(
                "/solana/wallet/tokens",
                web::get().to(handlers::trading::get_wallet_tokens),
            )
            .route(
                "/solana/wallet/buy",
                web::post().to(handlers::trading::manual_buy),
            )
            .route(
                "/solana/wallet/sell",
                web::post().to(handlers::trading::manual_sell),
            )
            .route(
                "/solana/wallet/{wallet}/token/{mint}",
                web::get().to(handlers::trading::get_wallet_token_balance),
            ),
    );
}
