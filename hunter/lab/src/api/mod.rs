pub mod handlers;

use actix_web::web;

/// Register local-only (analysis box) routes: the token list, background-job
/// control, rule authoring + backtest edge, and grouped param-sweeps. Call alongside
/// `trading_core::api::configure_core_routes` to build the full local route set
/// (`App::new().configure(configure_core_routes).configure(configure_local_routes)`).
///
/// Live-trading routes (position *writes* + lifecycle, matched, sync, live-mode,
/// on-chain Solana, cashback) are deploy-only and are **not** registered here, so
/// they 404 on the local bin.
///
/// The by-rule position **reads** are the one exception: they're registered below
/// so the analysis app can inspect the real/paper positions the live engine traded
/// (served off the mirror `db-incremental-sync.ps1` fills) against the lab-only
/// metric panes. Read-only — no close, no lifecycle.
pub fn configure_local_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api")
            // Token list (unified TableRequest POST)
            .route("/tokens", web::post().to(handlers::tokens::list_tokens))
            // Matched mint-address set only (same filter body).
            .route("/tokens/mints", web::post().to(handlers::tokens::list_token_mints))
            // Redesign: every metric's series over a token's trades (chart panes)
            .route(
                "/tokens/{mint}/metric-series",
                web::get().to(handlers::tokens::token_metric_series),
            )
            // ── Traded (real/paper) positions off the synced mirror — read-only.
            //    Same wire contract as the live bin's twin; `{strategy}` is ignored
            //    (URL back-compat). Static `summary` before the bare path so it
            //    can't be shadowed.
            .route(
                "/strategies/{strategy}/rules/{rule_id}/positions/summary",
                web::post().to(handlers::strategies::live_positions::get_positions_summary_by_rule),
            )
            .route(
                "/strategies/{strategy}/rules/{rule_id}/positions",
                web::post().to(handlers::strategies::live_positions::get_positions_by_rule),
            )
            // Redesign: time-travel debugger — replay a recorded event log through
            // the engine and dump every event→effect decision (plan 6.1)
            .route(
                "/replay/inspect",
                web::post().to(handlers::replay::inspect_replay),
            )
            // Trader Analysis: mints a wallet traded (recent-first, days+limit)
            .route(
                "/wallets/{wallet}/tokens",
                web::get().to(handlers::wallets::list_wallet_tokens),
            )
            // Background-job status + control (sweep / simulation)
            .route("/jobs/status", web::get().to(handlers::system::job_status))
            .route(
                "/jobs/simulations/{rule_id}/cancel",
                web::post().to(handlers::system::cancel_simulation),
            )
            .route(
                "/jobs/simulations/{rule_id}/result",
                web::get().to(handlers::system::simulation_result),
            )
            // ── Generic engine simulate (redesign) — one surface for every rule ──
            .route(
                "/strategies/simulate",
                web::post().to(handlers::strategies::engine::simulate_engine),
            )
            // Static path before `{run_id}` so "summaries" is never parsed as a UUID.
            .route(
                "/strategies/simulate/summaries",
                web::post().to(handlers::strategies::engine::engine_sim_result_summaries),
            )
            .route(
                "/strategies/simulate/{run_id}/cancel",
                web::post().to(handlers::strategies::engine::cancel_engine_simulation),
            )
            .route(
                "/strategies/simulate/{run_id}/result",
                web::post().to(handlers::strategies::engine::engine_sim_result),
            )
            .route(
                "/strategies/simulate/{run_id}/result/summary",
                web::post().to(handlers::strategies::engine::engine_sim_result_summary),
            )
            .route(
                "/strategies/simulate/{run_id}/result/time-summary",
                web::post().to(handlers::strategies::engine::engine_sim_result_time_summary),
            )
            .route(
                "/strategies/simulate/{run_id}/matched",
                web::post().to(handlers::strategies::engine::engine_matched_tokens),
            )
            // ── Generic rule-authoring CRUD + registry (lab twin of the live
            //    engine handlers) — lets the lab app author + dry-run rules ──
            .route(
                "/meta/strategy-registry",
                web::get().to(handlers::strategies::engine_crud::strategy_registry),
            )
            .route(
                "/fingerprints",
                web::get().to(handlers::strategies::engine_crud::list_fingerprints),
            )
            .route(
                "/fingerprints",
                web::post().to(handlers::strategies::engine_crud::create_fingerprint),
            )
            .route(
                "/fingerprints/{id}",
                web::get().to(handlers::strategies::engine_crud::get_fingerprint),
            )
            .route(
                "/fingerprints/{id}",
                web::put().to(handlers::strategies::engine_crud::update_fingerprint),
            )
            .route(
                "/fingerprints/{id}",
                web::delete().to(handlers::strategies::engine_crud::delete_fingerprint),
            )
            .route(
                "/strategy-rules",
                web::get().to(handlers::strategies::engine_crud::list_rules),
            )
            .route(
                "/strategy-rules",
                web::post().to(handlers::strategies::engine_crud::create_rule),
            )
            // Bulk lifecycle — literal segments before `{id}/...` so they never bind `{id}`.
            // Lab stop/stop-all deactivate only (no engine / positions to force-close).
            .route(
                "/strategy-rules/pause-all",
                web::post().to(handlers::strategies::engine_crud::pause_all_rules),
            )
            .route(
                "/strategy-rules/stop-all",
                web::post().to(handlers::strategies::engine_crud::stop_all_rules),
            )
            .route(
                "/strategy-rules/{id}",
                web::get().to(handlers::strategies::engine_crud::get_rule),
            )
            .route(
                "/strategy-rules/{id}",
                web::put().to(handlers::strategies::engine_crud::update_rule),
            )
            .route(
                "/strategy-rules/{id}",
                web::delete().to(handlers::strategies::engine_crud::delete_rule),
            )
            // Run history + finalized metrics for the Evidence run navigator (read
            // -only twin of the live route; served off the synced mirror).
            .route(
                "/strategy-rules/{id}/runs",
                web::get().to(handlers::strategies::live_positions::list_rule_runs),
            )
            .route(
                "/strategy-rules/{id}/activate",
                web::post().to(handlers::strategies::engine_crud::activate_rule),
            )
            .route(
                "/strategy-rules/{id}/pause",
                web::post().to(handlers::strategies::engine_crud::pause_rule),
            )
            .route(
                "/strategy-rules/{id}/enable",
                web::post().to(handlers::strategies::engine_crud::enable_rule),
            )
            .route(
                "/strategy-rules/{id}/disable",
                web::post().to(handlers::strategies::engine_crud::disable_rule),
            )
            .route(
                "/strategy-rules/{id}/stop",
                web::post().to(handlers::strategies::engine_crud::stop_rule),
            )
            // ── Grouped param-sweeps (generic across strategies) ──
            .route(
                "/strategies/sweeps",
                web::get().to(handlers::strategies::grouped_sweep::list_runs),
            )
            .route(
                "/strategies/sweeps",
                web::post().to(handlers::strategies::grouped_sweep::start_grouped_sweep),
            )
            .route(
                "/strategies/sweeps",
                web::delete().to(handlers::strategies::grouped_sweep::prune_runs),
            )
            .route(
                "/strategies/sweeps/cancel",
                web::post().to(handlers::strategies::grouped_sweep::cancel_grouped_sweep),
            )
            .route(
                "/strategies/sweeps/{run_id}",
                web::delete().to(handlers::strategies::grouped_sweep::delete_run),
            )
            .route(
                "/strategies/sweeps/{run_id}",
                web::patch().to(handlers::strategies::grouped_sweep::rename_run),
            )
            .route(
                "/strategies/sweeps/{run_id}/groups",
                web::get().to(handlers::strategies::grouped_sweep::list_groups),
            )
            .route(
                "/strategies/sweeps/{run_id}/groups/{group_id}/results",
                web::get().to(handlers::strategies::grouped_sweep::list_results),
            )
            .route(
                "/strategies/sweeps/{run_id}/groups/{group_id}/token-results",
                web::get().to(handlers::strategies::grouped_sweep::list_token_results),
            )
            .route(
                "/strategies/sweeps/{run_id}/groups/{group_id}/promote",
                web::post().to(handlers::strategies::grouped_sweep::promote_group),
            )
            // ── Flow discovery (volume ix-structure scoring) ──
            .route(
                "/strategies/flow-discovery",
                web::post().to(handlers::strategies::flow_discovery::start_flow_discovery),
            )
            .route(
                "/strategies/flow-discovery/cancel",
                web::post().to(handlers::strategies::flow_discovery::cancel_flow_discovery),
            )
            .route(
                "/strategies/flow-discovery/bind",
                web::post().to(handlers::strategies::flow_discovery::bind_flow_discovery),
            )
            .route(
                "/strategies/flow-discovery/last",
                web::get().to(handlers::strategies::flow_discovery::get_last_flow_discovery),
            )
            .route(
                "/strategies/flow-discovery/{run_id}",
                web::get().to(handlers::strategies::flow_discovery::get_flow_discovery),
            ),
    );
}
