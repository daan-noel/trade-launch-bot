//! Rule lifecycle transitions — the **single source of truth** for activate /
//! pause / stop-and-close. The API handlers call these so the paper-run and
//! rules-cache side effects can never drift between code paths (the bug the old
//! inline `update` toggle invited, where activation and the auto-finish path
//! flipped `is_active` through different routes).
//!
//! The three transitions:
//!   - [`activate_rule`]  — entries on; (re)start a paper run (fresh or continue).
//!   - [`pause_rule`]     — entries off; open positions drain via the exit ladder.
//!   - [`stop_and_close_rule`] — pause **and** force-close every open position now.

use std::sync::Arc;

use anyhow::anyhow;
use chrono::Utc;
use tracing::warn;
use uuid::Uuid;

use super::util::none_if_zero_u64;
use crate::models::{Position, Tpsl2StrategyRule};
use crate::state::app_state::AppState;
use crate::storage::repositories::{
    tpsl2_paper_trading_repo::Tpsl2PaperTradingRepo, tpsl2_position_repo::Tpsl2PositionRepo,
    tpsl2_strategy_rule_repo::Tpsl2StrategyRuleRepo, trade_repo::TradeRepo,
};

/// How a paper rule's run is (re)started when the rule is activated.
#[derive(Clone, Copy, Debug)]
pub enum PaperActivation {
    /// Start a brand-new run, dropping the prior run's positions + counters.
    Fresh,
    /// Resume the rule's prior run, keeping its positions + counters (falls back
    /// to `Fresh` if the rule has no prior run).
    Continue,
}

/// Exit reason stamped on positions force-closed by Stop & close.
const MANUAL_CLOSE_REASON: &str = "ManualClose";

/// Activate a rule (entries on). For a paper rule this also (re)starts its run
/// per `paper`. Persists `is_active = true`, applies the paper-run side effect,
/// then refreshes the rules cache so the strategy hot path sees the rule. Returns
/// the updated rule.
pub async fn activate_rule(
    app_state: &Arc<AppState>,
    rule_id: Uuid,
    paper: PaperActivation,
) -> anyhow::Result<Tpsl2StrategyRule> {
    let repo = Tpsl2StrategyRuleRepo::new(app_state.db.clone());
    let mut rule = repo
        .find_by_id(rule_id)
        .await?
        .ok_or_else(|| anyhow!("Rule not found"))?;

    rule.is_active = true;
    repo.update(&rule).await?;

    if rule.trade_mode == "paper" {
        let max_total = none_if_zero_u64(rule.p_max_total_tokens);
        match paper {
            PaperActivation::Fresh => {
                app_state
                    .tpsl2_cache
                    .start_paper_run(&app_state.db, rule_id, max_total)
                    .await?;
            }
            PaperActivation::Continue => {
                // Resume the prior run; fall back to a fresh one if there is none.
                if app_state
                    .tpsl2_cache
                    .resume_paper_run(&app_state.db, rule_id)
                    .await?
                    .is_none()
                {
                    app_state
                        .tpsl2_cache
                        .start_paper_run(&app_state.db, rule_id, max_total)
                        .await?;
                }
            }
        }
    }

    app_state.tpsl2_cache.reload_rules(&app_state.db).await?;
    Ok(rule)
}

/// Pause a rule (entries off; open positions left to drain via the exit ladder —
/// `on_trade_executed` / the time sweep still exit them). For a paper rule this
/// marks the current run `Stopped`. Returns the updated rule.
pub async fn pause_rule(
    app_state: &Arc<AppState>,
    rule_id: Uuid,
) -> anyhow::Result<Tpsl2StrategyRule> {
    let repo = Tpsl2StrategyRuleRepo::new(app_state.db.clone());
    let mut rule = repo
        .find_by_id(rule_id)
        .await?
        .ok_or_else(|| anyhow!("Rule not found"))?;
    let was_active = rule.is_active;

    rule.is_active = false;
    repo.update(&rule).await?;

    if rule.trade_mode == "paper" && was_active {
        app_state
            .tpsl2_cache
            .stop_paper_run(&app_state.db, rule_id)
            .await?;
    }
    app_state.tpsl2_cache.reload_rules(&app_state.db).await?;
    Ok(rule)
}

/// Stop a rule **and force-close every open position now**. Pauses first (so a
/// paper run is `Stopped` and the cap-reached finish logic can't fire mid-close),
/// then exits each holding position at its last observed price: paper rows close
/// in place; real rows mark `ExitPending` and sell on-chain in a spawned task so
/// a many-position close doesn't block the response. Returns the updated rule and
/// the number of positions that were closing.
pub async fn stop_and_close_rule(
    app_state: &Arc<AppState>,
    rule_id: Uuid,
) -> anyhow::Result<(Tpsl2StrategyRule, usize)> {
    let rule = pause_rule(app_state, rule_id).await?;

    // Authoritative snapshot of this rule's open positions — the in-memory
    // holding index is the live source of truth across both paper and real.
    let positions: Vec<Position> = app_state
        .tpsl2_cache
        .all_holding_positions()
        .into_iter()
        .filter(|p| p.rule_id == rule_id)
        .map(|p| (*p).clone())
        .collect();
    let count = positions.len();
    let now = Utc::now();

    for position in positions {
        // The last observed mark is the honest exit price; fall back to entry for
        // a token the WS cache has since evicted.
        let exit_price = app_state
            .token_cache
            .get(&position.mint)
            .and_then(|e| e.value().current_price)
            .unwrap_or(position.entry_price);

        if rule.trade_mode == "paper" {
            let paper_repo = Tpsl2PaperTradingRepo::new(app_state.db.clone());
            super::execution::paper::record_time_exit(
                &paper_repo,
                &app_state.tpsl2_cache,
                &app_state.db,
                &app_state.sse_tx,
                position,
                exit_price,
                now,
                MANUAL_CLOSE_REASON.to_string(),
                &rule,
            )
            .await;
        } else {
            // Real: sell on-chain. Spawn so the slow sell-with-retries loop runs
            // off the request path; the table reflects the drain via polling.
            let trader = app_state.trader.clone();
            let runtime = app_state.tpsl2_cache.clone();
            let token_cache = app_state.token_cache.clone();
            let position_repo = Tpsl2PositionRepo::new(app_state.db.clone());
            let trade_repo = TradeRepo::new(app_state.db.clone());
            let trade_signals = app_state.trade_signals.clone();
            tokio::spawn(async move {
                let mut pos = position;
                let prev = pos.clone();
                pos.mark_exit_pending();
                if let Err(err) = position_repo.update(&pos).await {
                    warn!(
                        "Failed to mark position {} ExitPending on manual close: {err}",
                        pos.id
                    );
                } else {
                    runtime.sync_position(Some(&prev), &pos);
                }
                super::execution::real::sell_and_close_position(
                    trader,
                    pos,
                    position_repo,
                    trade_repo,
                    runtime,
                    &token_cache,
                    trade_signals,
                    exit_price,
                    now,
                    MANUAL_CLOSE_REASON.to_string(),
                )
                .await;
            });
        }
    }

    Ok((rule, count))
}
