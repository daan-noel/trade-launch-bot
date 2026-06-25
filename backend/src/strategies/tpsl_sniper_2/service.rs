use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tokio::sync::{broadcast, watch};
use tracing::{debug, info, warn};

use crate::config::constants::{resolve_buy_slippage_bps, resolve_sell_slippage_bps};
use pump_trader::constants::LAMPORTS_PER_SOL;
use crate::models::ingest::SseEvent;
use crate::models::Position;
use crate::state::token_cache::TokenCache;
use crate::state::trade_signals::TradeSignals;
use crate::storage::repositories::settings_repo::AppSettings;
use crate::storage::repositories::{
    tpsl2_paper_trading_repo::Tpsl2PaperTradingRepo, tpsl2_position_repo::Tpsl2PositionRepo,
    trade_repo::TradeRepo,
};
use super::{TPSL2StrategyHandler, Tpsl2RuntimeCache};
use crate::trader::PumpFunTrader;
use super::util::none_if_zero_u64;

/// Exit reason stamped on a position re-driven out of an orphaned `ExitPending`
/// state (its original trigger reason wasn't persisted), so the recovered close
/// is distinguishable in the history.
const EXIT_PENDING_RECOVERY_REASON: &str = "ExitPendingRecovery";

#[derive(Clone)]
pub struct Tpsl2StrategyService {
    pool: PgPool,
    position_repo: Tpsl2PositionRepo,
    paper_repo: Tpsl2PaperTradingRepo,
    trade_repo: TradeRepo,
    trader: Arc<PumpFunTrader>,
    runtime: Arc<Tpsl2RuntimeCache>,
    /// WS-fed price/trade source of truth; cloned into spawned real-sell tasks.
    token_cache: Arc<TokenCache>,
    /// Cold-lane broadcast for client notifications (e.g. paper-run finished).
    sse_tx: broadcast::Sender<SseEvent>,
    /// Persisted-trade wakeup hub for the snipe buy confirm loop.
    trade_signals: Arc<TradeSignals>,
    /// Live view of the persisted settings document; read for the effective
    /// trade slippage (1B) at each real buy/sell without a DB round-trip.
    settings: watch::Receiver<AppSettings>,
}

impl Tpsl2StrategyService {
    pub fn new(
        pool: PgPool,
        trader: Arc<PumpFunTrader>,
        runtime: Arc<Tpsl2RuntimeCache>,
        token_cache: Arc<TokenCache>,
        sse_tx: broadcast::Sender<SseEvent>,
        trade_signals: Arc<TradeSignals>,
        settings: watch::Receiver<AppSettings>,
    ) -> Self {
        Self {
            position_repo: Tpsl2PositionRepo::new(pool.clone()),
            paper_repo: Tpsl2PaperTradingRepo::new(pool.clone()),
            trade_repo: TradeRepo::new(pool.clone()),
            pool,
            trader,
            runtime,
            token_cache,
            sse_tx,
            trade_signals,
            settings,
        }
    }

    fn buy_slippage(&self) -> Option<u64> {
        let s = self.settings.borrow();
        resolve_buy_slippage_bps(s.buy_slippage_bps, s.slippage_bps, None)
    }

    fn sell_slippage(&self) -> Option<u64> {
        resolve_sell_slippage_bps(self.settings.borrow().sell_slippage_bps, None)
    }

    const EXIT_PENDING_CLEANUP_INTERVAL_MS: u64 = 60_000;
    const EXIT_PENDING_STALE_MS: u64 = 300_000;

    /// Age past which a `BuySubmitted` position the recovery reaper still can't
    /// resolve (no fill indexed, no confirmed revert) is flagged for manual review.
    /// It is **never** auto-deleted — a durable-nonce buy may have landed but not
    /// indexed, so the tokens could be real. Comfortably exceeds the buy window.
    const BUY_SUBMITTED_REVIEW_MS: u64 = 600_000;

    /// Age past which an unentered (0-entry) Holding paper position is reaped as an
    /// orphan. Must comfortably exceed the largest real arming window
    /// (`SCALP_ARMING_BASE_SECS` + the rule's time gates, at most a few minutes) so
    /// the reaper only ever catches rows whose in-memory arming task is gone — never
    /// races a poll that is still legitimately working.
    const UNENTERED_STALE_MS: u64 = 600_000;
}

