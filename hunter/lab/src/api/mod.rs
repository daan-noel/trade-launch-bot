pub mod handlers;

use actix_web::web;

/// Register local-only (analysis box) routes: the swing-aware token list, swing
/// detection, background-job control, the tpsl1/tpsl2 rule authoring + backtest
/// edge, and grouped param-sweeps. Call alongside
/// `trading_core::api::configure_core_routes` to build the full local route set
/// (`App::new().configure(configure_core_routes).configure(configure_local_routes)`).
///
/// Live-trading routes (positions, lifecycle, matched, sync, live-mode, on-chain
/// Solana, cashback) are deploy-only and are **not** registered here, so they 404
/// on the local bin.
pub fn configure_local_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api")
            // Token list (unified TableRequest POST; swing-aware)
            .route("/tokens", web::post().to(handlers::tokens::list_tokens))
            // Matched mint-address set only (same filter body) — Swing Detection All
            // fans out over this instead of pulling every full row.
            .route("/tokens/mints", web::post().to(handlers::tokens::list_token_mints))
            // Swing detection
            .route(
                "/tokens/swings/batch",
                web::post().to(handlers::tokens::detect_tokens_swings_batch),
            )
            .route(
                "/tokens/{mint}/swings",
                web::post().to(handlers::tokens::detect_token_swings),
            )
            .route(
                "/tokens/{mint}/swing1-detect",
                web::post().to(handlers::tokens::detect_token_swing1),
            )
            // Redesign: every metric's series over a token's trades (chart panes)
            .route(
                "/tokens/{mint}/metric-series",
                web::get().to(handlers::tokens::token_metric_series),
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
            // Background-job status + control (sweep / simulation / swing)
            .route("/jobs/status", web::get().to(handlers::system::job_status))
            .route(
                "/jobs/simulations/{rule_id}/cancel",
                web::post().to(handlers::system::cancel_simulation),
            )
            .route(
                "/jobs/simulations/{rule_id}/result",
                web::get().to(handlers::system::simulation_result),
            )
            .route("/jobs/swings/{run_id}/cancel", web::post().to(handlers::system::cancel_swing))
            .route("/jobs/swings/{run_id}/result", web::get().to(handlers::system::swing_result))
            // ── Generic engine simulate (redesign) — one surface for every rule ──
            .route(
                "/strategies/simulate",
                web::post().to(handlers::strategies::engine::simulate_engine),
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
            // ── Strategy rule authoring + backtest — tpsl_sniper_1 ──
            .route(
                "/strategies/tpsl1/rules",
                web::get().to(handlers::strategies::tpsl1::list_tpsl_rules),
            )
            .route(
                "/strategies/tpsl1/rules",
                web::post().to(handlers::strategies::tpsl1::create_tpsl_rule),
            )
            .route(
                "/strategies/tpsl1/rules/simulate-all",
                web::post().to(handlers::strategies::tpsl1::simulate_all_tpsl_rules),
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
                "/strategies/tpsl1/rules/{rule_id}/simulate",
                web::post().to(handlers::strategies::tpsl1::simulate_tpsl_rule),
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
                web::post().to(handlers::strategies::tpsl1::get_matched_tokens),
            )
            .route(
                "/strategies/tpsl1/rules/{rule_id}/positions/summary",
                web::post().to(handlers::strategies::tpsl1::get_positions_summary_tpsl1),
            )
            .route(
                "/strategies/tpsl1/rules/{rule_id}/positions",
                web::post().to(handlers::strategies::tpsl1::get_positions_by_rule_tpsl1),
            )
            .route(
                "/strategies/tpsl1/rules/{rule_id}/simulate/result/summary",
                web::post().to(handlers::strategies::tpsl1::get_simulate_result_summary_tpsl1),
            )
            .route(
                "/strategies/tpsl1/rules/{rule_id}/simulate/result",
                web::post().to(handlers::strategies::tpsl1::get_simulate_result_tpsl1),
            )
            // ── Strategy rule authoring + backtest — tpsl_sniper_2 ──
            .route(
                "/strategies/tpsl2/rules",
                web::get().to(handlers::strategies::tpsl2::list_tpsl_rules),
            )
            .route(
                "/strategies/tpsl2/rules",
                web::post().to(handlers::strategies::tpsl2::create_tpsl_rule),
            )
            .route(
                "/strategies/tpsl2/rules/simulate-all",
                web::post().to(handlers::strategies::tpsl2::simulate_all_tpsl_rules),
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
                "/strategies/tpsl2/rules/{rule_id}/simulate",
                web::post().to(handlers::strategies::tpsl2::simulate_tpsl_rule),
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
                web::post().to(handlers::strategies::tpsl2::get_matched_tokens),
            )
            .route(
                "/strategies/tpsl2/rules/{rule_id}/positions/summary",
                web::post().to(handlers::strategies::tpsl2::get_positions_summary_tpsl2),
            )
            .route(
                "/strategies/tpsl2/rules/{rule_id}/positions",
                web::post().to(handlers::strategies::tpsl2::get_positions_by_rule_tpsl2),
            )
            .route(
                "/strategies/tpsl2/rules/{rule_id}/simulate/result/summary",
                web::post().to(handlers::strategies::tpsl2::get_simulate_result_summary_tpsl2),
            )
            .route(
                "/strategies/tpsl2/rules/{rule_id}/simulate/result",
                web::post().to(handlers::strategies::tpsl2::get_simulate_result_tpsl2),
            )
            // ── Strategy rule authoring + backtest — swing_1 ──
            .route(
                "/strategies/swing1/rules",
                web::get().to(handlers::strategies::swing1::list_swing1_rules),
            )
            .route(
                "/strategies/swing1/rules",
                web::post().to(handlers::strategies::swing1::create_swing1_rule),
            )
            .route(
                "/strategies/swing1/rules/simulate-all",
                web::post().to(handlers::strategies::swing1::simulate_all_swing1_rules),
            )
            .route(
                "/strategies/swing1/rules/{rule_id}",
                web::get().to(handlers::strategies::swing1::get_swing1_rule),
            )
            .route(
                "/strategies/swing1/rules/{rule_id}",
                web::put().to(handlers::strategies::swing1::update_swing1_rule),
            )
            .route(
                "/strategies/swing1/rules/{rule_id}",
                web::delete().to(handlers::strategies::swing1::delete_swing1_rule),
            )
            .route(
                "/strategies/swing1/rules/{rule_id}/simulate",
                web::post().to(handlers::strategies::swing1::simulate_swing1_rule),
            )
            .route(
                "/strategies/swing1/rules/{rule_id}/simulate/cancel",
                web::post().to(handlers::strategies::swing1::cancel_simulate_swing1_rule),
            )
            .route(
                "/strategies/swing1/rules/{rule_id}/paper-result",
                web::get().to(handlers::strategies::swing1::paper_result_swing1_rule),
            )
            .route(
                "/strategies/swing1/rules/{rule_id}/paper-result",
                web::delete().to(handlers::strategies::swing1::clear_paper_result_swing1_rule),
            )
            .route(
                "/strategies/swing1/rules/{rule_id}/matched",
                web::post().to(handlers::strategies::swing1::get_matched_tokens),
            )
            .route(
                "/strategies/swing1/rules/{rule_id}/positions/summary",
                web::post().to(handlers::strategies::swing1::get_positions_summary_swing1),
            )
            .route(
                "/strategies/swing1/rules/{rule_id}/positions",
                web::post().to(handlers::strategies::swing1::get_positions_by_rule_swing1),
            )
            .route(
                "/strategies/swing1/rules/{rule_id}/simulate/result/summary",
                web::post().to(handlers::strategies::swing1::get_simulate_result_summary_swing1),
            )
            .route(
                "/strategies/swing1/rules/{rule_id}/simulate/result",
                web::post().to(handlers::strategies::swing1::get_simulate_result_swing1),
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
            ),
    );
}
