//! Paper execution — mirror the real buy/sell lifecycle against the WS/DB trade
//! feed without sending any transaction, strategy-agnostic over the unified
//! [`StrategyPosition`] / [`StrategyRepo`] / [`StrategyRuntimeCache`].
//!
//! Entry and trade-driven exit each poll the feed for the confirming trade
//! (spawned tasks); a clock-driven exit on a silent token records immediately at
//! the last observed price. The fill *resolvers* differ per strategy and are
//! dispatched: the **entry** loop forks (tpsl1's fixed-count poll vs tpsl2's
//! scalp-watch + until-dead armer); the **exit** loop is uniform and resolves the
//! fill via [`StrategyImpl::resolve_paper_exit`].

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::sync::broadcast;
use tokio::time::sleep;
use tracing::{info, warn};
use uuid::Uuid;

use trading_core::config::constants::MAX_FILL_WAIT_SLOTS;
use trading_core::models::ingest::SseEvent;
use trading_core::models::{StrategyPosition, StrategyRule};
use trading_core::storage::repositories::strategy_repo::StrategyRepo;
use trading_core::storage::repositories::trade_repo;
use trading_core::strategies::registry::{StrategyImpl, StrategyParams, Swing1Params, Tpsl2Params};
use trading_core::strategies::runtime_cache::{ExitGuard, StrategyRuntimeCache};
use trading_core::strategies::swing_1::entry as sw_entry;
use trading_core::strategies::tpsl_sniper_1::entry as t1_entry;
use trading_core::strategies::tpsl_sniper_2::entry as t2_entry;

use super::scalp::scalp_watch_window;
use super::swing::swing_watch_window;
use crate::state::token_cache::{CachedTrade, TokenCache};

/// Snapshot the mint's retained trade window from the in-memory token cache
/// (oldest-first), or `None` if the token isn't tracked. The WS pipeline keeps this
/// current for every trade on the mint (any wallet), so the paper fill polls read
/// it instead of an unbounded `find_by_mint_all` DB scan per tick. Returns the
/// shared `Arc` (a refcount bump under the shard guard, not a deep copy) plus the
/// token's lifetime `trade_count`, so a poll can skip its O(n) fill walk on an idle
/// tick where no new trade landed (the count is monotonic; the resolvers watch ALL
/// wallets on the mint, so there's no per-(wallet,mint) lane to wake on instead).
fn cache_trades(token_cache: &TokenCache, mint: &str) -> Option<(Arc<Vec<CachedTrade>>, u64)> {
    token_cache
        .get(mint)
        .map(|e| (e.value().trades.clone(), e.value().trade_count))
}

/// The cap a paper run finishes at, in `u64` (0 / unset ⇒ no cap ⇒ never auto-finishes).
fn run_cap(rule: &StrategyRule) -> Option<u64> {
    rule.max_total_tokens.filter(|&c| c > 0).map(|c| c as u64)
}

/// A resolved paper entry, ready to persist. `target` (the trigger-trade snapshot)
/// is tpsl2-only; tpsl1 has no target↔entry gap.
struct PaperEntry {
    /// `(price, amount_tokens, block_time, tx)` of the trigger trade (tpsl2 only).
    target: Option<(f64, f64, DateTime<Utc>, String)>,
    price: f64,
    token_amount: f64,
    tx: String,
    time: DateTime<Utc>,
}