impl Tpsl2StrategyService {
    pub fn spawn_background_tasks(&self) {
        let cleanup_repo = self.position_repo.clone();
        let paper_repo = self.paper_repo.clone();
        let runtime = self.runtime.clone();
        let pool = self.pool.clone();
        // The reaper re-drives orphaned ExitPending sells (panic / crash recovery)
        // before the stale-fail pass below would terminally fail them. It needs the
        // full service (trader, caches, repos), so clone it into the task.
        let service = self.clone();
        tokio::spawn(async move {
            let stale = Duration::from_millis(Tpsl2StrategyService::EXIT_PENDING_STALE_MS);
            let unentered_stale =
                Duration::from_millis(Tpsl2StrategyService::UNENTERED_STALE_MS);
            let mut interval = tokio::time::interval(Duration::from_millis(
                Tpsl2StrategyService::EXIT_PENDING_CLEANUP_INTERVAL_MS,
            ));
            loop {
                // First tick is immediate, so this also performs the boot-time
                // ExitPending recovery (1A-2) for the process-crash+restart case;
                // subsequent ticks are the in-process reaper (1A-4) that recovers a
                // single panicked sell task without a restart. Re-drive runs BEFORE
                // the stale-fail pass so a recoverable bag is re-attempted, not
                // failed; the atomic exit-guard claim prevents double-driving an
                // exit that's still legitimately in flight.
                interval.tick().await;
                service.redrive_orphaned_exit_pending().await;
                // Buy-side recovery: adopt/wait/drop positions stranded in
                // `BuySubmitted` (crash/restart in the send→record gap, or a
                // panicked buy task). Runs BEFORE the stale/unentered passes; it
                // never deletes a row that might own tokens, so an unresolvable one
                // is just left + flagged, not reaped.
                service.redrive_orphaned_buy_submitted().await;
                let mut failed_total: u64 = 0;
                let mut changed = false;
                match cleanup_repo.fail_stale_exit_pending(stale).await {
                    Ok(n) => failed_total += n,
                    Err(err) => warn!("Failed to clean stale ExitPending positions: {err}"),
                }
                match paper_repo.fail_stale_exit_pending(stale).await {
                    Ok(n) => failed_total += n,
                    Err(err) => warn!("Failed to clean stale paper ExitPending positions: {err}"),
                }
                if failed_total > 0 {
                    info!("Marked {failed_total} stale (orphaned) ExitPending positions ExitFailed");
                    changed = true;
                }
                // Reap orphaned unentered positions (real + paper) whose in-memory
                // arming/buy task was lost to a restart/resume (never entered,
                // exited, or deleted otherwise). Without the real pass these
                // 0-entry Holding rows linger forever — gated out of every exit
                // path AND pinning their dead token in `token_cache` (`is_held`).
                let mut reaped_unentered: u64 = 0;
                match cleanup_repo.delete_stale_unentered(unentered_stale).await {
                    Ok(n) => reaped_unentered += n,
                    Err(err) => warn!("Failed to reap orphaned unentered real positions: {err}"),
                }
                match paper_repo.delete_stale_unentered(unentered_stale).await {
                    Ok(n) => reaped_unentered += n,
                    Err(err) => warn!("Failed to reap orphaned unentered paper positions: {err}"),
                }
                if reaped_unentered > 0 {
                    info!("Reaped {reaped_unentered} orphaned unentered positions");
                    changed = true;
                }
                if changed {
                    if let Err(err) = runtime.reload_holding(&pool).await {
                        warn!("Failed to refresh TPSL holding cache after cleanup: {err}");
                    }
                }
            }
        });
    }

