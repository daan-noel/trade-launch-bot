//! Unified, **strategy-agnostic** strategy service — the live event/timer edge that
//! turns token-created / trade-executed pings (and the wall-clock sweep) into
//! position lifecycle over the unified [`StrategyRepo`] / [`StrategyRuntimeCache`],
//! dispatching every per-rule decision through [`StrategyImpl`] keyed by
//! `rule.strategy_id`. Replaces the hand-cloned `Tpsl1StrategyService` /
//! `Tpsl2StrategyService`.
//!
//! The only strategy-specific orchestration is the **entry** path in
//! [`on_token_created`](StrategyService::on_token_created): tpsl1 buys immediately on
//! a token match; tpsl2 first arms on the scalp-entry signal
//! ([`scalp::await_scalp_entry_signal`]) and swing1 first arms on the kill→volume
//! phase-entry signal ([`swing::await_swing_entry_signal`]), both recording the
//! target before buying. Everything else — the trade/time exit ladder (via the core
//! exit-state memo), the real sell + manual-sell close, and the
//! ExitPending/BuySubmitted/unentered reapers — is identical across strategies
//! (including swing1's `NextKill` exit priority) and lives here once.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tokio::sync::{broadcast, watch};
use tracing::{debug, info, warn};
use uuid::Uuid;

use pump_trader::constants::LAMPORTS_PER_SOL;
use trading_core::config::constants::{resolve_buy_slippage_bps, resolve_sell_slippage_bps};
use trading_core::models::ingest::SseEvent;
use trading_core::models::{StrategyPosition, StrategyRule};
use trading_core::storage::repositories::settings_repo::AppSettings;
use trading_core::storage::repositories::strategy_repo::StrategyRepo;
use trading_core::storage::repositories::trade_repo::TradeRepo;
use trading_core::strategies::exit_state::clock_entry_time;
use trading_core::strategies::registry::{StrategyImpl, StrategyParams};
use trading_core::strategies::rules::{self, RuleDraft, RuleError};
use trading_core::strategies::runtime_cache::{ExitGuard, StrategyRuntimeCache};

use crate::state::token_cache::TokenCache;
use crate::state::trade_signals::TradeSignals;
use crate::trader::PumpFunTrader;

use super::execution::{paper, real, scalp, swing};

/// Exit reason stamped on a position re-driven out of an orphaned `ExitPending`
/// state (its original trigger reason wasn't persisted), so the recovered close is
/// distinguishable in the history.
const EXIT_PENDING_RECOVERY_REASON: &str = "ExitPendingRecovery";

/// Exit reason stamped on positions force-closed by Stop & close.
const MANUAL_CLOSE_REASON: &str = "ManualClose";

/// How a rule's run is (re)started when the rule is activated.
#[derive(Clone, Copy, Debug)]
pub enum PaperActivation {
    /// Start a brand-new run, dropping the prior run's positions + counters.
    Fresh,
    /// Resume the rule's prior run, keeping its positions + counters (falls back to
    /// `Fresh` if the rule has no prior run).
    Continue,
}

#[derive(Clone)]
pub struct StrategyService {
    repo: StrategyRepo,
    trade_repo: TradeRepo,
    trader: Arc<PumpFunTrader>,
    runtime: Arc<StrategyRuntimeCache>,
    /// WS-fed price/trade source of truth; cloned into spawned real-sell tasks.
    token_cache: Arc<TokenCache>,
    /// Cold-lane broadcast for client notifications (e.g. paper-run finished).
    sse_tx: broadcast::Sender<SseEvent>,
    /// Persisted-trade wakeup hub for the snipe buy confirm loop.
    trade_signals: Arc<TradeSignals>,
    /// Live view of the persisted settings document; read for the effective trade
    /// slippage at each real buy/sell without a DB round-trip.
    settings: watch::Receiver<AppSettings>,
}

impl StrategyService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        pool: PgPool,
        trader: Arc<PumpFunTrader>,
        runtime: Arc<StrategyRuntimeCache>,
        token_cache: Arc<TokenCache>,
        sse_tx: broadcast::Sender<SseEvent>,
        trade_signals: Arc<TradeSignals>,
        settings: watch::Receiver<AppSettings>,
    ) -> Self {
        Self {
            repo: StrategyRepo::new(pool.clone()),
            trade_repo: TradeRepo::new(pool),
            trader,
            runtime,
            token_cache,
            sse_tx,
            trade_signals,
            settings,
        }
    }

    /// The shared runtime cache (for the main wiring + handlers).
    pub fn runtime(&self) -> &Arc<StrategyRuntimeCache> {
        &self.runtime
    }

    /// The shared repo (for the rule-CRUD edge + handlers).
    pub fn repo(&self) -> &StrategyRepo {
        &self.repo
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
    /// **Never** auto-deleted — a durable-nonce buy may have landed but not indexed,
    /// so the tokens could be real. Comfortably exceeds the buy window.
    const BUY_SUBMITTED_REVIEW_MS: u64 = 600_000;

    /// Age past which an unentered (`Arming`, 0-entry) position is reaped as an
    /// orphan. Must comfortably exceed the largest real arming window so the reaper
    /// only ever catches rows whose in-memory arming task is gone — never races a
    /// poll that is still legitimately working.
    const UNENTERED_STALE_MS: u64 = 600_000;
}

