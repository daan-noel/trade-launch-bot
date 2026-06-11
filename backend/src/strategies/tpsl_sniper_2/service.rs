use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

use crate::models::ingest::SseEvent;
use crate::models::Position;
use crate::state::token_cache::TokenCache;
use crate::storage::repositories::{
    tpsl2_paper_trading_repo::Tpsl2PaperTradingRepo, tpsl2_position_repo::Tpsl2PositionRepo,
    trade_repo::TradeRepo,
};
use super::{TPSL2StrategyHandler, Tpsl2RuntimeCache};
use crate::trader::PumpFunTrader;
use super::util::none_if_zero_u64;

pub struct Tpsl2StrategyService {
    pool: PgPool,
    position_repo: Tpsl2PositionRepo,
    paper_repo: Tpsl2PaperTradingRepo,
    trade_repo: TradeRepo,
    trader: Arc<PumpFunTrader>,
    runtime: Arc<Tpsl2RuntimeCache>,
    /// Cold-lane broadcast for client notifications (e.g. paper-run finished).
    sse_tx: broadcast::Sender<SseEvent>,
}

impl Tpsl2StrategyService {
    pub fn new(
        pool: PgPool,
        trader: Arc<PumpFunTrader>,
        runtime: Arc<Tpsl2RuntimeCache>,
        sse_tx: broadcast::Sender<SseEvent>,
    ) -> Self {
        Self {
            position_repo: Tpsl2PositionRepo::new(pool.clone()),
            paper_repo: Tpsl2PaperTradingRepo::new(pool.clone()),
            trade_repo: TradeRepo::new(pool.clone()),
            pool,
            trader,
            runtime,
            sse_tx,
        }
    }

    const EXIT_PENDING_CLEANUP_INTERVAL_MS: u64 = 60_000;
    const EXIT_PENDING_STALE_MS: u64 = 300_000;
}

impl Tpsl2StrategyService {
    pub fn spawn_background_tasks(&self) {
        let cleanup_repo = self.position_repo.clone();
        let paper_repo = self.paper_repo.clone();
        let runtime = self.runtime.clone();
        let pool = self.pool.clone();
        tokio::spawn(async move {
            let stale = Duration::from_millis(Tpsl2StrategyService::EXIT_PENDING_STALE_MS);
            let mut interval = tokio::time::interval(Duration::from_millis(
                Tpsl2StrategyService::EXIT_PENDING_CLEANUP_INTERVAL_MS,
            ));
            loop {
                interval.tick().await;
                let mut failed_total: u64 = 0;
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

        let handler = TPSL2StrategyHandler::new(rules);
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
                    "TPSL2".to_string(),
                    rule_id,
                    rule.buy_amount,
                );
                position.token_program_id = token.token_program_id.clone();

                let insert_res = match paper_run_id {
                    Some(run_id) => self.paper_repo.insert(&position, run_id).await,
                    None => self.position_repo.insert(&position).await,
                };
                if let Err(err) = insert_res {
                    warn!("Failed to create position for token {mint}: {err}");
                    return;
                }
                self.runtime.sync_position(None, &position);

                info!(
                    "Created position {} for token {mint} under rule {rule_id}",
                    position.id
                );

                if rule.trade_mode == "paper" {
                    super::execution::paper::spawn_entry_fill_poll(
                        self.trade_repo.clone(),
                        self.paper_repo.clone(),
                        self.runtime.clone(),
                        mint.clone(),
                        position.id,
                        rule.buy_amount,
                    );
                } else if rule.trade_mode == "real" {
                    let trader = self.trader.clone();
                    let mint_clone = mint.clone();
                    let creator = token.creator_wallet.clone();
                    let buy_amount = rule.buy_amount;
                    let position_id = position.id;
                    let position_repo = self.position_repo.clone();
                    let trade_repo = self.trade_repo.clone();
                    let runtime = self.runtime.clone();
                    let token_program_id = position
                        .token_program_id
                        .clone()
                        .unwrap_or_else(|| crate::config::constants::TOKEN_PROGRAM_ID.to_string());
                    tokio::spawn(async move {
                        let mint_for_log = mint_clone.clone();
                        super::execution::real::buy_until_filled_or_give_up(
                            trader,
                            mint_clone,
                            creator,
                            token_program_id,
                            buy_amount,
                            position_id,
                            position_repo.clone(),
                            trade_repo.clone(),
                            runtime.clone(),
                            super::execution::real::BuyRetryCfg::production(),
                        )
                        .await;
                        if let Ok(Some(pos)) = position_repo.find_by_id(position_id).await {
                            if pos.entry_price == 0.0 {
                                let _ = position_repo.delete_position(position_id).await;
                                runtime.remove_position(&pos);
                                info!(
                                    "[REAL] Removed position {} for mint {}: buy not found",
                                    position_id, mint_for_log
                                );
                            }
                        }
                    });
                }
            }
        } else {
            debug!("Token {mint} does not match any TPSL buy entry rule");
        }
    }