    pub async fn on_token_created(&self, mint: &str, cache: &TokenCache) {
        // Probe on the borrowed `&str`; only allocate an owned mint once a rule
        // actually matches (most created tokens match nothing). Mirrors the
        // `on_trade_executed` early-bail discipline.
        let token = match cache.get(mint) {
            Some(entry) => entry.value().token.clone(),
            None => {
                debug!("Token {mint} not in cache — skipping TPSL create");
                return;
            }
        };

        let rules = self.runtime.active_rules();

        if rules.is_empty() {
            debug!("No active TPSL rules — skipping token {mint}");
            return;
        }

        let handler = TPSL2StrategyHandler::new(rules);
        let rule_ids = handler.check_all_buy_entries(&token);
        if rule_ids.is_empty() {
            debug!("Token {mint} does not match any TPSL buy entry rule");
            return;
        }

        for rule_id in rule_ids {
            let Some(rule) = handler.get_rule(rule_id) else {
                continue;
            };
            info!("Token {mint} matches TPSL buy entry rule {rule_id}");

            let is_paper = rule.trade_mode == "paper";
            let max_concurrent_tokens = none_if_zero_u64(rule.p_max_concurrent_tokens).map(|v| v as usize);
            let max_total_tokens =
                none_if_zero_u64(rule.p_max_total_tokens).map(|v| v as usize);

            // Paper rules trade inside a run; ensure one exists before the
            // caps are checked (its counters are scoped to the run). A run is
            // normally started on activation — this lazily starts one for a
            // rule that became active without going through the API path.
            let paper_run_id = if is_paper {
                Some(match self.runtime.current_paper_run(rule_id) {
                    Some(r) => r.run_id,
                    None => match self
                        .runtime
                        .start_paper_run(&self.pool, rule_id, none_if_zero_u64(rule.p_max_total_tokens))
                        .await
                    {
                        Ok(r) => r.id,
                        Err(err) => {
                            warn!("Failed to start paper run for rule {rule_id}: {err}");
                            continue;
                        }
                    },
                })
            } else {
                None
            };

            if let Some(cap) = max_concurrent_tokens {
                let current_holding = self.runtime.holding_count_by_rule(rule_id);
                if current_holding >= cap as i64 {
                    debug!(
                        "Rule {rule_id} reached max concurrent tokens \
                         ({current_holding}/{cap}), skipping {mint}"
                    );
                    continue;
                }
            }

            if let Some(total_max) = max_total_tokens {
                let total_traded = self.runtime.total_count_by_rule(rule_id);
                if total_traded >= total_max as i64 {
                    debug!(
                        "Rule {rule_id} reached max total tokens \
                         ({total_traded}/{total_max}), skipping {mint}"
                    );
                    continue;
                }
            }

            // A match: now it's worth owning the mint (cloned per rule, moved
            // into each spawned insert + buy/fill task below).
            let mint = mint.to_string();
            let mut position = Position::new(
                mint.clone(),
                self.trader.wallet_pubkey(),
                "TPSL2".to_string(),
                rule_id,
            );
            position.token_program_id = token.token_program_id.clone();
            position.target_price = Some(0.0);
            position.target_token_amount = Some(0.0);
            position.target_tx = Some(String::new());
            position.target_time = Some(token.created_at);

            let is_real = rule.trade_mode == "real";
            let buy_amount = rule.buy_amount;
            let position_id = position.id;

            // SOL balance-floor guard for real buys: checked BEFORE sync_position
            // so no runtime cleanup is needed when the guard fires.
            if is_real {
                let buy_lamports = (buy_amount * LAMPORTS_PER_SOL as f64) as u64;
                if !self.trader.can_commit_buy(buy_lamports) {
                    warn!("[REAL] SOL balance-floor guard blocked snipe of {buy_amount:.4} SOL \
                           for {mint} rule {rule_id}");
                    continue;
                }
                if let Some(max_sol) = self.settings.borrow().max_committed_sol {
                    let max_lamports = (max_sol * LAMPORTS_PER_SOL as f64) as u64;
                    if self.trader.committed_lamports().saturating_add(buy_lamports) > max_lamports {
                        warn!("[REAL] max_committed_sol guard blocked snipe of {buy_amount:.4} SOL \
                               for {mint} rule {rule_id}");
                        continue;
                    }
                }
                self.trader.commit_sol_for_position(position_id.to_string(), buy_lamports);
            }

            // Claim the cap slot + holding-index entry INLINE on the runner's
            // select task, then spawn the slow DB insert + buy/fill off it — the
            // same discipline `trigger_real_exit` uses for the sell side. The
            // count bump is done inline (not in the spawn) so the next ping in a
            // launch wave sees this token against the cap *before* its insert's DB
            // RTT completes; otherwise a matched entry would head-of-line-block the
            // next ping (possibly a TP/SL exit) for a round-trip. A 0-entry position
            // is gated out of every exit path (`clock_entry_time` => None), so it
            // can sit in the holding index until its entry fill lands without being
            // mis-exited.
            self.runtime.sync_position(None, &position);

            // Resolve slippage before the spawn (the watch borrow isn't held
            // across an await); reserves are read inside the task at buy time.
            let slippage_bps = self.buy_slippage();
            let creator = token.creator_wallet.clone();
            let token_program_id = position
                .token_program_id
                .clone()
                .unwrap_or_else(|| crate::config::constants::TOKEN_PROGRAM_ID.to_string());
            let rule_owned = rule.clone();
            let paper_repo = self.paper_repo.clone();
            let position_repo = self.position_repo.clone();
            let trade_repo = self.trade_repo.clone();
            let runtime = self.runtime.clone();
            let token_cache = self.token_cache.clone();
            let trader = self.trader.clone();
            let trade_signals = self.trade_signals.clone();
            tokio::spawn(async move {
                let insert_res = match paper_run_id {
                    Some(run_id) => paper_repo.insert(&position, run_id).await,
                    None => position_repo.insert(&position).await,
                };
                if let Err(err) = insert_res {
                    warn!("Failed to create position for token {mint}: {err}");
                    // Roll back the inline cap/holding-index claim and SOL commitment.
                    if is_real {
                        trader.release_sol_for_position(&position_id.to_string());
                    }
                    runtime.remove_position(&position);
                    return;
                }
                info!("Created position {position_id} for token {mint} under rule {rule_id}");

                if is_real {
                    // Claim the entry slot for the whole real branch (arming +
                    // buy) — the buy-side twin of the sell's ExitGuard. While held,
                    // the buy-recovery reaper skips this position; the RAII guard
                    // frees the slot when this task ends OR panics.
                    let _entry_guard = runtime.try_begin_entry(position_id);
                    // Arm on the scalp entry signal before sending any buy: real
                    // mode waits for the first trade where every configured gate
                    // holds (shared `find_scalp_entry`), so live entries honor
                    // `p_entry_*` exactly like paper and simulation. The candidate
                    // is only the timing trigger — the fill price comes from the
                    // wallet's own on-chain buy. No signal in the window → fall
                    // through and drop the unentered position, as a missed buy does.
                    let armed = super::execution::real::await_scalp_entry_signal(
                        &mint,
                        &rule_owned,
                        position_id,
                        &token_cache,
                        &trade_signals,
                        &runtime,
                        super::execution::real::ScalpWaitCfg::for_rule(&rule_owned),
                    )
                    .await;
                    if let Some(target) = armed {
                        // Persist the trigger trade as the target point BEFORE
                        // sending the buy — the real fill (entry_*) lands later
                        // and independently, so the two can be compared. Sync the
                        // returned row into the runtime cache so the snapshot
                        // carries target_*. The just-inserted in-memory `position`
                        // is the current DB state (nothing has mutated it since
                        // the insert), so use it as `prev` instead of a read-back.
                        //
                        // `target.tx_signature` is empty: the trigger was resolved
                        // off the sig-free live cache (Phase B step 1). target_* is
                        // a display/diagnostic snapshot, not a decision input;
                        // re-fetching the trigger's real sig would add a DB round
                        // trip to the entry path, which the latency budget forbids.
                        match position_repo
                            .update_target(
                                position_id,
                                target.price,
                                target.amount_tokens,
                                target.block_time,
                                &target.tx_signature,
                            )
                            .await
                        {
                            Ok(current) => runtime.sync_position(Some(&position), &current),
                            Err(err) => warn!(
                                "[REAL] Failed to record target for position {}: {err}",
                                position_id
                            ),
                        }
                        // Derive the slippage min_out from the curve's in-memory
                        // virtual reserves at the armed moment (the trigger trade
                        // just landed, so the snapshot is fresh); `None` ⇒ min_out=1,
                        // never an inline RPC on the snipe path.
                        let reserves = token_cache.get(&mint).and_then(|e| {
                            let st = e.value();
                            super::execution::real::snipe_reserves_from_cache(
                                st.current_virtual_token_reserves,
                                st.current_virtual_sol_reserves,
                            )
                        });
                        super::execution::real::buy_until_filled_or_give_up(
                            trader.clone(),
                            mint.clone(),
                            creator,
                            token_program_id,
                            buy_amount,
                            position_id,
                            position_repo.clone(),
                            trade_repo.clone(),
                            runtime.clone(),
                            trade_signals,
                            super::execution::real::BuyRetryCfg::production(),
                            slippage_bps,
                            reserves,
                        )
                        .await;
                    } else {
                        info!(
                            "[REAL] Scalp entry signal never fired for mint {} within the \
                             arming window; dropping unentered position {}",
                            mint, position_id
                        );
                    }
                    if let Ok(Some(pos)) = position_repo.find_by_id(position_id).await {
                        if pos.entry_price.is_none() {
                            // No fill — release the SOL commitment and clean up.
                            // Positions that DID fill stay Holding; their commitment
                            // is released in sell_and_close_position.
                            trader.release_sol_for_position(&position_id.to_string());
                            let _ = position_repo.delete_position(position_id).await;
                            runtime.remove_position(&pos);
                            info!(
                                "[REAL] Removed position {} for mint {}: no entry recorded",
                                position_id, mint
                            );
                        }
                    }
                } else if is_paper {
                    super::execution::paper::spawn_entry_fill_poll(
                        paper_repo,
                        runtime,
                        token_cache,
                        mint,
                        position_id,
                        rule_owned,
                    );
                }
            });
        }
    }

