//! Paper execution: mirror the real buy/sell lifecycle against the WS/DB trade
//! feed without sending any transaction. Entry and trade-driven exit each poll
//! the feed for the confirming trade (spawned tasks); a clock-driven exit on a
//! silent token records immediately at the last observed price.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tokio::sync::broadcast;
use tokio::time::sleep;
use tracing::{info, warn};
use uuid::Uuid;

use super::super::util::none_if_zero_u64;
use super::super::Tpsl2RuntimeCache;
use crate::models::ingest::SseEvent;
use crate::models::{Position, PositionStatus, StrategyTPSLRule};
use crate::storage::repositories::{
    tpsl2_paper_trading_repo::Tpsl2PaperTradingRepo, trade_repo::TradeRepo,
};

/// Poll the WS-fed trade feed for the token's opening trades and record the
/// paper entry on first sight — exactly as real mode polls for its on-chain
/// fill. The entry price/tx/time come only from a real indexed trade; paper mode
/// never synthesizes a fill from a create-time snapshot. If no fill is indexed
/// within the window, the unentered position is dropped.
pub(crate) fn spawn_entry_fill_poll(
    trade_repo: TradeRepo,
    paper_repo: Tpsl2PaperTradingRepo,
    runtime: Arc<Tpsl2RuntimeCache>,
    mint: String,
    position_id: Uuid,
    buy_amount: f64,
) {
    tokio::spawn(async move {
        let mut recorded = false;
        for _ in 0..super::BUY_POLL_MAX_ATTEMPTS {
            sleep(Duration::from_millis(super::BUY_POLL_INTERVAL_MS)).await;
            let Ok(trades) = trade_repo.find_by_mint_all(&mint).await else {
                continue;
            };
            if let Some(fill) = super::super::entry::find_entry_fill_in_trades(&trades, 5) {
                if let Ok(Some(prev)) = paper_repo.find_by_id(position_id).await {
                    let _ = paper_repo
                        .update_entry(
                            position_id,
                            &fill.tx_signature,
                            buy_amount,
                            fill.price,
                            fill.block_time,
                        )
                        .await;
                    if let Ok(Some(current)) = paper_repo.find_by_id(position_id).await {
                        runtime.sync_position(Some(&prev), &current);
                    }
                }
                info!(
                    "[PAPER] Set entry for position {}: {} (tx: {})",
                    position_id, fill.price, fill.tx_signature
                );
                recorded = true;
                break;
            }
        }
        if !recorded {
            // No real fill was indexed within the poll window. Mirror real mode's
            // "buy not found" cleanup: drop the unentered position rather than leave
            // a 0-entry row that can never trade. No create-time price is synthesized.
            if let Ok(Some(pos)) = paper_repo.find_by_id(position_id).await {
                if pos.entry_price <= 0.0 {
                    let _ = paper_repo.delete_position(position_id).await;
                    runtime.remove_position(&pos);
                    info!(
                        "[PAPER] Removed position {} for mint {}: no entry fill indexed",
                        position_id, mint
                    );
                }
            }
        }
    });
}

