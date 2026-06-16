use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

use crate::models::ingest::SseEvent;
use crate::models::Position;
use crate::state::token_cache::TokenCache;
use crate::state::trade_signals::TradeSignals;
use crate::storage::repositories::{
    tpsl1_paper_trading_repo::Tpsl1PaperTradingRepo, tpsl1_position_repo::Tpsl1PositionRepo,
    trade_repo::TradeRepo,
};
use super::{TPSL1StrategyHandler, Tpsl1RuntimeCache};
use crate::trader::PumpFunTrader;
use super::util::none_if_zero_u64;

pub struct Tpsl1StrategyService {
    pool: PgPool,
    position_repo: Tpsl1PositionRepo,
    paper_repo: Tpsl1PaperTradingRepo,
    trade_repo: TradeRepo,
    trader: Arc<PumpFunTrader>,
    runtime: Arc<Tpsl1RuntimeCache>,
    /// WS-fed price/trade source of truth; cloned into spawned real-sell tasks.
    token_cache: Arc<TokenCache>,
    /// Cold-lane broadcast for client notifications (e.g. paper-run finished).
    sse_tx: broadcast::Sender<SseEvent>,
    /// Persisted-trade wakeup hub for the snipe buy confirm loop.
    trade_signals: Arc<TradeSignals>,
}

impl Tpsl1StrategyService {
    pub fn new(
        pool: PgPool,
        trader: Arc<PumpFunTrader>,
        runtime: Arc<Tpsl1RuntimeCache>,
        token_cache: Arc<TokenCache>,
        sse_tx: broadcast::Sender<SseEvent>,
        trade_signals: Arc<TradeSignals>,
    ) -> Self {
        Self {
            position_repo: Tpsl1PositionRepo::new(pool.clone()),
            paper_repo: Tpsl1PaperTradingRepo::new(pool.clone()),
            trade_repo: TradeRepo::new(pool.clone()),
            pool,
            trader,
            runtime,
            token_cache,
            sse_tx,
            trade_signals,
        }
    }

    const EXIT_PENDING_CLEANUP_INTERVAL_MS: u64 = 60_000;
    const EXIT_PENDING_STALE_MS: u64 = 300_000;

    /// Age past which an unentered (0-entry) Holding paper position is reaped as an
    /// orphan. Must comfortably exceed the largest real arming window
    /// (`SCALP_ARMING_BASE_SECS` + the rule's time gates, at most a few minutes) so
    /// the reaper only ever catches rows whose in-memory arming task is gone — never
    /// races a poll that is still legitimately working.
    const UNENTERED_STALE_MS: u64 = 600_000;
}