    pub async fn on_trade_executed(&self, mint: &str, cache: &TokenCache) {
        // Cheap holding-index lookup first: the vast majority of trade pings are
        // for mints we hold no position in, so probe the index on the borrowed
        // `&str` and bail before allocating an owned mint (or touching the
        // potentially large trade history below).
        let positions = self.runtime.holding_by_mint(mint);
        if positions.is_empty() {
            return;
        }
        let mint = mint.to_string();

        // Evaluate every position's exit decision while holding the cache read
        // guard. The exit ladder (fixed TP/SL + E1–E4) and the clock-state fold are
        // synchronous, so we walk the in-memory trade history in place — no
        // per-event deep clone of up to `MAX_TRADES_RETAINED` trades. We collect
        // the positions that must exit, then drop the guard before any await on the
        // (slow) paper/real exit path. `current_price` is the reference price for
        // the resulting fill.
        let current_price;
        let mut to_exit = Vec::new();
        // Proactive manual-sell detection: if the last trade in the feed is a Sell
        // from the bot's own wallet, capture its price/time so we can confirm and
        // close any real Holding position that wasn't triggered by the exit ladder.
        // The DB balance check (and guard claim) happen AFTER the guard is dropped.
        let mut manual_sell_info: Option<(f64, DateTime<Utc>)> = None;
        {
            let Some(entry) = cache.get(&mint) else {
                return;
            };
            let state = entry.value();
            // Keep the Option: a missing price falls back per-position to the
            // position's own entry_price (below), so a failed exit records a 0%
            // move instead of a bogus −100% (matches the `sweep_time_exits` path).
            current_price = state.current_price;
            let trades = &state.trades;
            let trades_base = state.trades_base;

            // Scan the ring buffer backwards for the most recent sell from the bot
            // wallet. Checking only `trades.last()` is fragile: the strategy ping
            // channel is async, so further trades can be appended to the ring buffer
            // between the manual-sell ping being dispatched and the runner consuming
            // it, pushing the sell off `.last()`. Scanning backwards costs O(k) where
            // k ≤ MAX_TRADES_RETAINED but stops at the first hit.
            // `try_close_manually_sold` guards against stale-sell false positives via
            // a DB balance check (positive balance → early return), so finding an old
            // sell from a prior already-closed position is safe.
            let bot_wallet = self.trader.wallet_pubkey();
            if let Some(bot_id) = state.interner.id_of(&bot_wallet) {
                if let Some(sell) = trades.iter().rev().find(|t| !t.is_buy && t.wallet == bot_id) {
                    manual_sell_info = Some((sell.price_per_token, sell.block_time));
                }
            }

            for position in &positions {
                // Hot-path gate: pull only the exit-ladder scalars, not a full
                // rule clone (that happens below, once per *actual* exit).
                let Some(params) = self.runtime.ladder_params_by_id(position.rule_id) else {
                    continue;
                };
                // Holding + recorded entry only; a 0-entry/non-Holding position is
                // neither evaluated nor folded (matches the old gate guards).
                let Some(entry_time) = super::exit::clock_entry_time(position) else {
                    continue;
                };
                // Incremental trade gate: fold the newly-printed trades into the
                // position's memoized walk state + E5 cohort net AND evaluate the
                // exit ladder against only those new trades in one pass — no
                // per-ping full re-walk (H3) or cohort rebuild (H4). The memo
                // doubles as the clock-sweep's current snapshot.
                if let Some(exit_reason) = self.runtime.exit_state_advance_and_find_exit(
                    position.id,
                    position.entry_price.unwrap_or(0.0),
                    entry_time,
                    trades,
                    trades_base,
                    &params,
                ) {
                    to_exit.push((position.clone(), exit_reason));
                }
            }
        }

        // Collect IDs that the ladder already claimed BEFORE moving `to_exit`
        // into the loop — so the manual-sell pass can skip them without re-borrowing.
        let exiting_ids: HashSet<uuid::Uuid> = to_exit.iter().map(|(p, _)| p.id).collect();

        for (position, exit_reason) in to_exit {
            // This position is actually exiting (rare) — now it's worth the full
            // rule clone the execution paths need.
            let Some(rule) = self.runtime.rule_by_id(position.rule_id) else {
                continue;
            };
            // Deep-clone only now that this position is actually exiting; the
            // holding index hands us a shared `Arc<Position>`.
            let mut position = (*position).clone();
            // Reference price for the fill; fall back to this position's entry
            // price when the cache has no current price (a 0% move, not −100%).
            let exit_price = current_price.or(position.entry_price).unwrap_or(0.0);
            debug!(
                "Position {} for token {mint} triggered exit: {:?}",
                position.id, exit_reason
            );

            if rule.trade_mode == "paper" {
                // Claim the exit; if one is already in flight for this position
                // skip (don't spawn a second fill-poll). The RAII guard frees the
                // slot when the poll task ends OR panics.
                let Some(guard) = self.runtime.try_begin_exit(position.id) else {
                    continue;
                };
                let prev = position.clone();
                position.mark_exit_pending();
                if let Err(err) = self.paper_repo.update(&position).await {
                    warn!(
                        "Failed to mark position {} as ExitPending: {err}",
                        position.id
                    );
                    // The write failed, so the position is still Holding. Dropping
                    // `guard` here releases the claim and skips the spawn —
                    // otherwise the next ping/sweep re-fires the ladder and spawns
                    // another poll, an unbounded task storm under a dump that
                    // saturates `paper_poll_sem`.
                    continue;
                }
                self.runtime.sync_position(Some(&prev), &position);
                info!(position_id = %position.id, mint = %mint,
                    "[PAPER] Position marked ExitPending");
                super::execution::paper::spawn_exit_fill_poll(
                    self.paper_repo.clone(),
                    self.runtime.clone(),
                    self.token_cache.clone(),
                    self.pool.clone(),
                    self.sse_tx.clone(),
                    mint.clone(),
                    position.id,
                    position.entry_price.unwrap_or(0.0),
                    position.entry_time,
                    rule.clone(),
                    exit_price,
                    Utc::now(),
                    exit_reason.to_string(),
                    guard,
                );
            } else if rule.trade_mode == "real" {
                self.trigger_real_exit(
                    position,
                    exit_price,
                    Utc::now(),
                    exit_reason.to_string(),
                )
                .await;
            }
        }

        // Proactive manual-sell close: for real Holding positions that the ladder
        // did NOT exit, verify the DB balance and close directly (no sell tx) if the
        // position was externally cleared. The guard claim inside
        // `try_close_manually_sold` acts as the double-exit interlock: a position
        // already claimed by a ladder exit above returns `None` and is skipped.
        if let Some((sell_price, sell_time)) = manual_sell_info {
            for position in &positions {
                if exiting_ids.contains(&position.id) {
                    continue;
                }
                let Some(rule) = self.runtime.rule_by_id(position.rule_id) else {
                    continue;
                };
                if rule.trade_mode != "real" {
                    continue;
                }
                // Gate: only positions with a recorded entry (mirrors the ladder gate).
                if super::exit::clock_entry_time(position).is_none() {
                    continue;
                }
                self.try_close_manually_sold(position.clone(), mint.clone(), sell_price, sell_time)
                    .await;
            }
        }
    }

