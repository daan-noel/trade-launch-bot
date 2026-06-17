pub mod handlers;

use actix_web::web;

/// Register all `/api` routes onto the Actix service config.
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api")
            .route("/token/sync", web::post().to(handlers::tokens::sync_token))
            .route(
                "/token/sync/preview",
                web::post().to(handlers::tokens::preview_sync),
            )
            // Token endpoints
            .route("/tokens", web::get().to(handlers::tokens::list_tokens))
            // Creation-time bias aggregate (dashboard). Registered before the
            // `{mint}` route so the static path isn't captured as a mint.
            .route(
                "/tokens/creation-stats",
                web::get().to(handlers::tokens::get_creation_stats),
            )
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
                "/tokens/swings/batch",
                web::post().to(handlers::tokens::detect_tokens_swings_batch),
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
            // Background-job status + control (sweep / simulation). Lets a
            // freshly-loaded dashboard recover in-flight progress SSE can't replay.
            .route("/jobs/status", web::get().to(handlers::system::job_status))
            .route(
                "/jobs/simulations/{rule_id}/cancel",
                web::post().to(handlers::system::cancel_simulation),
            )
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
            // Persisted app settings (tracking policy + header prefs)
            .route(
                "/system/settings",
                web::get().to(handlers::system::get_settings),
            )
            .route(
                "/system/settings",
                web::put().to(handlers::system::update_settings),
            )
            // Profile endpoints
            .route(
                "/profiles",
                web::get().to(handlers::system::list_profiles),
            )
            .route(
                "/profiles",
                web::post().to(handlers::system::create_profile),
            )
            .route(
                "/profiles/{id}",
                web::get().to(handlers::system::get_profile),
            )
            .route(
                "/profiles/{id}",
                web::put().to(handlers::system::update_profile),
            )
            .route(
                "/profiles/{id}",
                web::delete().to(handlers::system::delete_profile),
            )
            // Profile tag assignment
            .route(
                "/profiles/{id}/tags",
                web::put().to(handlers::system::update_profile_tags),
            )
            // Wallet endpoints (under profiles for create, standalone for edit/delete)
            .route(
                "/profiles/{id}/wallets",
                web::post().to(handlers::system::create_wallet),
            )
            .route(
                "/wallets/{id}",
                web::get().to(handlers::system::get_wallet),
            )
            .route(
                "/wallets/{id}",
                web::put().to(handlers::system::update_wallet),
            )
            .route(
                "/wallets/{id}",
                web::delete().to(handlers::system::delete_wallet),
            )
            // Tag endpoints
            .route("/tags", web::get().to(handlers::system::list_tags))
            .route("/tags", web::post().to(handlers::system::create_tag))
            .route("/tags/{id}", web::put().to(handlers::system::update_tag))
            .route("/tags/{id}", web::delete().to(handlers::system::delete_tag))
            // Strategy endpoints — tpsl_sniper_1
            .route(
                "/strategies/tpsl1/rules",
                web::get().to(handlers::strategies::tpsl1::list_tpsl_rules),
            )
            .route(
                "/strategies/tpsl1/rules",
                web::post().to(handlers::strategies::tpsl1::create_tpsl_rule),
            )
            .route(
                "/strategies/tpsl1/rules/{rule_id}",
                web::get().to(handlers::strategies::tpsl1::get_tpsl_rule),
            )
            .route(
                "/strategies/tpsl1/rules/{rule_id}",
                web::put().to(handlers::strategies::tpsl1::update_tpsl_rule),
            )
            .route(
                "/strategies/tpsl1/rules/{rule_id}",
                web::delete().to(handlers::strategies::tpsl1::delete_tpsl_rule),
            )
            .route(
                "/strategies/tpsl1/rules/{rule_id}/positions",
                web::get().to(handlers::strategies::tpsl1_positions::get_positions_by_rule),
            )
            .route(
                "/strategies/tpsl1/rules/{rule_id}/simulate",
                web::get().to(handlers::strategies::tpsl1::simulate_tpsl_rule),
            )
            .route(
                "/strategies/tpsl1/rules/{rule_id}/simulate/cancel",
                web::post().to(handlers::strategies::tpsl1::cancel_simulate_tpsl_rule),
            )
            .route(
                "/strategies/tpsl1/rules/{rule_id}/paper-result",
                web::get().to(handlers::strategies::tpsl1::paper_result_tpsl_rule),
            )
            .route(
                "/strategies/tpsl1/rules/{rule_id}/paper-result",
                web::delete().to(handlers::strategies::tpsl1::clear_paper_result_tpsl_rule),
            )
            .route(
                "/strategies/tpsl1/rules/{rule_id}/matched",
                web::get().to(handlers::strategies::tpsl1::get_matched_tokens),
            )
            // tpsl1 rule lifecycle (activate / pause / stop-and-close)
            .route(
                "/strategies/tpsl1/rules/{rule_id}/activate",
                web::post().to(handlers::strategies::tpsl1::activate_tpsl_rule),
            )
            .route(
                "/strategies/tpsl1/rules/{rule_id}/pause",
                web::post().to(handlers::strategies::tpsl1::pause_tpsl_rule),
            )
            .route(
                "/strategies/tpsl1/rules/{rule_id}/stop",
                web::post().to(handlers::strategies::tpsl1::stop_tpsl_rule),
            )
            // tpsl1 positions
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
            // Strategy endpoints — tpsl_sniper_2 (clone of tpsl)
            .route(
                "/strategies/tpsl2/rules",
                web::get().to(handlers::strategies::tpsl2::list_tpsl_rules),
            )
            .route(
                "/strategies/tpsl2/rules",
                web::post().to(handlers::strategies::tpsl2::create_tpsl_rule),
            )
            .route(
                "/strategies/tpsl2/rules/{rule_id}",
                web::get().to(handlers::strategies::tpsl2::get_tpsl_rule),
            )
            .route(
                "/strategies/tpsl2/rules/{rule_id}",
                web::put().to(handlers::strategies::tpsl2::update_tpsl_rule),
            )
            .route(
                "/strategies/tpsl2/rules/{rule_id}",
                web::delete().to(handlers::strategies::tpsl2::delete_tpsl_rule),
            )
            .route(
                "/strategies/tpsl2/rules/{rule_id}/positions",
                web::get().to(handlers::strategies::tpsl2_positions::get_positions_by_rule),
            )
            .route(
                "/strategies/tpsl2/rules/{rule_id}/simulate",
                web::get().to(handlers::strategies::tpsl2::simulate_tpsl_rule),
            )
            .route(
                "/strategies/tpsl2/rules/{rule_id}/simulate/cancel",
                web::post().to(handlers::strategies::tpsl2::cancel_simulate_tpsl_rule),
            )
            .route(
                "/strategies/tpsl2/rules/{rule_id}/paper-result",
                web::get().to(handlers::strategies::tpsl2::paper_result_tpsl_rule),
            )
            .route(
                "/strategies/tpsl2/rules/{rule_id}/paper-result",
                web::delete().to(handlers::strategies::tpsl2::clear_paper_result_tpsl_rule),
            )
            .route(
                "/strategies/tpsl2/rules/{rule_id}/matched",
                web::get().to(handlers::strategies::tpsl2::get_matched_tokens),
            )
            // tpsl2 rule lifecycle (activate / pause / stop-and-close)
            .route(
                "/strategies/tpsl2/rules/{rule_id}/activate",
                web::post().to(handlers::strategies::tpsl2::activate_tpsl_rule),
            )
            .route(
                "/strategies/tpsl2/rules/{rule_id}/pause",
                web::post().to(handlers::strategies::tpsl2::pause_tpsl_rule),
            )
            .route(
                "/strategies/tpsl2/rules/{rule_id}/stop",
                web::post().to(handlers::strategies::tpsl2::stop_tpsl_rule),
            )
            // tpsl2 positions
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
            // Grouped strategy param-sweeps (generic across strategies; the
            // `strategy_id` body/query field resolves the per-strategy tables).
            .route(
                "/strategies/sweeps",
                web::get().to(handlers::strategies::grouped_sweep::list_runs),
            )
            .route(
                "/strategies/sweeps",
                web::post().to(handlers::strategies::grouped_sweep::start_grouped_sweep),
            )
            // Prune all runs older than `before` (groups + results cascade).
            .route(
                "/strategies/sweeps",
                web::delete().to(handlers::strategies::grouped_sweep::prune_runs),
            )
            .route(
                "/strategies/sweeps/cancel",
                web::post().to(handlers::strategies::grouped_sweep::cancel_grouped_sweep),
            )
            // Delete one run by id.
            .route(
                "/strategies/sweeps/{run_id}",
                web::delete().to(handlers::strategies::grouped_sweep::delete_run),
            )
            .route(
                "/strategies/sweeps/{run_id}/groups",
                web::get().to(handlers::strategies::grouped_sweep::list_groups),
            )
            .route(
                "/strategies/sweeps/{run_id}/groups/{group_id}/results",
                web::get().to(handlers::strategies::grouped_sweep::list_results),
            )
            // On-chain Solana queries — bypass local DB entirely
            .route(
                "/solana/wallet/tokens",
                web::get().to(handlers::trading::get_wallet_tokens),
            )
            .route(
                "/solana/wallet/tokens/{mint}",
                web::get().to(handlers::trading::get_wallet_token),
            )
            .route(
                "/solana/prices",
                web::get().to(handlers::trading::get_prices),
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
            )
            // Cashback — read accrued pump.fun cashback and sweep it back to the
            // wallet. Off the trade hot path (read-only status / recent-blockhash
            // claim).
            .route(
                "/cashback/status",
                web::get().to(handlers::trading::get_cashback_status),
            )
            .route(
                "/cashback/claim",
                web::post().to(handlers::trading::claim_cashback),
            ),
    );
}