impl StrategyService {
    pub fn spawn_background_tasks(&self) {
        let service = self.clone();
        tokio::spawn(async move {
            let stale = Duration::from_millis(StrategyService::EXIT_PENDING_STALE_MS);
            let unentered_stale = Duration::from_millis(StrategyService::UNENTERED_STALE_MS);
            let mut interval = tokio::time::interval(Duration::from_millis(
                StrategyService::EXIT_PENDING_CLEANUP_INTERVAL_MS,
            ));
            loop {
                // First tick is immediate, so this also performs boot-time recovery
                // for the process-crash+restart case; subsequent ticks are the
                // in-process reaper that recovers a single panicked task without a
                // restart. Re-drive runs BEFORE the stale-fail pass so a recoverable
                // bag is re-attempted, not failed; the atomic guard claims prevent
                // double-driving an exit/buy still legitimately in flight.
                interval.tick().await;
                service.redrive_orphaned_exit_pending().await;
                service.redrive_orphaned_buy_submitted().await;
                service.reconcile_externally_cleared_holdings().await;

                let mut changed = false;
                let mut failed_total: u64 = 0;
                for mode in ["real", "paper"] {
                    match service.repo.fail_stale_exit_pending(mode, stale).await {
                        Ok(n) => failed_total += n,
                        Err(err) => warn!("Failed to clean stale {mode} ExitPending positions: {err}"),
                    }
                }
                if failed_total > 0 {
                    info!("Marked {failed_total} stale (orphaned) ExitPending positions ExitFailed");
                    changed = true;
                }

                // Reap orphaned unentered positions (real + paper) whose in-memory
                // arming/buy task was lost to a restart/resume. Without this these
                // 0-entry rows linger forever — gated out of every exit path AND
                // pinning their dead token in `token_cache` (`is_mint_held`).
                let mut reaped_unentered: u64 = 0;
                for mode in ["real", "paper"] {
                    match service.repo.delete_stale_unentered(mode, unentered_stale).await {
                        Ok(n) => reaped_unentered += n,
                        Err(err) => warn!("Failed to reap orphaned unentered {mode} positions: {err}"),
                    }
                }
                if reaped_unentered > 0 {
                    info!("Reaped {reaped_unentered} orphaned unentered positions");
                    changed = true;
                }

                if changed {
                    if let Err(err) = service.runtime.reload_holding(&service.repo).await {
                        warn!("Failed to refresh strategy holding cache after cleanup: {err}");
                    }
                }
            }
        });
    }