    /// Wall-clock exit sweep. For every Holding position, evaluate the
    /// **time-based** exits (E2 TimeStop / E3 Stall) against `now` via
    /// [`exit::should_position_exit_on_clock`], so they fire even when a token
    /// has gone silent and no trade ping is arriving — the gap the trade-driven
    /// `on_trade_executed` leaves open. Price-based exits stay on the trade path
    /// (price can't change between trades), so the two paths never overlap.
    ///
    /// Runs on the strategy runner's timer in the **same task** as
    /// `on_trade_executed`, so the two can never race on a position's
    /// Holding→ExitPending transition without any DB-level locking.
    pub async fn sweep_time_exits(&self, cache: &TokenCache) {
        let now = Utc::now();

        // Iterate only the holdings whose rule carries a time exit (maintained as
        // a secondary index), not every open position. The per-rule short-circuit
        // below is kept as a cheap defensive check against index/rule skew.
        for position in self.runtime.time_exit_holding_positions() {
            // Hot-path gate: only the time-exit scalars, not a full rule clone
            // (deferred to the rare branch where a position actually exits).
            let Some(params) = self.runtime.ladder_params_by_id(position.rule_id) else {
                continue;
            };
            if params.time_stop_secs().is_none() && params.stall_secs().is_none() {
                continue;
            }
            // Skip until the entry fill is indexed (entry_price/entry_time set);
            // the clock gate would reject it anyway, and we avoid seeding state
            // for a not-yet-open position.
            let Some(entry_time) = super::exit::clock_entry_time(&position) else {
                continue;
            };

            // The walk state comes from the memoized cache — already kept current
            // by the trade path — so the sweep never re-walks the trade history.
            // `last_price` (the paper fill mark) is a cheap `Copy` read; only a
            // not-yet-seeded position needs the trade vec, and just once to seed.
            let (state, last_price) = match cache.get(&position.mint) {
                Some(entry) => {
                    let st = entry.value();
                    let last_price = st.current_price.or(position.entry_price).unwrap_or(0.0);
                    let state = self.runtime.exit_state_get(position.id).unwrap_or_else(|| {
                        self.runtime.exit_state_build(
                            position.id,
                            position.entry_price.unwrap_or(0.0),
                            entry_time,
                            &st.trades,
                            st.trades_base,
                        )
                    });
                    (state, last_price)
                }
                // Token absent from the cache (e.g. evicted) can still TimeStop
                // from entry_time alone — seed from an empty history if needed.
                None => {
                    let state = self.runtime.exit_state_get(position.id).unwrap_or_else(|| {
                        self.runtime
                            .exit_state_build(position.id, position.entry_price.unwrap_or(0.0), entry_time, &[], 0)
                    });
                    (state, position.entry_price.unwrap_or(0.0))
                }
            };

            let Some(exit_reason) =
                super::exit::should_position_exit_on_clock(&position, &state, &params, now)
            else {
                continue;
            };

            info!(
                position_id = %position.id, mint = %position.mint,
                "Time-driven exit triggered: {exit_reason}"
            );

            // This position is exiting (rare) — now clone the full rule the
            // execution paths need, and deep-clone out of the shared `Arc`.
            let Some(rule) = self.runtime.rule_by_id(position.rule_id) else {
                continue;
            };
            let position = (*position).clone();
            if rule.trade_mode == "paper" {
                super::execution::paper::record_time_exit(
                    &self.paper_repo,
                    &self.runtime,
                    &self.pool,
                    &self.sse_tx,
                    position,
                    last_price,
                    now,
                    exit_reason.to_string(),
                    &rule,
                )
                .await;
            } else if rule.trade_mode == "real" {
                self.trigger_real_exit(position, last_price, now, exit_reason.to_string())
                    .await;
            }
        }
    }