impl Tpsl1StrategyService {
    pub fn spawn_background_tasks(&self) {
        let cleanup_repo = self.position_repo.clone();
        let paper_repo = self.paper_repo.clone();
        let runtime = self.runtime.clone();
        let pool = self.pool.clone();
        tokio::spawn(async move {
            let stale = Duration::from_millis(Tpsl1StrategyService::EXIT_PENDING_STALE_MS);
            let unentered_stale =
                Duration::from_millis(Tpsl1StrategyService::UNENTERED_STALE_MS);
            let mut interval = tokio::time::interval(Duration::from_millis(
                Tpsl1StrategyService::EXIT_PENDING_CLEANUP_INTERVAL_MS,
            ));
            loop {
                interval.tick().await;
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
                // Reap orphaned unentered paper positions whose in-memory arming
                // task was lost to a restart/resume (never entered, exited, or
                // deleted otherwise).
                match paper_repo.delete_stale_unentered(unentered_stale).await {
                    Ok(n) if n > 0 => {
                        info!("Reaped {n} orphaned unentered paper positions");
                        changed = true;
                    }
                    Ok(_) => {}
                    Err(err) => warn!("Failed to reap orphaned unentered paper positions: {err}"),
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
        let mint = mint.to_string();
        let token = match cache.get(&mint) {
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

        let handler = TPSL1StrategyHandler::new(rules);
        if let Some(rule_id) = handler.check_buy_entry(&token) {
            info!("Token {mint} matches TPSL buy entry rule {rule_id}");

            if let Some(rule) = handler.get_rule(rule_id) {
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
                                return;
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
                        return;
                    }
                }

                if let Some(total_max) = max_total_tokens {
                    let total_traded = self.runtime.total_count_by_rule(rule_id);
                    if total_traded >= total_max as i64 {
                        debug!(
                            "Rule {rule_id} reached max total tokens \
                             ({total_traded}/{total_max}), skipping {mint}"
                        );
                        return;
                    }
                }

                let mut position = Position::new(
                    mint.clone(),
                    self.trader.wallet_pubkey(),
                    0.0,
                    token.creation_tx_signature.clone(),
                    "TPSL1".to_string(),
                    rule_id,
                    rule.buy_amount,
                );
                position.token_program_id = token.token_program_id.clone();

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

                let is_real = rule.trade_mode == "real";
                let buy_amount = rule.buy_amount;
                let creator = token.creator_wallet.clone();
                let position_id = position.id;
                let token_program_id = position
                    .token_program_id
                    .clone()
                    .unwrap_or_else(|| crate::config::constants::TOKEN_PROGRAM_ID.to_string());
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
                        // Roll back the inline cap/holding-index claim.
                        runtime.remove_position(&position);
                        return;
                    }
                    info!("Created position {position_id} for token {mint} under rule {rule_id}");

                    if is_real {
                        super::execution::real::buy_until_filled_or_give_up(
                            trader,
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
                        )
                        .await;
                        if let Ok(Some(pos)) = position_repo.find_by_id(position_id).await {
                            if pos.entry_price == 0.0 {
                                let _ = position_repo.delete_position(position_id).await;
                                runtime.remove_position(&pos);
                                info!(
                                    "[REAL] Removed position {} for mint {}: buy not found",
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
                            buy_amount,
                        );
                    }
                });
            }
        } else {
            debug!("Token {mint} does not match any TPSL buy entry rule");
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
        {
            let Some(entry) = cache.get(&mint) else {
                return;
            };
            let state = entry.value();
            current_price = state.current_price.unwrap_or(0.0);
            let trades = &state.trades;
            let trades_base = state.trades_base;

            for position in positions {
                // Hot-path gate: pull only the exit-ladder scalars, not a full
                // rule clone (that happens below, once per *actual* exit).
                let Some(params) = self.runtime.ladder_params_by_id(position.rule_id) else {
                    continue;
                };
                // Holding + recorded entry only; a 0-entry/non-Holding position is
                // neither evaluated nor folded (matches the old gate guards).
                let Some(entry_time) = super::exit::clock_entry_time(&position) else {
                    continue;
                };
                // Incremental trade gate: fold the newly-printed trades into the
                // position's memoized walk state AND evaluate the exit ladder
                // against only those new trades in one pass — no per-ping full
                // re-walk of the retained history. The memo doubles as the
                // clock-sweep's current snapshot, so the sweep still never re-walks.
                if let Some(exit_reason) = self.runtime.exit_state_advance_and_find_exit(
                    position.id,
                    position.entry_price,
                    entry_time,
                    trades,
                    trades_base,
                    &params,
                ) {
                    to_exit.push((position, exit_reason));
                }
            }
        }

        for (position, exit_reason) in to_exit {
            // This position is actually exiting (rare) — now it's worth the full
            // rule clone the execution paths need.
            let Some(rule) = self.runtime.rule_by_id(position.rule_id) else {
                continue;
            };
            // Deep-clone only now that this position is actually exiting; the
            // holding index hands us a shared `Arc<Position>`.
            let mut position = (*position).clone();
            debug!(
                "Position {} for token {mint} triggered exit: {:?}",
                position.id, exit_reason
            );

            if rule.trade_mode == "paper" {
                // Claim the exit; if one is already in flight for this position
                // skip (don't spawn a second fill-poll). Released by the poll task.
                if !self.runtime.try_begin_exit(position.id) {
                    continue;
                }
                let prev = position.clone();
                position.mark_exit_pending();
                if let Err(err) = self.paper_repo.update(&position).await {
                    warn!(
                        "Failed to mark position {} as ExitPending: {err}",
                        position.id
                    );
                    // The write failed, so the position is still Holding. Release
                    // the claim and skip the spawn — otherwise the next ping/sweep
                    // re-fires the ladder and spawns another poll, an unbounded
                    // task storm under a dump that saturates `paper_poll_sem`.
                    self.runtime.end_exit(position.id);
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
                    position.entry_tx.clone(),
                    position.entry_price,
                    position.entry_time,
                    rule.clone(),
                    current_price,
                    Utc::now(),
                    exit_reason.to_string(),
                );
            } else if rule.trade_mode == "real" {
                self.trigger_real_exit(
                    position,
                    current_price,
                    Utc::now(),
                    exit_reason.to_string(),
                )
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
                    let last_price = st.current_price.unwrap_or(position.entry_price);
                    let state = self.runtime.exit_state_get(position.id).unwrap_or_else(|| {
                        self.runtime.exit_state_build(
                            position.id,
                            position.entry_price,
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
                            .exit_state_build(position.id, position.entry_price, entry_time, &[], 0)
                    });
                    (state, position.entry_price)
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
        if !self.runtime.try_begin_exit(position.id) {
            return;
        }

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

        let position_id = position.id;
        let trader = self.trader.clone();
        let position_repo = self.position_repo.clone();
        let trade_repo = self.trade_repo.clone();
        let runtime = self.runtime.clone();
        let exit_guard = self.runtime.clone();
        let token_cache = self.token_cache.clone();
        let trade_signals = self.trade_signals.clone();
        tokio::spawn(async move {
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
            )
            .await;
            exit_guard.end_exit(position_id);
        });
    }
}