    pub async fn on_token_created(&self, mint: &str, cache: &TokenCache) {
        // Probe on the borrowed `&str`; only allocate an owned mint once a rule
        // actually matches (most created tokens match nothing).
        let token = match cache.get(mint) {
            Some(entry) => entry.value().token.clone(),
            None => {
                debug!("Token {mint} not in cache — skipping strategy create");
                return;
            }
        };

        let rules = self.runtime.active_rules();
        if rules.is_empty() {
            debug!("No active strategy rules — skipping token {mint}");
            return;
        }

        // Token-creation entry gate, dispatched per rule by its strategy. Collect the
        // matched (rule, params, strategy) tuples (cheap clones) before any await.
        let mut matched = Vec::new();
        for rule in rules.iter() {
            let Some(strat) = StrategyImpl::from_id(&rule.strategy_id) else {
                continue;
            };
            let Some(params) = self.runtime.params_by_id(rule.id) else {
                continue;
            };
            if strat.matches_entry(&token, &params) {
                matched.push((rule.clone(), params, strat));
            }
        }
        if matched.is_empty() {
            debug!("Token {mint} does not match any strategy buy entry rule");
            return;
        }

        for (rule, params, strat) in matched {
            let rule_id = rule.id;
            info!("Token {mint} matches strategy buy entry rule {rule_id}");

            let is_real = rule.trade_mode == "real";
            let max_concurrent = rule.max_concurrent_tokens.filter(|&c| c > 0);
            let max_total = rule.max_total_tokens.filter(|&c| c > 0);

            // Every position trades inside a run (both modes). Ensure one exists
            // before the caps are checked (counters are scoped to the current run). A
            // run is normally started on activation — this lazily starts one for a
            // rule that became active outside the API path.
            let run_id = match self.runtime.current_run(rule_id) {
                Some(r) => r.run_id,
                None => match self.runtime.start_run(&self.repo, &rule).await {
                    Ok(run) => run.id,
                    Err(err) => {
                        warn!("Failed to start run for rule {rule_id}: {err}");
                        continue;
                    }
                },
            };

            if let Some(cap) = max_concurrent {
                let current_holding = self.runtime.holding_count_by_rule(rule_id);
                if current_holding >= cap {
                    debug!("Rule {rule_id} at max concurrent ({current_holding}/{cap}), skipping {mint}");
                    continue;
                }
            }
            if let Some(total_max) = max_total {
                let total_traded = self.runtime.total_count_by_rule(rule_id);
                if total_traded >= total_max {
                    debug!("Rule {rule_id} at max total ({total_traded}/{total_max}), skipping {mint}");
                    continue;
                }
            }

            // A match: now it's worth owning the mint (moved into the spawned task).
            let mint = mint.to_string();
            let mut position = StrategyPosition::new(
                run_id,
                rule.strategy_id.clone(),
                rule_id,
                rule.trade_mode.clone(),
                mint.clone(),
                self.trader.wallet_pubkey(),
            );
            position.token_program_id = token.token_program_id.clone();
            // Inert target defaults (tpsl2 real overwrites with the trigger snapshot).
            position.set_target(0.0, 0, token.created_at, String::new());

            let buy_amount_sol = rule.buy_amount_sol;
            let position_id = position.id;

            // SOL balance-floor guard for real buys: checked BEFORE sync_position so
            // no runtime cleanup is needed when the guard fires.
            if is_real {
                let buy_lamports = (buy_amount_sol * LAMPORTS_PER_SOL as f64) as u64;
                if !self.trader.can_commit_buy(buy_lamports) {
                    warn!("[REAL] SOL balance-floor guard blocked snipe of {buy_amount_sol:.4} SOL \
                           for {mint} rule {rule_id}");
                    continue;
                }
                if let Some(max_sol) = self.settings.borrow().max_committed_sol {
                    let max_lamports = (max_sol * LAMPORTS_PER_SOL as f64) as u64;
                    if self.trader.committed_lamports().saturating_add(buy_lamports) > max_lamports {
                        warn!("[REAL] max_committed_sol guard blocked snipe of {buy_amount_sol:.4} SOL \
                               for {mint} rule {rule_id}");
                        continue;
                    }
                }
                self.trader.commit_sol_for_position(position_id.to_string(), buy_lamports);
            }

            // Claim the cap slot + holding-index entry INLINE on the runner's select
            // task, then spawn the slow DB insert + buy/fill off it. The count bump is
            // inline so the next ping in a launch wave sees this token against the cap
            // before the insert's DB RTT completes. A 0-entry position is gated out of
            // every exit path (`clock_entry_time` => None), so it can sit in the
            // holding index until its entry fill lands without being mis-exited.
            self.runtime.sync_position(None, &position);

            // Resolve slippage before the spawn (the watch borrow isn't held across an
            // await); reserves are read inside the task at buy time.
            let slippage_bps = self.buy_slippage();
            let cashback_enabled = token.is_cashback_enabled;
            let creator = token.creator_wallet.clone();
            let token_program_id = position
                .token_program_id
                .clone()
                .unwrap_or_else(|| trading_core::config::constants::TOKEN_PROGRAM_ID.to_string());
            let repo = self.repo.clone();
            let trade_repo = self.trade_repo.clone();
            let runtime = self.runtime.clone();
            let token_cache = self.token_cache.clone();
            let trader = self.trader.clone();
            let trade_signals = self.trade_signals.clone();
            tokio::spawn(async move {
                if let Err(err) = repo.insert_position(&position).await {
                    warn!("Failed to create position for token {mint}: {err}");
                    // Roll back the inline cap/holding-index claim + SOL commitment.
                    if is_real {
                        trader.release_sol_for_position(&position_id.to_string());
                    }
                    runtime.remove_position(&position);
                    return;
                }
                info!("Created position {position_id} for token {mint} under rule {rule_id}");

                if is_real {
                    // Claim the entry slot for the buy's lifetime — the buy-side twin
                    // of the sell's ExitGuard. The RAII guard frees the slot when this
                    // task ends OR panics.
                    let _entry_guard = runtime.try_begin_entry(position_id);

                    // Entry orchestration: tpsl2 arms on the scalp signal first,
                    // swing1 arms on the kill→volume phase-entry signal, both
                    // recording the target before buying; tpsl1 buys immediately.
                    // `proceed` is false only when an arming window closed without
                    // a signal.
                    let proceed = match (strat, &params) {
                        (StrategyImpl::Tpsl2, StrategyParams::Tpsl2(p)) => {
                            match scalp::await_scalp_entry_signal(
                                &mint,
                                p,
                                position_id,
                                &token_cache,
                                &trade_signals,
                                &runtime,
                                scalp::ScalpWaitCfg::for_params(p),
                            )
                            .await
                            {
                                Some(target) => {
                                    // Persist the trigger as the target BEFORE the buy
                                    // — the real fill (entry_*) lands later and
                                    // independently, so the two can be compared. The
                                    // just-inserted in-memory `position` is the current
                                    // DB state, so use it as `prev` (no read-back).
                                    let prev = position.clone();
                                    position.set_target(
                                        target.price,
                                        // amount_tokens is raw units as f64 (TradeRow
                                        // accessor); store the exact integer count.
                                        target.amount_tokens.round() as u64,
                                        target.block_time,
                                        target.tx_signature,
                                    );
                                    if let Err(err) = repo.update_position(&position).await {
                                        warn!("[REAL] Failed to record target for position {position_id}: {err}");
                                    } else {
                                        runtime.sync_position(Some(&prev), &position);
                                    }
                                    true
                                }
                                None => {
                                    info!(
                                        "[REAL] Scalp entry signal never fired for mint {mint} within \
                                         the arming window; dropping unentered position {position_id}"
                                    );
                                    false
                                }
                            }
                        }
                        (StrategyImpl::Swing1, StrategyParams::Swing1(p)) => {
                            match swing::await_swing_entry_signal(
                                &mint,
                                p,
                                position_id,
                                &token_cache,
                                &trade_signals,
                                &runtime,
                                swing::SwingWaitCfg::for_params(p),
                            )
                            .await
                            {
                                Some(target) => {
                                    // Same target-before-buy persistence as tpsl2 —
                                    // `find_phase_entry` is already worst-case-spot-
                                    // priced at the trigger, so this doubles as the
                                    // recorded entry target for comparison against
                                    // the real fill.
                                    let prev = position.clone();
                                    position.set_target(
                                        target.price,
                                        target.amount_tokens.round() as u64,
                                        target.block_time,
                                        target.tx_signature,
                                    );
                                    if let Err(err) = repo.update_position(&position).await {
                                        warn!("[REAL] Failed to record target for position {position_id}: {err}");
                                    } else {
                                        runtime.sync_position(Some(&prev), &position);
                                    }
                                    true
                                }
                                None => {
                                    info!(
                                        "[REAL] Swing1 phase-entry signal never fired for mint {mint} \
                                         within the arming window; dropping unentered position {position_id}"
                                    );
                                    false
                                }
                            }
                        }
                        // tpsl1 (and any mismatch, defensively) buys immediately.
                        _ => true,
                    };

                    if proceed {
                        // Re-quote the slippage min_out from the curve's in-memory
                        // virtual reserves on EVERY attempt (a resend with a stale
                        // floor would just re-revert); `None` ⇒ min_out=1, never an
                        // inline RPC. The closure captures a cache clone so the buy
                        // loop can re-read fresh reserves per retry.
                        let reserves_cache = token_cache.clone();
                        let reserves_mint = mint.clone();
                        let reserves_fn = move || {
                            reserves_cache.get(&reserves_mint).and_then(|e| {
                                let st = e.value();
                                real::snipe_reserves_from_cache(
                                    st.current_reserve_token,
                                    st.current_reserve_sol,
                                )
                            })
                        };
                        real::buy_until_filled_or_give_up(
                            trader.clone(),
                            mint.clone(),
                            creator,
                            token_program_id,
                            buy_amount_sol,
                            position_id,
                            repo.clone(),
                            trade_repo.clone(),
                            runtime.clone(),
                            trade_signals,
                            real::BuyRetryCfg::production(),
                            slippage_bps,
                            reserves_fn,
                            cashback_enabled,
                        )
                        .await;
                        // Do NOT inline-delete unentered positions after a buy attempt.
                        // A buy that lands on-chain but isn't yet indexed returns
                        // unentered from `buy_until_filled_or_give_up`, and deleting
                        // it would orphan real tokens with no position tracking them.
                        // The periodic `redrive_orphaned_buy_submitted` reaper is the
                        // safe owner: it adopts indexed fills and only drops when
                        // EVERY submitted sig is a confirmed revert. SOL is released
                        // there on confirmed revert.
                    } else {
                        // Arming window closed with no signal — no buy was ever sent,
                        // so the row is safe to drop immediately (mirrors paper's
                        // "no entry fill" cleanup). Release the SOL commitment taken
                        // before spawn so the wallet budget isn't pinned for 10 min.
                        trader.release_sol_for_position(&position_id.to_string());
                        match repo.delete_position(position_id).await {
                            Ok(()) => runtime.remove_position(&position),
                            Err(err) => warn!(
                                "[REAL] Failed to drop unentered position {position_id} after arming timeout: {err}"
                            ),
                        }
                    }
                } else {
                    paper::spawn_entry_fill_poll(
                        repo, runtime, token_cache, mint, position_id, rule, params,
                    );
                }
            });
        }
    }