    /// Mark a real position ExitPending inline, then **spawn** the slow sell pass
    /// off the runner's `select!` task. Shared by the trade-driven exit
    /// (`on_trade_executed`) and the time-driven sweep.
    ///
    /// The ExitPending mark + holding-index sync happen inline (synchronously
    /// within the select task) so the position leaves the holding index before
    /// the next ping/sweep. The actual sell — which can await tens of seconds
    /// (`SELL_MAX_ATTEMPTS × SELL_POLL_MAX_ATTEMPTS × 1s`) — is spawned so it
    /// never stalls ping handling or the 1s time sweep. The `selling` in-flight
    /// set guards the no-double-sell invariant across that spawn boundary even if
    /// the ExitPending DB write fails (which would leave the position in the
    /// holding index).
    ///
    /// `sell_with_retries` (inside `sell_and_close_position`) owns the retry +
    /// partial-fill loop and re-reads migration routing from the WS cache on every
    /// attempt, so a mid-exit migration self-heals to the AMM path. It then closes
    /// the position (on a confirmed sell) or marks it ExitFailed.
    /// `trigger_price`/`trigger_time` are the hypothetical exit recorded if the
    /// sell never confirms.
    async fn trigger_real_exit(
        &self,
        mut position: Position,
        trigger_price: f64,
        trigger_time: DateTime<Utc>,
        reason: String,
    ) {
        // Claim the position via the shared exit guard; bail if a sell is already
        // in flight for it (from the ladder OR the manual Stop&Close lifecycle).
        let Some(guard) = self.runtime.try_begin_exit(position.id) else {
            return;
        };

        let prev = position.clone();
        position.mark_exit_pending();
        if let Err(err) = self.position_repo.update(&position).await {
            warn!(
                "Failed to mark position {} as ExitPending: {err}",
                position.id
            );
        } else {
            self.runtime.sync_position(Some(&prev), &position);
            info!(position_id = %position.id, mint = %position.mint,
                "[REAL] Position marked ExitPending");
        }

        self.spawn_real_sell(position, trigger_price, trigger_time, reason, guard);
    }