    pub async fn on_trade_executed(&self, mint: &str, cache: &TokenCache) {
        let mint = mint.to_string();
        // Snapshot the latest price and the in-memory trade history once. The
        // exit ladder (fixed TP/SL + E1–E4) walks these post-entry trades per
        // position via `exit::should_position_exit_on_trade`; `current_price` is still
        // the reference price for the resulting paper/real fill.
        let (current_price, trades) = match cache.get(&mint) {
            Some(entry) => {
                let state = entry.value();
                (state.current_price.unwrap_or(0.0), state.trades.clone())
            }
            None => return,
        };

        let positions = self.runtime.holding_by_mint(&mint);
        if positions.is_empty() {
            return;
        }

        for mut position in positions {
            if let Some(rule) = self.runtime.rule_by_id(position.rule_id) {
                if let Some(exit_reason) =
                    super::exit::should_position_exit_on_trade(&position, &trades, &rule)
                {
                    debug!(
                        "Position {} for token {mint} triggered exit: {:?}",
                        position.id, exit_reason
                    );

                    if rule.trade_mode == "paper" {
                        let prev = position.clone();
                        position.mark_exit_pending();
                        if let Err(err) = self.paper_repo.update(&position).await {
                            warn!(
                                "Failed to mark position {} as ExitPending: {err}",
                                position.id
                            );
                        } else {
                            self.runtime.sync_position(Some(&prev), &position);
                            info!(position_id = %position.id, mint = %mint,
                                "[PAPER] Position marked ExitPending");
                        }
                        super::execution::paper::spawn_exit_fill_poll(
                            self.trade_repo.clone(),
                            self.paper_repo.clone(),
                            self.runtime.clone(),
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
                            cache,
                            current_price,
                            Utc::now(),
                            exit_reason.to_string(),
                        )
                        .await;
                    }
                }
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

        for position in self.runtime.all_holding_positions() {
            let Some(rule) = self.runtime.rule_by_id(position.rule_id) else {
                continue;
            };
            // Cheap short-circuit: nothing to do for rules with no time exit.
            if none_if_zero_u64(rule.p_time_stop_secs).is_none()
                && none_if_zero_u64(rule.p_stall_secs).is_none()
            {
                continue;
            }

            // Trades drive the Stall derivation; last price is the paper fill mark.
            // A token absent from the cache (e.g. evicted) can still TimeStop from
            // entry_time alone — fall back to an empty history and the entry price.
            let (trades, last_price) = match cache.get(&position.mint) {
                Some(entry) => {
                    let state = entry.value();
                    (
                        state.trades.clone(),
                        state.current_price.unwrap_or(position.entry_price),
                    )
                }
                None => (Vec::new(), position.entry_price),
            };

            let Some(exit_reason) =
                super::exit::should_position_exit_on_clock(&position, &trades, &rule, now)
            else {
                continue;
            };

            info!(
                position_id = %position.id, mint = %position.mint,
                "Time-driven exit triggered: {exit_reason}"
            );

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
                self.trigger_real_exit(position, cache, last_price, now, exit_reason.to_string())
                    .await;
            }
        }
    }

    /// Mark a real position ExitPending and run the single sell pass. Shared by
    /// the trade-driven exit (`on_trade_executed`) and the time-driven sweep.
    ///
    /// `sell_with_retries` (inside `execute_sell_for_position`) owns the retry +
    /// partial-fill loop and re-reads migration routing from the WS cache on every
    /// attempt, so a mid-exit migration self-heals to the AMM path.
    /// `execute_sell_for_position` then closes the position (on a confirmed sell)
    /// or marks it ExitFailed. `trigger_price`/`trigger_time` are the hypothetical
    /// exit recorded if the sell never confirms.
    async fn trigger_real_exit(
        &self,
        mut position: Position,
        cache: &TokenCache,
        trigger_price: f64,
        trigger_time: DateTime<Utc>,
        reason: String,
    ) {
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
        super::execution::real::sell_and_close_position(
            self.trader.clone(),
            position,
            self.position_repo.clone(),
            self.trade_repo.clone(),
            self.runtime.clone(),
            cache,
            trigger_price,
            trigger_time,
            reason,
        )
        .await;
    }
}