/// Poll the WS-fed trade feed for the token's opening trades and record the paper
/// entry on first sight — exactly as real mode polls for its on-chain fill. The
/// entry price/tx/time come only from a real indexed trade; paper mode never
/// synthesizes a fill from a create-time snapshot. If no fill is indexed within the
/// window, the unentered position is dropped.
///
/// The entry *resolution* forks by strategy; the persist/cleanup tail is shared.
pub(crate) fn spawn_entry_fill_poll(
    repo: StrategyRepo,
    runtime: Arc<StrategyRuntimeCache>,
    token_cache: Arc<TokenCache>,
    mint: String,
    position_id: Uuid,
    rule: StrategyRule,
    params: StrategyParams,
) {
    let poll_sem = runtime.paper_poll_sem();
    tokio::spawn(async move {
        // Bound concurrent fill-poll tasks; held for the task's lifetime.
        let _permit = poll_sem.acquire_owned().await;
        let buy_amount_sol = rule.buy_amount_sol;

        let resolved = match (StrategyImpl::from_id(&rule.strategy_id), &params) {
            (Some(StrategyImpl::Tpsl1), _) => {
                resolve_paper_entry_tpsl1(&repo, &token_cache, &mint, buy_amount_sol).await
            }
            (Some(StrategyImpl::Tpsl2), StrategyParams::Tpsl2(p)) => {
                resolve_paper_entry_tpsl2(
                    &repo,
                    &runtime,
                    &token_cache,
                    &mint,
                    position_id,
                    buy_amount_sol,
                    p,
                )
                .await
            }
            (Some(StrategyImpl::Swing1), StrategyParams::Swing1(p)) => {
                resolve_paper_entry_swing1(
                    &repo,
                    &runtime,
                    &token_cache,
                    &mint,
                    position_id,
                    buy_amount_sol,
                    p,
                )
                .await
            }
            _ => None,
        };

        match resolved {
            Some(e) => {
                if let Ok(Some(mut pos)) = repo.find_position(position_id).await {
                    let prev = pos.clone();
                    if let Some((tp, ta, tt, ttx)) = e.target {
                        // Position token amounts are raw integer units; paper computes
                        // a fractional estimate, so round to whole raw units here.
                        pos.set_target(tp, ta.round() as u64, tt, ttx);
                    }
                    // SOL is display-derived (`price × amount_tokens`); store it so the
                    // unified row carries a consistent entry cost.
                    let sol = e.price * e.token_amount;
                    pos.set_entry(e.price, e.token_amount.round() as u64, sol, e.time, vec![e.tx.clone()]);
                    if repo.update_position(&pos).await.is_ok() {
                        runtime.sync_position(Some(&prev), &pos);
                    }
                    info!(
                        "[PAPER] Set entry for position {}: {} (tx: {})",
                        position_id, e.price, e.tx
                    );
                }
            }
            None => {
                // No real fill was indexed within the poll window. Mirror real mode's
                // "buy not found" cleanup: drop the unentered position rather than
                // leave a 0-entry row that can never trade.
                if let Ok(Some(pos)) = repo.find_position(position_id).await {
                    if pos.entry_price.is_none() {
                        let _ = repo.delete_position(position_id).await;
                        runtime.remove_position(&pos);
                        info!(
                            "[PAPER] Removed position {} for mint {}: no entry fill indexed",
                            position_id, mint
                        );
                    }
                }
            }
        }
    });
}

/// tpsl1 paper entry: a fixed-count poll for the first indexed opening fill (cap 5),
/// recording the token count `buy_amount_sol / entry_price` (SOL is display-derived,
/// same convention as tpsl2/swing1). No target, no scalp window.
async fn resolve_paper_entry_tpsl1(
    repo: &StrategyRepo,
    token_cache: &TokenCache,
    mint: &str,
    buy_amount_sol: f64,
) -> Option<PaperEntry> {
    let mut last_count: Option<u64> = None;
    for _ in 0..super::BUY_POLL_MAX_ATTEMPTS {
        sleep(Duration::from_millis(super::BUY_POLL_INTERVAL_MS)).await;
        let Some((trades, trade_count)) = cache_trades(token_cache, mint) else {
            continue;
        };
        if last_count == Some(trade_count) {
            continue;
        }
        last_count = Some(trade_count);
        if let Some(fill) = t1_entry::find_entry_fill_in_trades(&trades, 5) {
            // Recover the real tx_signature from the DB (the cache strips it); the
            // fill is a real on-chain trade ingested into `trades`. "" if not found.
            let entry_tx = trade_repo::find_tx_by_fill(repo.pool(), mint, fill.block_time, fill.price)
                .await
                .unwrap_or_default()
                .unwrap_or_default();
            // Paper entry size is the token count `buy_amount_sol / entry_price` (SOL is
            // display-derived); guard a 0 price so we never divide by ~0.
            let token_amount = if fill.price > 0.0 { buy_amount_sol / fill.price } else { 0.0 };
            return Some(PaperEntry {
                target: None,
                price: fill.price,
                token_amount,
                tx: entry_tx,
                time: fill.block_time,
            });
        }
    }
    None
}