    /// Spawn the slow sell pass off the runner's task, holding the RAII exit
    /// `guard` for its lifetime (the guard frees the `exiting` slot when the task
    /// ends OR panics — so a panic can no longer wedge the slot). Shared by the
    /// ladder/time exit (`trigger_real_exit`, which marks ExitPending first) and
    /// the ExitPending reaper (`redrive_orphaned_exit_pending`, where the position
    /// is already ExitPending). `trigger_price`/`trigger_time` are the hypothetical
    /// exit recorded if the sell never confirms.
    fn spawn_real_sell(
        &self,
        position: Position,
        trigger_price: f64,
        trigger_time: DateTime<Utc>,
        reason: String,
        guard: super::ExitGuard,
    ) {
        let trader = self.trader.clone();
        let position_repo = self.position_repo.clone();
        let trade_repo = self.trade_repo.clone();
        let runtime = self.runtime.clone();
        let token_cache = self.token_cache.clone();
        let trade_signals = self.trade_signals.clone();
        // Resolve slippage before the spawn (watch borrow not held across await).
        let slippage_bps = self.sell_slippage();
        tokio::spawn(async move {
            // Held until the sell completes or the task unwinds on panic.
            let _guard = guard;
            super::execution::real::sell_and_close_position(
                trader,
                position,
                position_repo,
                trade_repo,
                runtime,
                &token_cache,
                trade_signals,
                trigger_price,
                trigger_time,
                reason,
                slippage_bps,
            )
            .await;
        });
    }

    /// Close a real `Holding` position that was externally (manually) sold out,
    /// without sending any new on-chain sell transaction.
    ///
    /// Verifies via the DB that the net token balance is ≤ `PARTIAL_FILL_THRESHOLD`
    /// before committing — a cheap indexed query that guards against false positives
    /// from a same-wallet sell on a different concurrent position. On confirmation,
    /// claims the exit guard (the double-exit interlock: returns `None` if a sell is
    /// already in flight), marks `ExitPending`, then spawns the direct close.
    async fn try_close_manually_sold(
        &self,
        position: std::sync::Arc<crate::models::Position>,
        mint: String,
        sell_price: f64,
        sell_time: DateTime<Utc>,
    ) {
        let wallet = self.trader.wallet_pubkey();
        match self
            .trade_repo
            .net_token_amount_by_wallet_and_mint(&wallet, &mint)
            .await
        {
            Ok(balance) if balance <= super::execution::PARTIAL_FILL_THRESHOLD as f64 => {}
            Ok(_) => return, // bag still positive — not fully cleared yet
            Err(err) => {
                warn!(position_id = %position.id, mint = %mint,
                    "manual-sell check: balance query failed, skipping: {err}");
                return;
            }
        }
        let Some(guard) = self.runtime.try_begin_exit(position.id) else { return; };
        let mut position = (*position).clone();
        let prev = position.clone();
        position.mark_exit_pending();
        if let Err(err) = self.position_repo.update(&position).await {
            warn!(position_id = %position.id, mint = %mint,
                "Failed to mark position ExitPending (ManualSell): {err}");
        } else {
            self.runtime.sync_position(Some(&prev), &position);
            info!(position_id = %position.id, mint = %mint,
                "[REAL] Position marked ExitPending (ManualSell)");
        }
        let position_repo = self.position_repo.clone();
        let runtime = self.runtime.clone();
        let trader = self.trader.clone();
        tokio::spawn(async move {
            let _guard = guard;
            super::execution::real::close_externally_cleared_position(
                &mut position,
                &position_repo,
                &runtime,
                &trader,
                sell_price,
                sell_time,
                "ManualSell",
            )
            .await;
        });
    }

    /// Re-drive real positions stranded in `ExitPending` whose exit guard is **not**
    /// currently held — i.e. their sell task panicked (no process restart) or the
    /// process crashed mid-exit and restarted (the holding cache only loads
    /// `Holding`, so these are invisible to the trade/time ladder and nothing else
    /// would ever re-attempt their sell). Runs at boot (the reaper's first,
    /// immediate tick) and on the maintenance cadence thereafter.
    ///
    /// The atomic `try_begin_exit` claim **is** the double-sell interlock: if a
    /// legitimate sell is still in flight its guard is held, the claim returns
    /// `None`, and we skip; only a position with no live exit is re-driven. A
    /// position that never recorded an entry fill can't be sold, so it is left for
    /// the stale-ExitPending reaper to terminally fail.
    async fn redrive_orphaned_exit_pending(&self) {
        let pending = match self.position_repo.find_all_exit_pending().await {
            Ok(p) => p,
            Err(err) => {
                warn!("Failed to load ExitPending positions for recovery: {err}");
                return;
            }
        };
        for position in pending {
            if position.entry_price.is_none() {
                continue;
            }
            // Atomic claim doubles as the in-flight check — skip if a sell is live.
            let Some(guard) = self.runtime.try_begin_exit(position.id) else {
                continue;
            };
            let trigger_price = self
                .token_cache
                .get(&position.mint)
                .and_then(|e| e.value().current_price)
                .or(position.entry_price)
                .unwrap_or(0.0);
            let reason = position
                .exit_reason
                .clone()
                .unwrap_or_else(|| EXIT_PENDING_RECOVERY_REASON.to_string());
            info!(position_id = %position.id, mint = %position.mint,
                "[REAL] Re-driving orphaned ExitPending sell");
            self.spawn_real_sell(position, trigger_price, Utc::now(), reason, guard);
        }
    }

