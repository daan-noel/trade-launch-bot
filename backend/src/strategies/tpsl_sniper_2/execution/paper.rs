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
use super::super::{ExitGuard, Tpsl2RuntimeCache};
use crate::config::constants::EXIT_SLIPPAGE_SLOTS;
use crate::models::ingest::SseEvent;
use crate::state::token_cache::CachedTrade;
use crate::models::{Position, PositionStatus, Tpsl2Rule};
use crate::state::token_cache::TokenCache;
use crate::storage::repositories::tpsl2_paper_trading_repo::Tpsl2PaperTradingRepo;
use crate::storage::repositories::trade_repo;

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
///
/// Entry resolution mirrors the backtest: the fill is the first trade where every
/// configured scalp gate holds ([`find_scalp_entry`]). tpsl2 has no legacy
/// first-slot fallback — a rule with no scalp gates never resolves a fill and the
/// position is dropped — so paper entries always honor `p_entry_*` and agree with
/// simulation.
pub(crate) fn spawn_entry_fill_poll(
    paper_repo: Tpsl2PaperTradingRepo,
    runtime: Arc<Tpsl2RuntimeCache>,
    token_cache: Arc<TokenCache>,
    mint: String,
    position_id: Uuid,
    rule: Tpsl2Rule,
) {
    let buy_amount = rule.buy_amount;
    let pool = paper_repo.pool().clone();
    let poll_sem = runtime.paper_poll_sem();
    tokio::spawn(async move {
        // Bound concurrent fill-poll tasks; held for the task's lifetime.
        let _permit = poll_sem.acquire_owned().await;
        let mut recorded = false;
        // Watch the live feed for the scalp entry signal over the arming window,
        // sized to this rule's own gates (`scalp_arming_attempts`) so slow gates
        // (min-age, higher-low) always have time to form. A signal needing longer
        // than the window is missed live but still found by the backtest (no cutoff).
        let attempts = super::scalp_arming_attempts(&rule);
        // Skip the O(n) scalp-entry walk on ticks where no new trade landed for the
        // mint (count is monotonic; `None` forces the first walk).
        let mut last_count: Option<u64> = None;
        for _ in 0..attempts {
            sleep(Duration::from_millis(super::SCALP_ENTRY_WAIT_INTERVAL_MS)).await;
            // Read the mint's trade window from the in-memory cache (kept current
            // by the WS pipeline) instead of an unbounded DB scan per tick.
            let Some((trades, trade_count)) = cache_trades(&token_cache, &mint) else {
                continue;
            };
            if last_count == Some(trade_count) {
                continue;
            }
            last_count = Some(trade_count);
            // Resolve the trigger by **index** (Phase B step 1): the cache row carries
            // no signature, so the worst-case entry is keyed off the trigger's position
            // in `trades`, not a sig match. `scalp_cohort` + the indexed resolver give
            // the exact same trigger the sig-keyed `find_scalp_entry` did.
            let cohort = super::super::entry::scalp_cohort(&trades);
            if let Some((trigger_idx, target_fill)) =
                super::super::entry::find_scalp_entry_with_cohort_indexed(&trades, &rule, &cohort)
            {
                // The trigger trade is the *target*; the recorded *entry* is the
                // worst-case adverse fill in the trigger's block (and the next), so
                // paper has a real target↔entry gap. They coincide only in the
                // fallback case where no adverse trade exists.
                let entry = super::super::entry::find_worst_case_paper_entry_at(&trades, trigger_idx);
                // Recover real tx_signatures from the DB (cache strips them, Phase B).
                let target_tx = trade_repo::find_tx_by_fill(&pool, &mint, target_fill.block_time, target_fill.price)
                    .await
                    .unwrap_or_default()
                    .unwrap_or_default();
                let entry_tx = trade_repo::find_tx_by_fill(&pool, &mint, entry.block_time, entry.price)
                    .await
                    .unwrap_or_default()
                    .unwrap_or_default();
                if let Ok(Some(prev)) = paper_repo.find_by_id(position_id).await {
                    // Persist the trigger as the target, then the worst-case entry.
                    // Each writer RETURNs the updated row; sync off the latest so the
                    // cached snapshot carries both target_* and entry_*.
                    if let Err(err) = paper_repo
                        .update_target(
                            position_id,
                            target_fill.price,
                            target_fill.amount_tokens,
                            target_fill.block_time,
                            &target_tx,
                        )
                        .await
                    {
                        warn!("[PAPER] Failed to record target for position {position_id}: {err}");
                    }
                    // Paper entry size is the token count `buy_amount / entry_price`
                    // (SOL is derived at display); guard a 0 price so we never divide
                    // by ~0. The worst-case entry's own token amount is not used here.
                    let entry_token_amount =
                        if entry.price > 0.0 { buy_amount / entry.price } else { 0.0 };
                    if let Ok(current) = paper_repo
                        .update_entry(position_id, &entry_tx, entry_token_amount, entry.price, entry.block_time)
                        .await
                    {
                        runtime.sync_position(Some(&prev), &current);
                    }
                }
                info!(
                    "[PAPER] Set target/entry for position {}: target {} (tx: {}) → entry {} (tx: {})",
                    position_id, target_fill.price, target_tx, entry.price, entry_tx
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
                if pos.entry_price.is_none() {
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
    paper_repo: Tpsl2PaperTradingRepo,
    runtime: Arc<Tpsl2RuntimeCache>,
    token_cache: Arc<TokenCache>,
    pool: PgPool,
    sse_tx: broadcast::Sender<SseEvent>,
    mint: String,
    position_id: Uuid,
    entry_price: f64,
    entry_time_db: Option<DateTime<Utc>>,
    // The full rule drives the exit-fill resolver so the recorded paper exit
    // honors the same E1–E4 ladder that triggered the gate.
    rule: Tpsl2Rule,
    // The price/time the exit condition met; the hypothetical exit if no real
    // fill confirms. `trigger_reason` is recorded on the ExitFailed fallback.
    trigger_price: f64,
    trigger_time: DateTime<Utc>,
    trigger_reason: String,
    // RAII exit claim — held for the poll's lifetime so the `exiting` slot frees
    // when this task ends OR panics (the caller claimed it before spawning).
    guard: ExitGuard,
) {
    let poll_sem = runtime.paper_poll_sem();
    tokio::spawn(async move {
        // Held until this task ends or unwinds on panic.
        let _guard = guard;
        // Bound concurrent fill-poll tasks; held for the task's lifetime.
        let _permit = poll_sem.acquire_owned().await;
        let rule_id = rule.id;
        let max_total = none_if_zero_u64(rule.p_max_total_tokens);
        let mut found = false;
        // Skip the O(n) exit-fill walk on ticks where no new trade landed for the
        // mint (count is monotonic; `None` forces the first walk).
        let mut last_count: Option<u64> = None;
        // Worst-case fill modelling (#1): once the ladder first fires at slot S, the
        // recorded fill is the lowest price over [S, S + EXIT_SLIPPAGE_SLOTS]. We must
        // NOT record on the first match — slots S+1/S+2 may not be indexed yet — so we
        // keep re-walking as trades arrive (the windowed min only drops) until a trade
        // past the window lands, or the poll window elapses, then record the lowest fill.
        let mut fired: Option<(super::super::exit::ExitFill, u64)> = None;
        let mut max_slot_seen: u64 = 0;
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(super::PAPER_EXIT_POLL_WINDOW_SECS) {
            // Confirm the fill from the in-memory cache window (kept current by the
            // WS pipeline) instead of an unbounded `find_by_mint_all` DB scan per
            // tick. A `None` (token untracked) skips this tick like an empty fetch.
            if let Some((trades, trade_count)) =
                cache_trades(&token_cache, &mint).filter(|(_, c)| last_count != Some(*c))
            {
                last_count = Some(trade_count);
                if let Some(s) = trades.iter().map(|t| t.slot).max() {
                    max_slot_seen = max_slot_seen.max(s);
                }
                // The entry block time is the one persisted with `entry_price` at
                // entry recording (`entry_time_db`). The cache row no longer carries a
                // signature (Phase B step 1), so there's nothing to match it against;
                // the persisted entry time is the same value the old sig lookup
                // recovered. `Utc::now()` only as a last-resort if it's somehow unset.
                let entry_block_time = entry_time_db.unwrap_or_else(chrono::Utc::now);
                // Re-walk over the latest window; the windowed worst-case fill only
                // drops as the slippage-window slots land, so always take the freshest.
                if let Some(windowed) = super::super::exit::find_trade_driven_exit_with_slot(
                    &trades,
                    entry_block_time,
                    entry_price,
                    &rule,
                ) {
                    fired = Some(windowed);
                }
                // The slippage window is fully indexed once a trade past it lands.
                if let Some((_, fire_slot)) = fired {
                    if max_slot_seen > fire_slot + EXIT_SLIPPAGE_SLOTS {
                        break;
                    }
                }
            }
            sleep(Duration::from_millis(super::PAPER_EXIT_POLL_INTERVAL_MS)).await;
        }
        if let Some((fill, _)) = fired {
            // Recover the real tx_signature from the DB (cache stripped it).
            let exit_tx = trade_repo::find_tx_by_fill(&pool, &mint, fill.block_time, fill.price)
                .await
                .unwrap_or_default()
                .unwrap_or_default();
            if let Ok(Some(prev)) = paper_repo.find_by_id(position_id).await {
                // `update_exit` returns the updated row (RETURNING), so we sync
                // runtime state directly without a read-back. Full-bag exit: the exit
                // token amount is the entry token count.
                if let Ok(current) = paper_repo
                    .update_exit(
                        position_id,
                        &exit_tx,
                        fill.price,
                        fill.block_time,
                        fill.reason.as_str(),
                        prev.entry_token_amount.unwrap_or(0.0),
                    )
                    .await
                {
                    runtime.sync_position(Some(&prev), &current);
                }
            }
            info!(
                "[PAPER] Set exit for position {}: {} (tx: {}, reason: {})",
                position_id, fill.price, exit_tx, fill.reason
            );
            found = true;
        }
        if !found {
            // No confirming exit trade was indexed within the poll window. A paper
            // exit that can't fill is booked as a worst-case TOTAL LOSS (#3): record
            // exit_price = 0 (⇒ −100% PnL) rather than the hypothetical trigger price,
            // so paper stays a worst-case-faithful proxy. Status stays terminal
            // ExitFailed with the trigger reason; never reverts to Holding.
            if let Ok(Some(prev)) = paper_repo.find_by_id(position_id).await {
                let _ = paper_repo
                    .mark_exit_failed(position_id, 0.0, trigger_time, &trigger_reason)
                    .await;
                if let Ok(Some(current)) = paper_repo.find_by_id(position_id).await {
                    runtime.sync_position(Some(&prev), &current);
                }
            }
            info!(
                "[PAPER] No exit fill indexed; marked position {} ExitFailed as total loss \
                 (exit_price=0; hypothetical trigger was {})",
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
        // `_guard` drops here, releasing the shared exit claim (the caller claimed
        // it before spawning to bound this to one task per position).
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
    rule: &Tpsl2Rule,
) {
    // Time exits have no confirming trade, so the lookup returns "" (graceful).
    let exit_tx = trade_repo::find_tx_by_fill(pool, &position.mint, exit_time, exit_price)
        .await
        .unwrap_or_default()
        .unwrap_or_default();
    let prev = position.clone();
    // Full-bag exit: the exit token amount is the entry token count.
    let exit_token_amount = position.entry_token_amount.unwrap_or(0.0);
    if let Err(err) = paper_repo
        .update_exit(position.id, &exit_tx, exit_price, exit_time, &reason, exit_token_amount)
        .await
    {
        warn!(position_id = %position.id, "Failed to record paper time exit: {err}");
        return;
    }
    // Reflect the close in the snapshot synced to the runtime cache.
    position.close(exit_price, vec![exit_tx], exit_token_amount, exit_time);
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
