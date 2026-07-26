pub mod handlers;

use actix_web::web;

/// Register deploy-only routes (live trading, token sync, live-mode toggle,
/// rule CRUD + lifecycle, position reads, on-chain Solana queries, cashback).
/// Call alongside `trading_core::api::configure_core_routes` to build the full
/// deploy route set.
///
/// Rule CRUD + lifecycle run over the generic fingerprint+metrics engine
/// (`/strategy-rules/*`, `handlers::strategies::engine::*`). Position reads keep
/// the `/strategies/{strategy}/positions*` URLs for back-compat (the `{strategy}`
/// segment is ignored). (The analysis box's `simulate` / `paper-result` handlers
/// take `LocalState` and stay in `lab` — they are not registered here.)
pub fn configure_deploy_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api")
            // ── Generic fingerprint + metrics engine (strategy redesign) ──
            // The metric registry the frontend renders its rule-authoring UI from.
            .route(
                "/meta/strategy-registry",
                web::get().to(handlers::strategies::engine::strategy_registry),
            )
            // Live armed (token, rule) snapshot for the monitor.
            .route("/strategies/armed", web::get().to(handlers::strategies::engine::list_armed))
            // Fingerprints CRUD (shared by many rules).
            .route("/fingerprints", web::get().to(handlers::strategies::engine::list_fingerprints))
            .route("/fingerprints", web::post().to(handlers::strategies::engine::create_fingerprint))
            .route(
                "/fingerprints/{id}",
                web::get().to(handlers::strategies::engine::get_fingerprint),
            )
            .route(
                "/fingerprints/{id}",
                web::put().to(handlers::strategies::engine::update_fingerprint),
            )
            .route(
                "/fingerprints/{id}",
                web::delete().to(handlers::strategies::engine::delete_fingerprint),
            )
            // Generic rules CRUD + activate/pause/stop.
            .route("/strategy-rules", web::get().to(handlers::strategies::engine::list_rules))
            .route("/strategy-rules", web::post().to(handlers::strategies::engine::create_rule))
            // Bulk lifecycle (Pause All / Stop All), scoped by `?mode=real|paper` —
            // literal segments, registered before `{id}/...` so they never bind `{id}`.
            .route(
                "/strategy-rules/pause-all",
                web::post().to(handlers::strategies::engine::pause_all_rules),
            )
            .route(
                "/strategy-rules/stop-all",
                web::post().to(handlers::strategies::engine::stop_all_rules),
            )
            .route(
                "/strategy-rules/{id}/activate",
                web::post().to(handlers::strategies::engine::activate_rule),
            )
            .route(
                "/strategy-rules/{id}/pause",
                web::post().to(handlers::strategies::engine::pause_rule),
            )
            .route(
                "/strategy-rules/{id}/enable",
                web::post().to(handlers::strategies::engine::enable_rule),
            )
            .route(
                "/strategy-rules/{id}/disable",
                web::post().to(handlers::strategies::engine::disable_rule),
            )
            .route(
                "/strategy-rules/{id}/stop",
                web::post().to(handlers::strategies::engine::stop_rule),
            )
            .route(
                "/strategy-rules/{id}/runs",
                web::get().to(handlers::strategies::engine::list_rule_runs),
            )
            .route("/strategy-rules/{id}", web::get().to(handlers::strategies::engine::get_rule))
            .route("/strategy-rules/{id}", web::put().to(handlers::strategies::engine::update_rule))
            .route(
                "/strategy-rules/{id}",
                web::delete().to(handlers::strategies::engine::delete_rule),
            )
            // Token list (unified TableRequest POST; no swing-chain stats on live)
            .route("/tokens", web::post().to(handlers::tokens::list_tokens))
            // Token sync
            .route("/token/sync", web::post().to(handlers::tokens::sync_token))
            .route("/token/sync/preview", web::post().to(handlers::tokens::preview_sync))
            // Live mode toggle
            .route("/system/live", web::get().to(handlers::system::get_live_mode))
            .route("/system/live", web::put().to(handlers::system::set_live_mode))
            // Strategy position reads — one unified handler set over the
            // `strategy_positions` table, keyed by the `{strategy}` path segment
            // (`tpsl1`/`tpsl2` aliases or canonical ids). More-specific
            // `positions/mint|wallet/...` are registered before the catch-all
            // `positions/{position_id}` so they win the match.
            // Run/rule-wide aggregates for the Positions Summary panel — registered
            // before the paginated list route (distinct `/summary` suffix, no clash).
            .route(
                "/strategies/{strategy}/rules/{rule_id}/positions/summary",
                web::post().to(handlers::strategies::positions::get_positions_summary_by_rule),
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
            // Per-row "Sell ALL" — force-close ONE position via the position-aware
            // path (row shows ExitPending→closed over SSE). More specific than the
            // `{position_id}` GET below (extra `/close` segment), so ordering is safe.
            .route(
                "/strategies/{strategy}/positions/{position_id}/close",
                web::post().to(handlers::strategies::positions::close_position),
            )
            // Manual position's TP/SL config (set / replace / clear).
            .route(
                "/strategies/{strategy}/positions/{position_id}/manual-exit",
                web::post().to(handlers::strategies::positions::set_manual_exit),
            )
            // Console manual buy → a full tracked position (202 {position_id};
            // progress over SSE like a bot buy). Replaces the sync wallet buy.
            .route(
                "/positions/manual-buy",
                web::post().to(handlers::strategies::positions::manual_buy_position),
            )
            .route(
                "/strategies/{strategy}/positions/{position_id}",
                web::get().to(handlers::strategies::positions::get_position),
            )
            // Portfolio/PnL reads (Holdings + Home + Live-Trading) — enriched
            // holdings with cost basis/PnL/bot tag, the wallet summary, and the
            // cross-strategy open-positions roll-up.
            .route("/portfolio/holdings", web::get().to(handlers::trading::get_portfolio_holdings))
            .route("/portfolio/holdings/query", web::post().to(handlers::trading::query_portfolio_holdings))
            .route("/portfolio/holdings/summary", web::post().to(handlers::trading::portfolio_holdings_summary))
            .route("/portfolio/summary", web::get().to(handlers::trading::get_portfolio_summary))
            .route("/portfolio/positions", web::get().to(handlers::trading::get_portfolio_positions))
            .route(
                "/portfolio/recent-closes",
                web::get().to(handlers::trading::get_portfolio_recent_closes),
            )
            .route(
                "/portfolio/performance",
                web::get().to(handlers::trading::get_portfolio_performance),
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