/// tpsl2 paper entry: watch the live feed for the scalp signal (same window as real
/// mode — a `p_entry_max_age_secs` ceiling is self-limiting, else an until-dead
/// armer slot bounds it), resolve the trigger by **index**, then the worst-case
/// adverse fill in the trigger's block (and the next). Records both the target
/// (trigger trade) and the worst-case entry, so paper has a real target↔entry gap.
async fn resolve_paper_entry_tpsl2(
    repo: &StrategyRepo,
    runtime: &Arc<StrategyRuntimeCache>,
    token_cache: &TokenCache,
    mint: &str,
    position_id: Uuid,
    buy_amount_sol: f64,
    params: &Tpsl2Params,
) -> Option<PaperEntry> {
    let rule = params.to_rule();
    // An until-dead watch takes a bounded armer slot so a never-dying token can't pin
    // this `paper_poll_sem` permit forever (the slot frees when this resolver ends).
    let window = scalp_watch_window(params);
    let _armer_guard = match window {
        Some(_) => None,
        None => Some(runtime.begin_until_dead_armer(position_id)),
    };
    let deadline = window.map(|max| std::time::Instant::now() + max);
    let mut last_count: Option<u64> = None;
    loop {
        sleep(Duration::from_millis(super::SCALP_ENTRY_WAIT_INTERVAL_MS)).await;
        // Stop conditions mirror the real watch: max-age deadline, eviction by the
        // until-dead armer cap, or token death.
        if let Some(dl) = deadline {
            if std::time::Instant::now() >= dl {
                return None;
            }
        }
        if let Some(g) = &_armer_guard {
            if g.is_cancelled() {
                return None;
            }
        }
        let dead = token_cache
            .get(mint)
            .map(|e| e.value().is_dead(Utc::now()))
            .unwrap_or(false);
        let Some((trades, trade_count)) = cache_trades(token_cache, mint) else {
            if dead {
                return None;
            }
            continue;
        };
        if last_count != Some(trade_count) {
            last_count = Some(trade_count);
            // Resolve the trigger by index: the cache row carries no signature, so the
            // worst-case entry keys off the trigger's position in `trades`. The indexed
            // resolver gives the same trigger the sig-keyed path did.
            if let Some((trigger_idx, target_fill)) =
                t2_entry::find_scalp_entry_indexed(&trades, &rule)
            {
                // No real buy in [trigger+1, trigger+MAX_FILL_WAIT_SLOTS] yet — keep
                // polling until the fill window indexes or the deadline expires.
                let Some(entry) = t2_entry::find_worst_case_paper_entry_at(&trades, trigger_idx)
                else {
                    if dead {
                        return None;
                    }
                    continue;
                };
                let target_tx =
                    trade_repo::find_tx_by_fill(repo.pool(), mint, target_fill.block_time, target_fill.price)
                        .await
                        .unwrap_or_default()
                        .unwrap_or_default();
                let entry_tx = trade_repo::find_tx_by_fill(repo.pool(), mint, entry.block_time, entry.price)
                    .await
                    .unwrap_or_default()
                    .unwrap_or_default();
                // Paper entry size is the token count `buy_amount_sol / entry_price` (SOL is
                // display-derived); guard a 0 price so we never divide by ~0.
                let token_amount = if entry.price > 0.0 { buy_amount_sol / entry.price } else { 0.0 };
                return Some(PaperEntry {
                    target: Some((
                        target_fill.price,
                        target_fill.amount_tokens,
                        target_fill.block_time,
                        target_tx,
                    )),
                    price: entry.price,
                    token_amount,
                    tx: entry_tx,
                    time: entry.block_time,
                });
            }
        }
        if dead {
            return None;
        }
    }
}

