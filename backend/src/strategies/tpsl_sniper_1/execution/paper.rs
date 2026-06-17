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
use super::super::Tpsl1RuntimeCache;
use crate::models::ingest::SseEvent;
use crate::models::{Position, PositionStatus, Tpsl1Rule};
use crate::state::token_cache::{CachedTrade, TokenCache};
use crate::storage::repositories::tpsl1_paper_trading_repo::Tpsl1PaperTradingRepo;

/// Snapshot the mint's retained trade window from the in-memory token cache
/// (oldest-first), or `None` if the token isn't tracked. The WS pipeline keeps
/// this current for every trade on the mint (any wallet), so the paper fill polls
/// read it instead of issuing an unbounded `find_by_mint_all` DB scan every tick
/// (H5/H6). The clone is bounded by `MAX_TRADES_RETAINED`; per-trade
/// `instruction_labels` are not read by any fill resolver, so the cache ring's
/// stripped labels don't matter here.
///
/// Returns the shared `Arc` so the snapshot is a refcount bump under the shard
/// guard, not a deep copy of up to `MAX_TRADES_RETAINED` trades. Also returns the
/// token's lifetime `trade_count`, sampled under the same guard, so a poll can
/// skip its O(n) fill walk on an idle tick where no new trade landed (the count
/// is monotonic; the resolvers watch ALL wallets on the mint, so there's no
/// per-(wallet,mint) `TradeSignals` key to wake on instead).
fn cache_trades(token_cache: &TokenCache, mint: &str) -> Option<(Arc<Vec<CachedTrade>>, u64)> {
    token_cache
        .get(mint)
        .map(|e| (e.value().trades.clone(), e.value().trade_count))
}

/// Poll the WS-fed trade feed for the token's opening trades and record the
/// paper entry on first sight — exactly as real mode polls for its on-chain
/// fill. The entry price/tx/time come only from a real indexed trade; paper mode
/// never synthesizes a fill from a create-time snapshot. If no fill is indexed
/// within the window, the unentered position is dropped.
pub(crate) fn spawn_entry_fill_poll(
    paper_repo: Tpsl1PaperTradingRepo,
    runtime: Arc<Tpsl1RuntimeCache>,
    token_cache: Arc<TokenCache>,
    mint: String,
    position_id: Uuid,
    buy_amount: f64,
) {
    let poll_sem = runtime.paper_poll_sem();
    tokio::spawn(async move {
        // Bound concurrent fill-poll tasks; held for the task's lifetime.
        let _permit = poll_sem.acquire_owned().await;
        let mut recorded = false;
        // Skip the O(n) fill walk on ticks where no new trade landed for the mint
        // (the count is monotonic; `None` forces the first walk).
        let mut last_count: Option<u64> = None;
        for _ in 0..super::BUY_POLL_MAX_ATTEMPTS {
            sleep(Duration::from_millis(super::BUY_POLL_INTERVAL_MS)).await;
            // Read the mint's trade window from the in-memory cache (kept current
            // by the WS pipeline) instead of an unbounded DB scan per tick.
            let Some((trades, trade_count)) = cache_trades(&token_cache, &mint) else {
                continue;
            };
            if last_count == Some(trade_count) {
                continue;
            }
            last_count = Some(trade_count);
            if let Some(fill) = super::super::entry::find_entry_fill_in_trades(&trades, 5) {
                if let Ok(Some(prev)) = paper_repo.find_by_id(position_id).await {
                    // `update_entry` RETURNs the updated row — sync off it directly
                    // instead of reading back the row we just wrote.
                    if let Ok(current) = paper_repo
                        .update_entry(
                            position_id,
                            &fill.tx_signature,
                            buy_amount,
                            fill.price,
                            fill.block_time,
                        )
                        .await
                    {
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
    paper_repo: Tpsl1PaperTradingRepo,
    runtime: Arc<Tpsl1RuntimeCache>,
    token_cache: Arc<TokenCache>,
    pool: PgPool,
    sse_tx: broadcast::Sender<SseEvent>,
    mint: String,
    position_id: Uuid,
    entry_tx: String,
    entry_price: f64,
    entry_time_db: Option<DateTime<Utc>>,
    // The full rule drives the exit-fill resolver so the recorded paper exit
    // honors the same E1–E4 ladder that triggered the gate.
    rule: Tpsl1Rule,
    // The price/time the exit condition met; the hypothetical exit if no real
    // fill confirms. `trigger_reason` is recorded on the ExitFailed fallback.
    trigger_price: f64,
    trigger_time: DateTime<Utc>,
    trigger_reason: String,
) {
    let poll_sem = runtime.paper_poll_sem();
    tokio::spawn(async move {
        // Bound concurrent fill-poll tasks; held for the task's lifetime.
        let _permit = poll_sem.acquire_owned().await;
        let rule_id = rule.id;
        let max_total = none_if_zero_u64(rule.p_max_total_tokens);
        let mut found = false;
        // Skip the O(n) exit-fill walk on ticks where no new trade landed for the
        // mint (count is monotonic; `None` forces the first walk).
        let mut last_count: Option<u64> = None;
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(super::PAPER_EXIT_POLL_WINDOW_SECS) {
            // Confirm the fill from the in-memory cache window (kept current by the
            // WS pipeline) instead of an unbounded `find_by_mint_all` DB scan per
            // tick. A `None` (token untracked) skips this tick like an empty fetch.
            if let Some((trades, trade_count)) =
                cache_trades(&token_cache, &mint).filter(|(_, c)| last_count != Some(*c))
            {
                last_count = Some(trade_count);
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
                        // `update_exit` returns the updated row (RETURNING), so we
                        // sync runtime state directly without a read-back.
                        if let Ok(current) = paper_repo
                            .update_exit(
                                position_id,
                                &fill.tx_signature,
                                fill.price,
                                fill.block_time,
                                fill.reason.as_str(),
                            )
                            .await
                        {
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
            sleep(Duration::from_millis(super::PAPER_EXIT_POLL_INTERVAL_MS)).await;
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

        // Release the shared exit claim now the fill-poll is done (the caller
        // claimed it before spawning to bound this to one task per position).
        runtime.end_exit(position_id);
    });
}

/// Record a paper clock-driven exit immediately at the last observed price.
/// Unlike the trade-driven resolver, this does **not** poll for a confirming
/// trade — a time/stall exit on a silent token has none by definition. The last
/// seen price is the honest mark ("held N seconds, sell at the current mark").
/// Transitions Holding→End in one write and, if the run's cap is met with
/// nothing left open, finishes the run.
pub(crate) async fn record_time_exit(
    paper_repo: &Tpsl1PaperTradingRepo,
    runtime: &Arc<Tpsl1RuntimeCache>,
    pool: &PgPool,
    sse_tx: &broadcast::Sender<SseEvent>,
    mut position: Position,
    exit_price: f64,
    exit_time: DateTime<Utc>,
    reason: String,
    rule: &Tpsl1Rule,
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