    pub async fn on_trade_executed(&self, mint: &str, cache: &TokenCache) {
        // Cheap holding-index lookup first: the vast majority of trade pings are for
        // mints we hold no position in, so probe the index on the borrowed `&str` and
        // bail before allocating an owned mint or touching the trade history.
        let positions = self.runtime.holding_by_mint(mint);
        if positions.is_empty() {
            return;
        }
        let mint = mint.to_string();

        // Evaluate every position's exit decision while holding the cache read guard.
        // The exit ladder + the clock-state fold are synchronous, so we walk the
        // in-memory trade history in place — no per-event deep clone. Collect the
        // positions that must exit, then drop the guard before any await.
        let current_price;
        let mut to_exit: Vec<(Arc<StrategyPosition>, &'static str)> = Vec::new();
        // Proactive manual-sell detection: capture the latest Sell from the bot's own
        // wallet so we can confirm + close any real Holding position the ladder didn't.
        let mut manual_sell_info: Option<(f64, DateTime<Utc>)> = None;
        {
            let Some(entry) = cache.get(&mint) else {
                return;
            };
            let state = entry.value();
            // Keep the Option: a missing price falls back per-position to the
            // position's own entry_price, so a failed exit records a 0% move instead
            // of a bogus −100% (matches the sweep path).
            current_price = state.current_price;
            let trades = &state.trades;
            let trades_base = state.trades_base;

            // Scan the ring backwards for the most recent bot-wallet sell. Checking
            // only `.last()` is fragile — the ping channel is async, so later trades
            // can push the sell off the tail. O(k) with an early stop on first hit.
            let bot_wallet = self.trader.wallet_pubkey();
            if let Some(bot_id) = state.interner.id_of(&bot_wallet) {
                if let Some(sell) = trades.iter().rev().find(|t| !t.is_buy && t.wallet == bot_id) {
                    manual_sell_info = Some((sell.price_per_token, sell.block_time));
                }
            }

            for position in &positions {
                let Some(rule_id) = position.rule_id else {
                    continue;
                };
                // Holding + recorded entry only; a 0-entry/non-Holding position is
                // neither evaluated nor folded.
                let Some(entry_time) = clock_entry_time(position) else {
                    continue;
                };
                // Incremental trade gate: fold the newly-printed trades into the
                // position's memoized walk state AND evaluate the exit ladder against
                // only those new trades in one pass — no per-ping full re-walk.
                // Dispatched by the rule's stored ladder.
                if let Some(exit_reason) = self.runtime.exit_state_advance_and_find_exit(
                    position.id,
                    rule_id,
                    position.entry_price.unwrap_or(0.0),
                    entry_time,
                    trades,
                    trades_base,
                ) {
                    to_exit.push((position.clone(), exit_reason));
                }
            }
        }

        // IDs the ladder already claimed — so the manual-sell pass skips them.
        let exiting_ids: HashSet<uuid::Uuid> = to_exit.iter().map(|(p, _)| p.id).collect();

        for (position, exit_reason) in to_exit {
            let Some(rule_id) = position.rule_id else {
                continue;
            };
            // This position is actually exiting (rare) — now it's worth the full rule
            // + params clone the execution paths need.
            let Some(rule) = self.runtime.rule_by_id(rule_id) else {
                continue;
            };
            let Some(params) = self.runtime.params_by_id(rule_id) else {
                continue;
            };
            let mut position = (*position).clone();
            let exit_price = current_price.or(position.entry_price).unwrap_or(0.0);
            debug!("Position {} for token {mint} triggered exit: {exit_reason}", position.id);

            if rule.trade_mode == "paper" {
                // Claim the exit; if one's already in flight skip (no second poll).
                let Some(guard) = self.runtime.try_begin_exit(position.id) else {
                    continue;
                };
                let prev = position.clone();
                position.mark_exit_pending();
                // Swing1's exit memo is about to be evicted (leaving the holding
                // index) — peek its legs now so they land in the SAME write as the
                // status flip, no second round-trip. A peek, not the eviction: safe
                // even if the write below fails.
                if let Some(legs) = self.runtime.peek_swing_legs(position.id) {
                    trading_core::strategies::runtime_cache::merge_swing_legs_into_extra(
                        &mut position,
                        legs,
                    );
                }
                if let Err(err) = self.repo.update_position(&position).await {
                    warn!("Failed to mark position {} as ExitPending: {err}", position.id);
                    // The write failed → still Holding. Dropping `guard` releases the
                    // claim and skips the spawn; otherwise the next ping re-fires the
                    // ladder and spawns another poll (an unbounded task storm).
                    continue;
                }
                self.runtime.sync_position(Some(&prev), &position);
                info!(position_id = %position.id, mint = %mint, "[PAPER] Position marked ExitPending");
                paper::spawn_exit_fill_poll(
                    self.repo.clone(),
                    self.runtime.clone(),
                    self.token_cache.clone(),
                    self.sse_tx.clone(),
                    mint.clone(),
                    position.id,
                    position.entry_price.unwrap_or(0.0),
                    position.entry_time,
                    rule.clone(),
                    params,
                    exit_price,
                    Utc::now(),
                    exit_reason.to_string(),
                    guard,
                );
            } else if rule.trade_mode == "real" {
                self.trigger_real_exit(position, exit_price, Utc::now(), exit_reason.to_string())
                    .await;
            }
        }

        // Proactive manual-sell close for real Holding positions the ladder did NOT
        // exit. The guard claim inside `try_close_manually_sold` is the double-exit
        // interlock: a position already claimed above returns `None` and is skipped.
        if let Some((sell_price, sell_time)) = manual_sell_info {
            for position in &positions {
                if exiting_ids.contains(&position.id) {
                    continue;
                }
                let Some(rule_id) = position.rule_id else {
                    continue;
                };
                let Some(rule) = self.runtime.rule_by_id(rule_id) else {
                    continue;
                };
                if rule.trade_mode != "real" {
                    continue;
                }
                if clock_entry_time(position).is_none() {
                    continue;
                }
                self.try_close_manually_sold(position.clone(), mint.clone(), sell_price, sell_time)
                    .await;
            }
        }
    }

    /// Wall-clock exit sweep. For every Holding position whose rule carries a
    /// time-based exit (E2 TimeStop / E3 Stall), evaluate it against `now` via the
    /// memoized exit state, so time exits fire even when a token goes silent — the
    /// gap the trade-driven path leaves open. Price exits stay on the trade path, so
    /// the two never overlap. Runs in the **same task** as `on_trade_executed`, so
    /// the two can't race on a Holding→ExitPending transition without DB locking.
    pub async fn sweep_time_exits(&self, cache: &TokenCache) {
        let now = Utc::now();

        for position in self.runtime.time_exit_holding_positions() {
            let Some(rule_id) = position.rule_id else {
                continue;
            };
            // Cheap defensive check against index/rule skew.
            let Some(ladder) = self.runtime.ladder_params_by_id(rule_id) else {
                continue;
            };
            if !ladder.has_time_exit() {
                continue;
            }
            // Skip until the entry fill is indexed (the clock gate would reject it
            // anyway, and we avoid seeding state for a not-yet-open position).
            if clock_entry_time(&position).is_none() {
                continue;
            }

            // The walk state comes from the memoized cache (kept current by the trade
            // path) so the sweep never re-walks the history. `last_price` is the paper
            // fill mark; a not-yet-seeded position seeds from the trade vec just once,
            // inside `exit_state_clock_check`.
            let (reason, last_price) = match cache.get(&position.mint_address) {
                Some(entry) => {
                    let st = entry.value();
                    let last_price = st.current_price.or(position.entry_price).unwrap_or(0.0);
                    let reason =
                        self.runtime.exit_state_clock_check(&position, &st.trades, st.trades_base, now);
                    (reason, last_price)
                }
                // Token absent from the cache (e.g. evicted) can still TimeStop from
                // entry_time alone — seed from an empty history if needed.
                None => {
                    let reason = self.runtime.exit_state_clock_check(&position, &[], 0, now);
                    (reason, position.entry_price.unwrap_or(0.0))
                }
            };
            let Some(exit_reason) = reason else {
                continue;
            };

            info!(position_id = %position.id, mint = %position.mint_address,
                "Time-driven exit triggered: {exit_reason}");

            let Some(rule) = self.runtime.rule_by_id(rule_id) else {
                continue;
            };
            let position = (*position).clone();
            if rule.trade_mode == "paper" {
                paper::record_time_exit(
                    &self.repo,
                    &self.runtime,
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

    /// Mark a real position ExitPending inline, then **spawn** the slow sell pass off
    /// the runner's select task. The ExitPending mark + holding-index sync happen
    /// inline so the position leaves the holding index before the next ping/sweep; the
    /// sell (which can await tens of seconds) is spawned so it never stalls ping
    /// handling. The atomic exit guard is the no-double-sell interlock across the
    /// spawn boundary even if the ExitPending write fails.
    async fn trigger_real_exit(
        &self,
        mut position: StrategyPosition,
        trigger_price: f64,
        trigger_time: DateTime<Utc>,
        reason: String,
    ) {
        let Some(guard) = self.runtime.try_begin_exit(position.id) else {
            return;
        };
        let prev = position.clone();
        position.mark_exit_pending();
        // Peek (not evict) swing1's exit memo so its legs land in this same write.
        if let Some(legs) = self.runtime.peek_swing_legs(position.id) {
            trading_core::strategies::runtime_cache::merge_swing_legs_into_extra(&mut position, legs);
        }
        if let Err(err) = self.repo.update_position(&position).await {
            warn!("Failed to mark position {} as ExitPending: {err}", position.id);
        } else {
            self.runtime.sync_position(Some(&prev), &position);
            info!(position_id = %position.id, mint = %position.mint_address, "[REAL] Position marked ExitPending");
        }
        self.spawn_real_sell(position, trigger_price, trigger_time, reason, guard);
    }

    /// Spawn the slow sell pass off the runner's task, holding the RAII exit `guard`
    /// for its lifetime (freed when the task ends OR panics). Shared by the ladder/time
    /// exit (`trigger_real_exit`, which marks ExitPending first) and the ExitPending
    /// reaper (`redrive_orphaned_exit_pending`, where the row is already ExitPending).
    fn spawn_real_sell(
        &self,
        position: StrategyPosition,
        trigger_price: f64,
        trigger_time: DateTime<Utc>,
        reason: String,
        guard: ExitGuard,
    ) {
        let trader = self.trader.clone();
        let repo = self.repo.clone();
        let trade_repo = self.trade_repo.clone();
        let runtime = self.runtime.clone();
        let token_cache = self.token_cache.clone();
        let trade_signals = self.trade_signals.clone();
        let slippage_bps = self.sell_slippage();
        tokio::spawn(async move {
            let _guard = guard;
            real::sell_and_close_position(
                trader,
                position,
                repo,
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

    /// Close a real `Holding` position externally (manually) sold out, without
    /// sending a new sell tx. Verifies the DB net balance is ≤ threshold before
    /// committing (guards against a same-wallet sell on a different concurrent
    /// position), claims the exit guard (double-exit interlock), marks ExitPending,
    /// then spawns the direct close.
    async fn try_close_manually_sold(
        &self,
        position: Arc<StrategyPosition>,
        mint: String,
        sell_price: f64,
        sell_time: DateTime<Utc>,
    ) {
        let wallet = self.trader.wallet_pubkey();
        match self.trade_repo.net_token_amount_by_wallet_and_mint(&wallet, &mint).await {
            Ok(balance) if balance <= super::execution::PARTIAL_FILL_THRESHOLD => {}
            Ok(_) => return, // bag still positive — not fully cleared yet
            Err(err) => {
                warn!(position_id = %position.id, mint = %mint,
                    "manual-sell check: balance query failed, skipping: {err}");
                return;
            }
        }
        let Some(guard) = self.runtime.try_begin_exit(position.id) else {
            return;
        };
        let mut position = (*position).clone();
        let prev = position.clone();
        position.mark_exit_pending();
        // Peek (not evict) swing1's exit memo so its legs land in this same write.
        if let Some(legs) = self.runtime.peek_swing_legs(position.id) {
            trading_core::strategies::runtime_cache::merge_swing_legs_into_extra(&mut position, legs);
        }
        if let Err(err) = self.repo.update_position(&position).await {
            warn!(position_id = %position.id, mint = %mint,
                "Failed to mark position ExitPending (ManualSell): {err}");
        } else {
            self.runtime.sync_position(Some(&prev), &position);
            info!(position_id = %position.id, mint = %mint,
                "[REAL] Position marked ExitPending (ManualSell)");
        }
        let repo = self.repo.clone();
        let runtime = self.runtime.clone();
        let trader = self.trader.clone();
        tokio::spawn(async move {
            let _guard = guard;
            real::close_externally_cleared_position(
                &mut position,
                &repo,
                &runtime,
                &trader,
                sell_price,
                sell_time,
                "ManualSell",
            )
            .await;
        });
    }

    /// Reconcile real positions left `Holding` after their bag was cleared outside the
    /// strategy exit path (a manual "Sell" / "Sell All"). Runs at boot (reaper's first
    /// tick) and on the maintenance cadence — one set-based candidate query, normally
    /// empty.
    async fn reconcile_externally_cleared_holdings(&self) {
        let mints = match self
            .repo
            .find_externally_cleared_holding_mints(super::execution::PARTIAL_FILL_THRESHOLD)
            .await
        {
            Ok(m) => m,
            Err(err) => {
                warn!("manual-sell reaper: candidate query failed: {err}");
                return;
            }
        };
        for mint in mints {
            real::reconcile_externally_cleared_mint(
                &mint,
                &self.repo,
                &self.trade_repo,
                &self.runtime,
                &self.trader,
            )
            .await;
        }
    }

    /// Re-drive real positions stranded in `ExitPending` whose exit guard is **not**
    /// held — their sell task panicked, or the process crashed mid-exit and restarted.
    /// The atomic `try_begin_exit` claim **is** the double-sell interlock. A position
    /// that never recorded an entry can't be sold, so it's left for the stale reaper.
    async fn redrive_orphaned_exit_pending(&self) {
        let pending = match self.repo.find_all_exit_pending("real").await {
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
            let Some(guard) = self.runtime.try_begin_exit(position.id) else {
                continue;
            };
            let trigger_price = self
                .token_cache
                .get(&position.mint_address)
                .and_then(|e| e.value().current_price)
                .or(position.entry_price)
                .unwrap_or(0.0);
            let reason = position
                .exit_reason
                .clone()
                .unwrap_or_else(|| EXIT_PENDING_RECOVERY_REASON.to_string());
            info!(position_id = %position.id, mint = %position.mint_address,
                "[REAL] Re-driving orphaned ExitPending sell");
            self.spawn_real_sell(position, trigger_price, Utc::now(), reason, guard);
        }
    }

    /// Recover real positions stranded in `BuySubmitted` — a buy was signed/sent (so
    /// tokens may exist on-chain) but its fill was never recorded. **Never re-sends and
    /// never blindly deletes** (a durable-nonce buy can still land → re-firing would
    /// double-buy). Per row, by its persisted `submitted_buy_signatures`: **Adopt** an
    /// indexed fill; **Drop** only if *every* sig confirmed-reverted; else **Wait**
    /// (flag for review past a max age). The atomic `try_begin_entry` claim is the
    /// interlock against a live buy task.
    async fn redrive_orphaned_buy_submitted(&self) {
        let submitted = match self.repo.find_all_buy_submitted("real").await {
            Ok(p) => p,
            Err(err) => {
                warn!("Failed to load BuySubmitted positions for recovery: {err}");
                return;
            }
        };
        let wallet = self.trader.wallet_pubkey();
        for position in submitted {
            let Some(_guard) = self.runtime.try_begin_entry(position.id) else {
                continue;
            };

            // 1. Adopt: a submitted sig is already indexed → record the entry.
            if real::adopt_existing_fill_if_present(
                &position.mint_address,
                &wallet,
                position.id,
                &self.repo,
                &self.trade_repo,
                &self.runtime,
                &position.submitted_buy_signatures,
                // The recovery reaper has no fresher account than the row itself;
                // pass it through (COALESCE no-ops if unchanged, repopulates if the
                // trader cache later warms it elsewhere).
                position.token_account.as_deref(),
            )
            .await
            {
                info!(position_id = %position.id, mint = %position.mint_address,
                    "[REAL] Recovered BuySubmitted position: adopted on-chain fill");
                continue;
            }

            // 2. No fill, no signatures → can't check → wait (never delete).
            if position.submitted_buy_signatures.is_empty() {
                warn!(position_id = %position.id, mint = %position.mint_address,
                    "BuySubmitted position has no submitted signatures — leaving for review");
                continue;
            }

            // 3. Drop ONLY if EVERY submitted sig is a confirmed revert.
            let mut all_reverted = true;
            for sig in &position.submitted_buy_signatures {
                let status = self.trader.signature_state(sig).await;
                if real::classify_submitted_buy(&status) == real::BuyRecoveryVerdict::Wait {
                    all_reverted = false;
                    break;
                }
            }

            if all_reverted {
                match self.repo.delete_position(position.id).await {
                    Ok(()) => {
                        // Release the SOL commitment now that we know no tokens were
                        // acquired. This is the only drop path for BuySubmitted rows
                        // (the inline cleanup was removed); without it the budget
                        // tracker permanently holds the committed SOL until restart.
                        self.trader.release_sol_for_position(&position.id.to_string());
                        self.runtime.remove_position(&position);
                        info!(position_id = %position.id, mint = %position.mint_address,
                            "[REAL] Dropped reverted BuySubmitted position (every buy reverted; no tokens)");
                    }
                    Err(err) => warn!(position_id = %position.id,
                        "Failed to drop reverted BuySubmitted position: {err}"),
                }
            } else {
                let age = Utc::now().signed_duration_since(position.updated_at);
                if age > chrono::Duration::milliseconds(Self::BUY_SUBMITTED_REVIEW_MS as i64) {
                    warn!(position_id = %position.id, mint = %position.mint_address,
                        "BuySubmitted position unresolved past the review window — needs manual \
                         review (a buy may have landed but never indexed); NOT auto-deleting");
                }
            }
        }
    }
}

// ── Rule lifecycle ──────────────────────────────────────────────────────────────
//
// The **single source of truth** for activate / pause / stop-and-close. The rule-CRUD
// edge (T5 handlers) calls these so the run + rules-cache side effects can never drift
// between code paths. Every position carries a run now (both modes), so activation
// (re)starts a run for real rules too — `on_token_created` stamps new positions with
// the current run and caps are per current run.
impl StrategyService {
    /// Frontend strategy label for [`SseEvent::TpslRulesChanged`]
    /// ("tpsl1" / "tpsl2" / "swing_1"). Must match the string the corresponding
    /// frontend page filters on — swing1 filters `swing_1`, so it can't collapse
    /// into the tpsl1 catch-all or its rule-list refetch would never fire.
    fn strat_label(strategy_id: &str) -> String {
        match StrategyImpl::from_id(strategy_id) {
            Some(StrategyImpl::Tpsl2) => "tpsl2".to_string(),
            Some(StrategyImpl::Swing1) => "swing_1".to_string(),
            _ => "tpsl1".to_string(),
        }
    }

    /// Best-effort cold-lane signal that this strategy's rule list changed, so SSE
    /// clients refetch it. No subscribers ⇒ the send is a no-op.
    fn emit_rules_changed(&self, strategy_id: &str) {
        let _ = self.sse_tx.send(SseEvent::TpslRulesChanged {
            strategy: Self::strat_label(strategy_id),
        });
    }

    /// Activate a rule (entries on) and (re)start its run per `activation`. Persists
    /// `is_active = true`, applies the run side effect, then refreshes the rules cache
    /// so the hot path sees the rule. Returns the updated rule.
    pub async fn activate_rule(
        &self,
        rule_id: Uuid,
        activation: PaperActivation,
    ) -> anyhow::Result<StrategyRule> {
        let mut rule = self
            .repo
            .find_rule(rule_id)
            .await?
            .ok_or_else(|| anyhow!("Rule not found"))?;
        rule.is_active = true;
        self.repo.update_rule(&rule).await?;

        match activation {
            PaperActivation::Fresh => {
                self.runtime.start_run(&self.repo, &rule).await?;
            }
            PaperActivation::Continue => {
                // Resume the prior run; fall back to a fresh one if there is none.
                if self
                    .runtime
                    .resume_run(&self.repo, rule_id, &rule.trade_mode)
                    .await?
                    .is_none()
                {
                    self.runtime.start_run(&self.repo, &rule).await?;
                }
            }
        }

        self.runtime.reload_rules(&self.repo).await?;
        self.emit_rules_changed(&rule.strategy_id);
        Ok(rule)
    }

    /// Pause a rule (entries off; open positions left to drain via the exit ladder —
    /// `on_trade_executed` / the time sweep still exit them). Marks the current run
    /// `Stopped`. Returns the updated rule.
    pub async fn pause_rule(&self, rule_id: Uuid) -> anyhow::Result<StrategyRule> {
        let mut rule = self
            .repo
            .find_rule(rule_id)
            .await?
            .ok_or_else(|| anyhow!("Rule not found"))?;
        let was_active = rule.is_active;
        rule.is_active = false;
        self.repo.update_rule(&rule).await?;

        if was_active {
            self.runtime.stop_run(&self.repo, rule_id).await?;
        }
        self.runtime.reload_rules(&self.repo).await?;
        self.emit_rules_changed(&rule.strategy_id);
        Ok(rule)
    }

    /// Stop a rule **and force-close every open position now**. Pauses first (so the
    /// run is `Stopped` and the cap-reached finish logic can't fire mid-close), then
    /// exits each holding position at its last observed price: paper rows close in
    /// place; real rows mark `ExitPending` and sell on-chain in a spawned task so a
    /// many-position close doesn't block the response. Returns the updated rule and
    /// the number of positions that were closing.
    pub async fn stop_and_close_rule(
        &self,
        rule_id: Uuid,
    ) -> anyhow::Result<(StrategyRule, usize)> {
        // Grabbed before `pause_rule` (→ `stop_run` → `finalize_run`) so we can
        // re-check emptiness below even if that first pass left the run `Stopped`
        // because a since-deleted 0-entry position was still on it at the time.
        let run_id_before_close = self.runtime.current_run(rule_id).map(|r| r.run_id);
        let rule = self.pause_rule(rule_id).await?;

        // Authoritative snapshot of this rule's open positions — the in-memory holding
        // index is the live source of truth across both modes.
        let positions: Vec<StrategyPosition> = self
            .runtime
            .all_holding_positions()
            .into_iter()
            .filter(|p| p.rule_id == Some(rule_id))
            .map(|p| (*p).clone())
            .collect();
        let count = positions.len();
        let now = Utc::now();

        for position in positions {
            let position_id = position.id;
            // 0-entry positions never received a fill — delete them rather than
            // stamping a ManualClose exit at price 0. Claim the guard first so this
            // can't race a concurrent ladder/time exit.
            if position.entry_price.is_none() {
                let Some(_guard) = self.runtime.try_begin_exit(position_id) else {
                    continue;
                };
                let _ = self.repo.delete_position(position_id).await;
                self.runtime.remove_position(&position);
                continue;
            }
            // The last observed mark is the honest exit price; fall back to entry for
            // a token the WS cache has since evicted.
            let exit_price = self
                .token_cache
                .get(&position.mint_address)
                .and_then(|e| e.value().current_price)
                .unwrap_or_else(|| position.entry_price.unwrap_or(0.0));

            if rule.trade_mode == "paper" {
                // Claim the shared exit guard so this manual close can't race a
                // concurrent ladder/time exit into a double-close.
                let Some(_guard) = self.runtime.try_begin_exit(position_id) else {
                    continue;
                };
                paper::record_time_exit(
                    &self.repo,
                    &self.runtime,
                    &self.sse_tx,
                    position,
                    exit_price,
                    now,
                    MANUAL_CLOSE_REASON.to_string(),
                    &rule,
                )
                .await;
                // `record_time_exit` closes synchronously; `_guard` drops here.
            } else {
                // Real: `trigger_real_exit` claims the guard, marks ExitPending, and
                // spawns the slow sell off the request path.
                self.trigger_real_exit(position, exit_price, now, MANUAL_CLOSE_REASON.to_string())
                    .await;
            }
        }

        // Re-check emptiness: the loop above may have just deleted the last 0-entry
        // (never-filled) position row that made the run look non-empty during
        // `pause_rule`'s finalize pass.
        if let Some(run_id) = run_id_before_close {
            if self.repo.run_position_count(run_id).await? == 0 {
                let _ = self.repo.delete_run(run_id).await;
            }
        }

        Ok((rule, count))
    }

    /// Pause every currently active rule for `strategy_id` in `trade_mode`. Each
    /// rule runs through the exact single-rule `pause_rule` path (sequential, not
    /// concurrent — this is an infrequent admin action, not a hot path), so a
    /// per-rule failure doesn't block the rest of the batch. Returns one result
    /// per targeted rule.
    pub async fn pause_all_rules(
        &self,
        strategy_id: &str,
        trade_mode: &str,
    ) -> anyhow::Result<Vec<(Uuid, anyhow::Result<StrategyRule>)>> {
        let rules = self.repo.find_rules_by_strategy(strategy_id).await?;
        let mut out = Vec::new();
        for r in rules.into_iter().filter(|r| r.trade_mode == trade_mode && r.is_active) {
            out.push((r.id, self.pause_rule(r.id).await));
        }
        Ok(out)
    }

    /// Stop-and-close every rule for `strategy_id` in `trade_mode` that is either
    /// active or still holding open positions (mirrors the per-row Stop button's
    /// visibility: Active + Draining, not Idle/Finished). Sequential, same
    /// rationale as [`pause_all_rules`].
    pub async fn stop_all_rules(
        &self,
        strategy_id: &str,
        trade_mode: &str,
    ) -> anyhow::Result<Vec<(Uuid, anyhow::Result<(StrategyRule, usize)>)>> {
        let rules = self.repo.find_rules_by_strategy(strategy_id).await?;
        let mut out = Vec::new();
        for r in rules.into_iter().filter(|r| {
            r.trade_mode == trade_mode && (r.is_active || self.runtime.holding_count_by_rule(r.id) > 0)
        }) {
            out.push((r.id, self.stop_and_close_rule(r.id).await));
        }
        Ok(out)
    }

    // ── Rule CRUD (validate via the core `rules` domain, then append the live
    //    side effects the domain deliberately leaves to the edge: cache
    //    `reload_rules` + `rules_changed` SSE) ──────────────────────────────────

    /// Validate + persist a new (inactive) rule, then refresh the rules cache and
    /// notify SSE. A lifecycle endpoint activates it afterward.
    pub async fn create_rule(&self, draft: &RuleDraft) -> Result<StrategyRule, RuleError> {
        let rule = rules::create(&self.repo, draft).await?;
        self.runtime.reload_rules(&self.repo).await.map_err(RuleError::Repo)?;
        self.emit_rules_changed(&rule.strategy_id);
        Ok(rule)
    }

    /// Re-validate + persist an edited rule (the edge merges its request patch and
    /// enforces any frozen-field guard first), then refresh the cache + notify SSE.
    pub async fn save_rule(&self, rule: &StrategyRule) -> Result<(), RuleError> {
        rules::save(&self.repo, rule).await?;
        self.runtime.reload_rules(&self.repo).await.map_err(RuleError::Repo)?;
        self.emit_rules_changed(&rule.strategy_id);
        Ok(())
    }

    /// Delete a rule. An active rule is stopped-and-closed first (so no open real
    /// position is orphaned), then its in-memory run state is purged and the row
    /// deleted; finally the cache is refreshed and SSE notified. Returns `false`
    /// if the rule did not exist.
    pub async fn delete_rule(&self, rule_id: Uuid) -> anyhow::Result<bool> {
        let Some(rule) = self.repo.find_rule(rule_id).await? else {
            return Ok(false);
        };
        if rule.is_active {
            self.stop_and_close_rule(rule_id).await?;
        }
        self.runtime.clear_rule(rule_id);
        self.repo.delete_rule(rule_id).await?;
        self.runtime.reload_rules(&self.repo).await?;
        self.emit_rules_changed(&rule.strategy_id);
        Ok(true)
    }
}