/// swing1 paper entry: watch the live feed for the kill→volume latch + higher-low
/// confirmation (same causal decision as backtest — `find_phase_entry`), bounded the
/// same way as tpsl2 (a `p_entry_max_age_secs` ceiling is self-limiting; else an
/// until-dead armer slot). Unlike tpsl2 there is **no target↔entry gap**: swing1's
/// `find_phase_entry` already returns the worst-case, canonical-spot-priced fill at
/// the trigger, so we record that directly with `target: None`.
async fn resolve_paper_entry_swing1(
    repo: &StrategyRepo,
    runtime: &Arc<StrategyRuntimeCache>,
    token_cache: &TokenCache,
    mint: &str,
    position_id: Uuid,
    buy_amount_sol: f64,
    params: &Swing1Params,
) -> Option<PaperEntry> {
    let rule = params.to_rule();
    // A set entry-window ceiling self-limits the watch; without one, take a bounded
    // armer slot so a never-dying token can't pin the `paper_poll_sem` permit forever.
    let window = swing_watch_window(params);
    let _armer_guard = match window {
        Some(_) => None,
        None => Some(runtime.begin_until_dead_armer(position_id)),
    };
    let deadline = window.map(|max| std::time::Instant::now() + max);
    let mut last_count: Option<u64> = None;
    loop {
        sleep(Duration::from_millis(super::SCALP_ENTRY_WAIT_INTERVAL_MS)).await;
        // Stop conditions mirror the real watch: max-age deadline, eviction by the
        // until-dead armer cap, or token death.
        if let Some(dl) = deadline {
            if std::time::Instant::now() >= dl {
                return None;
            }
        }
        if let Some(g) = &_armer_guard {
            if g.is_cancelled() {
                return None;
            }
        }
        let dead = token_cache
            .get(mint)
            .map(|e| e.value().is_dead(Utc::now()))
            .unwrap_or(false);
        let Some((trades, trade_count)) = cache_trades(token_cache, mint) else {
            if dead {
                return None;
            }
            continue;
        };
        if last_count != Some(trade_count) {
            last_count = Some(trade_count);
            // The full causal resolver: swing-leg scan → kill→volume latch → first
            // confirmed higher-low → worst-case spot fill at the trigger. Byte-identical
            // to the backtest path (both run over `trades` oldest-first).
            if let Some((_trigger_idx, fill)) = sw_entry::find_phase_entry(&trades, &rule) {
                // Recover the real tx_signature from the DB (the cache strips it); the
                // fill is a real on-chain trade ingested into `trades`. "" if not found.
                let entry_tx =
                    trade_repo::find_tx_by_fill(repo.pool(), mint, fill.block_time, fill.price)
                        .await
                        .unwrap_or_default()
                        .unwrap_or_default();
                // Paper entry size is the token count `buy_amount_sol / entry_price` (SOL is
                // display-derived); guard a 0 price so we never divide by ~0.
                let token_amount = if fill.price > 0.0 { buy_amount_sol / fill.price } else { 0.0 };
                return Some(PaperEntry {
                    target: None,
                    price: fill.price,
                    token_amount,
                    tx: entry_tx,
                    time: fill.block_time,
                });
            }
        }
        if dead {
            return None;
        }
    }
}