/// Poll the WS-fed trade feed for the confirming exit trade and record the paper
/// exit via the same exit ladder that triggered it. If none is indexed within
/// the window, mark the position ExitFailed at the trigger price (terminal — no
/// revert to Holding). Either terminal outcome may complete the run.
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_exit_fill_poll(
    trade_repo: TradeRepo,
    paper_repo: Tpsl2PaperTradingRepo,
    runtime: Arc<Tpsl2RuntimeCache>,
    pool: PgPool,
    sse_tx: broadcast::Sender<SseEvent>,
    mint: String,
    position_id: Uuid,
    entry_tx: String,
    entry_price: f64,
    entry_time_db: Option<DateTime<Utc>>,
    // The full rule drives the exit-fill resolver so the recorded paper exit
    // honors the same E1–E4 ladder that triggered the gate.
    rule: StrategyTPSLRule,
    // The price/time the exit condition met; the hypothetical exit if no real
    // fill confirms. `trigger_reason` is recorded on the ExitFailed fallback.
    trigger_price: f64,
    trigger_time: DateTime<Utc>,
    trigger_reason: String,
) {
    tokio::spawn(async move {
        let rule_id = rule.id;
        let max_total = none_if_zero_u64(rule.p_max_total_tokens);
        let mut found = false;
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(10) {
            if let Ok(trades) = trade_repo.find_by_mint_all(&mint).await {
                // Prefer the entry trade's own block time; fall back to the
                // entry_time stored on the position (set together with entry_price).
                // The old `Utc::now()` fallback made every trade look pre-entry
                // whenever the entry row wasn't in the fetched set, so the walk saw
                // nothing and the position always reverted to Holding.
                let entry_block_time = trades
                    .iter()
                    .find(|t| t.tx_signature == entry_tx)
                    .map(|t| t.block_time)
                    .or(entry_time_db)
                    .unwrap_or_else(chrono::Utc::now);
                if let Some(fill) =
                    super::super::exit::find_trade_driven_exit(&trades, entry_block_time, entry_price, &rule)
                {
                    if let Ok(Some(prev)) = paper_repo.find_by_id(position_id).await {
                        let _ = paper_repo
                            .update_exit(
                                position_id,
                                &fill.tx_signature,
                                fill.price,
                                fill.block_time,
                                fill.reason.as_str(),
                            )
                            .await;
                        if let Ok(Some(current)) = paper_repo.find_by_id(position_id).await {
                            runtime.sync_position(Some(&prev), &current);
                        }
                    }
                    info!(
                        "[PAPER] Set exit for position {}: {} (tx: {}, reason: {})",
                        position_id, fill.price, fill.tx_signature, fill.reason
                    );
                    found = true;
                    break;
                }
            }
            sleep(Duration::from_millis(500)).await;
        }
        if !found {
            // No confirming exit trade was indexed within the poll window. Per the
            // terminal-failure design this is the end of the line — mark ExitFailed
            // rather than reverting to Holding. The exit price only ever comes from a
            // real indexed trade via `find_trade_driven_exit`, so there is nothing to fill.
            if let Ok(Some(prev)) = paper_repo.find_by_id(position_id).await {
                let _ = paper_repo
                    .mark_exit_failed(position_id, trigger_price, trigger_time, &trigger_reason)
                    .await;
                if let Ok(Some(current)) = paper_repo.find_by_id(position_id).await {
                    runtime.sync_position(Some(&prev), &current);
                }
            }
            info!(
                "[PAPER] No exit fill indexed; marked position {} ExitFailed at {}",
                position_id, trigger_price
            );
        }

        // A terminal outcome (clean exit or failed exit) may have been the last
        // open position — the run can now complete.
        if let Ok(Some(pos)) = paper_repo.find_by_id(position_id).await {
            if matches!(pos.status, PositionStatus::End | PositionStatus::ExitFailed) {
                super::super::paper_run::finish_paper_run_if_complete(
                    &pool, &runtime, &sse_tx, rule_id, &rule.rule_name, max_total,
                )
                .await;
            }
        }
    });
}

/// Record a paper clock-driven exit immediately at the last observed price.
/// Unlike the trade-driven resolver, this does **not** poll for a confirming
/// trade — a time/stall exit on a silent token has none by definition. The last
/// seen price is the honest mark ("held N seconds, sell at the current mark").
/// Transitions Holding→End in one write and, if the run's cap is met with
/// nothing left open, finishes the run.
pub(crate) async fn record_time_exit(
    paper_repo: &Tpsl2PaperTradingRepo,
    runtime: &Arc<Tpsl2RuntimeCache>,
    pool: &PgPool,
    sse_tx: &broadcast::Sender<SseEvent>,
    mut position: Position,
    exit_price: f64,
    exit_time: DateTime<Utc>,
    reason: String,
    rule: &StrategyTPSLRule,
) {
    let exit_tx = format!("paper-time-exit-{}", position.id);
    let prev = position.clone();
    if let Err(err) = paper_repo
        .update_exit(position.id, &exit_tx, exit_price, exit_time, &reason)
        .await
    {
        warn!(position_id = %position.id, "Failed to record paper time exit: {err}");
        return;
    }
    // Reflect the close in the snapshot synced to the runtime cache.
    position.close(exit_price, exit_tx, position.entry_amount, exit_time);
    position.exit_reason = Some(reason);
    runtime.sync_position(Some(&prev), &position);
    info!(
        position_id = %position.id, mint = %position.mint, exit_price,
        "[PAPER] Time-driven exit recorded"
    );

    super::super::paper_run::finish_paper_run_if_complete(
        pool,
        runtime,
        sse_tx,
        rule.id,
        &rule.rule_name,
        none_if_zero_u64(rule.p_max_total_tokens),
    )
    .await;
}