    /// Recover real positions stranded in `BuySubmitted` — a buy was signed/sent
    /// (so tokens may exist on-chain) but its fill was never recorded, because the
    /// process crashed/restarted in the send→record gap or the buy task panicked.
    /// The holding cache reloads them, but nothing on the live path re-drives a buy
    /// whose task is gone. Runs at boot (the reaper's first, immediate tick) and on
    /// the maintenance cadence thereafter, BEFORE the stale/unentered passes.
    ///
    /// **Never re-sends and never blindly deletes** — a durable-nonce buy can still
    /// land after reboot, so re-firing would double-buy (the buy path's core
    /// invariant). For each row, by its persisted `submitted_buy_signatures`:
    /// - **Adopt** — a sig is already in the trade feed → record the entry →
    ///   `Holding` (reuses `adopt_existing_fill_if_present`, per-signature attributed).
    /// - **Drop** — *every* sig is a confirmed on-chain revert → bought nothing →
    ///   delete the unentered row (safe, no tokens).
    /// - **Wait** — any sig landed-but-unindexed / pending / unknown → leave it
    ///   `BuySubmitted` for the next tick; flag for manual review past a max age.
    ///
    /// The atomic `try_begin_entry` claim **is** the interlock: a live buy task
    /// holds the entry guard → the claim returns `None` → skip (the buy is
    /// genuinely in flight). After a crash the `entering` set is empty, so every
    /// reloaded row is claimable and recovered.
    async fn redrive_orphaned_buy_submitted(&self) {
        let submitted = match self.position_repo.find_all_buy_submitted().await {
            Ok(p) => p,
            Err(err) => {
                warn!("Failed to load BuySubmitted positions for recovery: {err}");
                return;
            }
        };
        let wallet = self.trader.wallet_pubkey();
        for position in submitted {
            // Atomic claim doubles as the in-flight check — skip if a buy is live.
            let Some(_guard) = self.runtime.try_begin_entry(position.id) else {
                continue;
            };

            // 1. Adopt: a submitted sig is already indexed → record the entry.
            if super::execution::real::adopt_existing_fill_if_present(
                &position.mint,
                &wallet,
                position.id,
                &self.position_repo,
                &self.trade_repo,
                &self.runtime,
                &position.submitted_buy_signatures,
            )
            .await
            {
                info!(position_id = %position.id, mint = %position.mint,
                    "[REAL] Recovered BuySubmitted position: adopted on-chain fill");
                continue;
            }

            // 2. No fill found. A row with no persisted signature can't be checked
            //    (shouldn't happen — `mark_buy_submitted` always appends) — wait,
            //    never delete (tokens might exist).
            if position.submitted_buy_signatures.is_empty() {
                warn!(position_id = %position.id, mint = %position.mint,
                    "BuySubmitted position has no submitted signatures — leaving for review");
                continue;
            }

            // 3. Classify each submitted signature on-chain. Drop ONLY if EVERY one
            //    is a confirmed revert (bought nothing); otherwise wait — a single
            //    landed/pending/unknown sig means tokens may exist or the tx may
            //    still land, so deleting would orphan real tokens.
            let mut all_reverted = true;
            for sig in &position.submitted_buy_signatures {
                let status = self.trader.signature_state(sig).await;
                if super::execution::real::classify_submitted_buy(&status)
                    == super::execution::real::BuyRecoveryVerdict::Wait
                {
                    all_reverted = false;
                    break;
                }
            }

            if all_reverted {
                match self.position_repo.delete_position(position.id).await {
                    Ok(()) => {
                        self.runtime.remove_position(&position);
                        info!(position_id = %position.id, mint = %position.mint,
                            "[REAL] Dropped reverted BuySubmitted position (every buy reverted; no tokens)");
                    }
                    Err(err) => warn!(position_id = %position.id,
                        "Failed to drop reverted BuySubmitted position: {err}"),
                }
            } else {
                // Tokens may exist or a durable-nonce tx may still land — wait. Flag
                // for manual review once stuck well past any plausible window.
                let age = Utc::now().signed_duration_since(position.updated_at);
                if age > chrono::Duration::milliseconds(Self::BUY_SUBMITTED_REVIEW_MS as i64) {
                    warn!(position_id = %position.id, mint = %position.mint,
                        "BuySubmitted position unresolved past the review window — needs manual \
                         review (a buy may have landed but never indexed); NOT auto-deleting");
                }
            }
            // `_guard` drops here, freeing the entry slot for the next tick.
        }
    }
}