/// Poll the WS-fed trade feed for the confirming exit trade and record the paper
/// exit via the same exit ladder that triggered it. If none is indexed within the
/// window, mark the position ExitFailed (terminal — no revert to Holding). Either
/// terminal outcome may complete the run.
///
/// The fill resolver is dispatched via [`StrategyImpl::resolve_paper_exit`]: tpsl1
/// records on first find; tpsl2 keeps the freshest slot-windowed worst-case fill
/// until the fill window indexes. The timeout fallback price also differs — tpsl1
/// books the hypothetical trigger price, tpsl2 books a total loss (`exit_price=0`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_exit_fill_poll(
    repo: StrategyRepo,
    runtime: Arc<StrategyRuntimeCache>,
    token_cache: Arc<TokenCache>,
    sse_tx: broadcast::Sender<SseEvent>,
    mint: String,
    position_id: Uuid,
    entry_price: f64,
    entry_time_db: Option<DateTime<Utc>>,
    rule: StrategyRule,
    params: StrategyParams,
    trigger_price: f64,
    trigger_time: DateTime<Utc>,
    trigger_reason: String,
    // RAII exit claim — held for the poll's lifetime so the `exiting` slot frees
    // when this task ends OR panics (the caller claimed it before spawning).
    guard: ExitGuard,
) {
    let poll_sem = runtime.paper_poll_sem();
    tokio::spawn(async move {
        let _guard = guard;
        let _permit = poll_sem.acquire_owned().await;
        let strategy = StrategyImpl::from_id(&rule.strategy_id);
        let rule_id = rule.id;
        let max_total = run_cap(&rule);
        let is_tpsl2 = strategy == Some(StrategyImpl::Tpsl2);

        // Worst-case fill modelling (tpsl2): once the ladder first fires at slot S, the
        // fill is the lowest price in {S, next_slot} where next_slot ≤ S + MAX_FILL_WAIT_SLOTS.
        // Keep re-walking as trades arrive until a trade past the window lands (the min
        // can only drop as more slots index), then record the lowest fill. tpsl1 returns
        // a `None` fire slot and records on the first find.
        let mut fired: Option<(f64, DateTime<Utc>, &'static str)> = None;
        let mut max_slot_seen: u64 = 0;
        let mut last_count: Option<u64> = None;
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(super::PAPER_EXIT_POLL_WINDOW_SECS) {
            if let Some((trades, trade_count)) =
                cache_trades(&token_cache, &mint).filter(|(_, c)| last_count != Some(*c))
            {
                last_count = Some(trade_count);
                if let Some(s) = trades.iter().map(|t| t.slot).max() {
                    max_slot_seen = max_slot_seen.max(s);
                }
                // The entry block time is the one persisted with `entry_price` at entry
                // recording; the cache row no longer carries a signature to match.
                let entry_block_time = entry_time_db.unwrap_or_else(Utc::now);
                if let Some(strat) = strategy {
                    if let Some((exit, fire_slot)) =
                        strat.resolve_paper_exit(&trades, entry_block_time, entry_price, &params)
                    {
                        // Always take the freshest (the windowed worst-case can only drop).
                        fired = Some((exit.price, exit.block_time, exit.reason));
                        match fire_slot {
                            // tpsl1: the resolver already returns the final fill.
                            None => break,
                            // tpsl2: the window is fully indexed once a trade past it lands.
                            Some(fs) => {
                                if max_slot_seen > fs + MAX_FILL_WAIT_SLOTS {
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            sleep(Duration::from_millis(super::PAPER_EXIT_POLL_INTERVAL_MS)).await;
        }

        if let Some((price, block_time, reason)) = fired {
            let exit_tx = trade_repo::find_tx_by_fill(repo.pool(), &mint, block_time, price)
                .await
                .unwrap_or_default()
                .unwrap_or_default();
            if let Ok(Some(mut pos)) = repo.find_position(position_id).await {
                let prev = pos.clone();
                // Full-bag exit: the exit token amount is the entry token count.
                let amt = pos.entry_token_amount.unwrap_or(0);
                pos.close(price, price * amt as f64, amt, vec![exit_tx.clone()], block_time, reason);
                if repo.update_position(&pos).await.is_ok() {
                    runtime.sync_position(Some(&prev), &pos);
                }
            }
            info!(
                "[PAPER] Set exit for position {}: {} (tx: {}, reason: {})",
                position_id, price, exit_tx, reason
            );
        } else {
            // No confirming exit trade was indexed within the poll window. tpsl2 books a
            // worst-case TOTAL LOSS (`exit_price=0` ⇒ −100% PnL) so paper stays a
            // worst-case-faithful proxy; tpsl1 books the hypothetical trigger price.
            // Status is terminal ExitFailed either way; never reverts to Holding.
            let fail_price = if is_tpsl2 { 0.0 } else { trigger_price };
            if let Ok(Some(mut pos)) = repo.find_position(position_id).await {
                let prev = pos.clone();
                pos.mark_exit_failed(fail_price, trigger_time);
                pos.exit_reason = Some(trigger_reason.clone());
                if repo.update_position(&pos).await.is_ok() {
                    runtime.sync_position(Some(&prev), &pos);
                }
            }
            info!(
                "[PAPER] No exit fill indexed; marked position {} ExitFailed at {} \
                 (hypothetical trigger was {})",
                position_id, fail_price, trigger_price
            );
        }

        // A terminal outcome (clean exit or failed exit) may have been the last open
        // position — the run can now complete.
        if let Ok(Some(pos)) = repo.find_position(position_id).await {
            if pos.is_closed() {
                finish_paper_run_if_complete(
                    &repo, &runtime, &sse_tx, rule_id, &rule.rule_name, max_total,
                )
                .await;
            }
        }
        // `_guard` drops here, releasing the shared exit claim.
    });
}

/// Record a paper clock-driven exit immediately at the last observed price. Unlike
/// the trade-driven resolver this does **not** poll for a confirming trade — a
/// time/stall exit on a silent token has none by definition. The last seen price is
/// the honest mark. Transitions Holding→End in one write and, if the run's cap is
/// met with nothing left open, finishes the run.
pub(crate) async fn record_time_exit(
    repo: &StrategyRepo,
    runtime: &Arc<StrategyRuntimeCache>,
    sse_tx: &broadcast::Sender<SseEvent>,
    mut position: StrategyPosition,
    exit_price: f64,
    exit_time: DateTime<Utc>,
    reason: String,
    rule: &StrategyRule,
) {
    // Time/manual exits are synthetic (exit_time = now, exit_price = last mark), so
    // no on-chain trade ever matches — skip the `trades` lookup entirely. Full-bag
    // exit: the exit token amount is the entry token count.
    let prev = position.clone();
    let amt = position.entry_token_amount.unwrap_or(0);
    position.close(exit_price, exit_price * amt as f64, amt, vec![String::new()], exit_time, &reason);
    if let Err(err) = repo.update_position(&position).await {
        warn!(position_id = %position.id, "Failed to record paper time exit: {err}");
        return;
    }
    runtime.sync_position(Some(&prev), &position);
    info!(
        position_id = %position.id, mint = %position.mint_address, exit_price,
        "[PAPER] Time-driven exit recorded"
    );

    finish_paper_run_if_complete(repo, runtime, sse_tx, rule.id, &rule.rule_name, run_cap(rule))
        .await;
}

/// After a paper position closes, finish the run if its total-token cap has been
/// reached and no positions remain open: auto-deactivate the rule, refresh the
/// rules cache, and broadcast a [`SseEvent::PaperTestFinished`]. No-ops when the
/// rule has no cap (`max_total` is `None`) or the cap/holding conditions aren't met.
pub(crate) async fn finish_paper_run_if_complete(
    repo: &StrategyRepo,
    runtime: &Arc<StrategyRuntimeCache>,
    sse_tx: &broadcast::Sender<SseEvent>,
    rule_id: Uuid,
    rule_name: &str,
    max_total: Option<u64>,
) {
    let Some(cap) = max_total else { return };
    let total = runtime.total_count_by_rule(rule_id);
    let holding = runtime.holding_count_by_rule(rule_id);
    if total < cap as i64 || holding > 0 {
        return;
    }

    match runtime.finish_run(repo, rule_id).await {
        Ok(Some(run)) => {
            // Auto-deactivate the rule so it stops cleanly, then refresh the cache.
            match repo.find_rule(rule_id).await {
                Ok(Some(mut rule)) if rule.is_active => {
                    rule.is_active = false;
                    if let Err(err) = repo.update_rule(&rule).await {
                        warn!("Failed to deactivate finished paper rule {rule_id}: {err}");
                    }
                }
                Ok(_) => {}
                Err(err) => warn!("Failed to load rule {rule_id} for paper finish: {err}"),
            }
            if let Err(err) = runtime.reload_rules(repo).await {
                warn!("Failed to reload rules after paper finish: {err}");
            }
            let _ = sse_tx.send(SseEvent::PaperTestFinished {
                rule_id,
                rule_name: rule_name.to_string(),
                run_seq: run.run_seq,
                tokens_traded: total,
                timestamp: Utc::now(),
            });
            info!(
                %rule_id, run_seq = run.run_seq, tokens = total,
                "[PAPER] run finished — rule auto-deactivated"
            );
        }
        Ok(None) => {} // already finished or stopped
        Err(err) => warn!("Failed to finish paper run for rule {rule_id}: {err}"),
    }
}
