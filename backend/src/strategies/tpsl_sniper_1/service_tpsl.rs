#![allow(dead_code)]
use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;
use tokio::time::sleep;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::models::Position;
use crate::state::token_cache::TokenCache;
use crate::storage::repositories::{position_repo::PositionRepo, trade_repo::TradeRepo};
use super::{TPSLStrategyHandler, TpslRuntimeCache};
use crate::trader::PumpFunTrader;
use super::util::ignore_zero_u64;

pub struct TpslStrategyService {
    pool: PgPool,
    position_repo: PositionRepo,
    trade_repo: TradeRepo,
    trader: Arc<PumpFunTrader>,
    runtime: Arc<TpslRuntimeCache>,
}

impl TpslStrategyService {
    pub fn new(pool: PgPool, trader: Arc<PumpFunTrader>, runtime: Arc<TpslRuntimeCache>) -> Self {
        Self {
            position_repo: PositionRepo::new(pool.clone()),
            trade_repo: TradeRepo::new(pool.clone()),
            pool,
            trader,
            runtime,
        }
    }

    const BUY_MAX_ATTEMPTS: usize = 3;
    const BUY_POLL_MAX_ATTEMPTS: usize = 12;
    const BUY_POLL_INTERVAL_MS: u64 = 1_000;
    const SELL_MAX_ATTEMPTS: usize = 6;
    const SELL_POLL_MAX_ATTEMPTS: usize = 10;
    const SELL_POLL_INTERVAL_MS: u64 = 500;
    const PARTIAL_FILL_THRESHOLD: f64 = 0.0001;
    const EXIT_PENDING_CLEANUP_INTERVAL_MS: u64 = 60_000;
    const EXIT_PENDING_STALE_MS: u64 = 300_000;
}

impl TpslStrategyService {
    pub fn spawn_background_tasks(&self) {
        let cleanup_repo = self.position_repo.clone();
        let runtime = self.runtime.clone();
        let pool = self.pool.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(
                TpslStrategyService::EXIT_PENDING_CLEANUP_INTERVAL_MS,
            ));
            loop {
                interval.tick().await;
                match cleanup_repo
                    .reopen_stale_exit_pending(Duration::from_millis(
                        TpslStrategyService::EXIT_PENDING_STALE_MS,
                    ))
                    .await
                {
                    Ok(reopened) => {
                        if reopened > 0 {
                            info!("Reopened {reopened} stale ExitPending positions");
                            if let Err(err) = runtime.reload_holding(&pool).await {
                                warn!("Failed to refresh TPSL holding cache after cleanup: {err}");
                            }
                        }
                    }
                    Err(err) => {
                        warn!("Failed to clean stale ExitPending positions: {err}");
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

        let handler = TPSLStrategyHandler::new(rules);
        if let Some(rule_id) = handler.check_buy_entry(&token) {
            info!("Token {mint} matches TPSL buy entry rule {rule_id}");

            if let Some(rule) = handler.get_rule(rule_id) {
                let max_concurrent_tokens = ignore_zero_u64(rule.p_max_concurrent_tokens).map(|v| v as usize);
                let max_total_tokens =
                    ignore_zero_u64(rule.p_max_total_tokens).map(|v| v as usize);

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
                    "TPSL".to_string(),
                    rule_id,
                    rule.buy_amount,
                );
                position.token_program_id = token.token_program_id.clone();

                if let Err(err) = self.position_repo.insert(&position).await {
                    warn!("Failed to create position for token {mint}: {err}");
                    return;
                }
                self.runtime.sync_position(None, &position);

                info!(
                    "Created position {} for token {mint} under rule {rule_id}",
                    position.id
                );

                if rule.trade_mode == "paper" {
                    let trade_repo = self.trade_repo.clone();
                    let mint_for_trades = mint.clone();
                    let position_repo = self.position_repo.clone();
                    let runtime = self.runtime.clone();
                    let position_id = position.id;
                    let buy_amount = rule.buy_amount;
                    tokio::spawn(async move {
                        sleep(Duration::from_secs(1)).await;
                        if let Ok(trades) = trade_repo.find_by_mint_all(&mint_for_trades).await {
                            if let Some((entry_price, entry_tx, entry_time)) =
                                super::simulation_tpsl::find_entry(&trades, 5)
                            {
                                if let Ok(Some(prev)) = position_repo.find_by_id(position_id).await {
                                    let _ = position_repo
                                        .update_entry(position_id, &entry_tx, buy_amount, entry_price, entry_time)
                                        .await;
                                    if let Ok(Some(current)) = position_repo.find_by_id(position_id).await {
                                        runtime.sync_position(Some(&prev), &current);
                                    }
                                }
                                info!(
                                    "[PAPER] Set entry for position {}: {} (tx: {})",
                                    position_id, entry_price, entry_tx
                                );
                            } else {
                                info!(
                                    "[PAPER] No entry price found for position {} (mint {})",
                                    position_id, mint_for_trades
                                );
                            }
                        }
                    });
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
                        buy_with_retries(
                            trader,
                            mint_clone,
                            creator,
                            token_program_id,
                            buy_amount,
                            position_id,
                            position_repo.clone(),
                            trade_repo.clone(),
                            runtime.clone(),
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
        let current_price = match cache.get(&mint) {
            Some(entry) => entry.value().current_price.unwrap_or(0.0),
            None => return,
        };

        let positions = self.runtime.holding_by_mint(&mint);
        if positions.is_empty() {
            return;
        }

        let handler = TPSLStrategyHandler::new(self.runtime.all_rules_vec());

        for mut position in positions {
            if let Some(rule) = handler.get_rule(position.rule_id) {
                if let Some(exit_reason) = handler.check_exit(&position, current_price, rule) {
                    debug!(
                        "Position {} for token {mint} triggered exit: {:?}",
                        position.id, exit_reason
                    );

                    if rule.trade_mode == "paper" {
                        let prev = position.clone();
                        position.mark_exit_pending();
                        if let Err(err) = self.position_repo.update(&position).await {
                            warn!(
                                "Failed to mark position {} as ExitPending: {err}",
                                position.id
                            );
                        } else {
                            self.runtime.sync_position(Some(&prev), &position);
                            info!(position_id = %position.id, mint = %mint,
                                "[PAPER] Position marked ExitPending");
                        }
                        let trade_repo = self.trade_repo.clone();
                        let mint_for_trades = mint.clone();
                        let position_repo = self.position_repo.clone();
                        let runtime = self.runtime.clone();
                        let position_id = position.id;
                        let entry_tx = position.entry_tx.clone();
                        let entry_price = position.entry_price;
                        let take_profit = rule.take_profit;
                        let stop_loss = rule.stop_loss;
                        tokio::spawn(async move {
                            let mut found = false;
                            let start = std::time::Instant::now();
                            let entry_time = chrono::Utc::now();
                            while start.elapsed() < Duration::from_secs(10) {
                                if let Ok(trades) =
                                    trade_repo.find_by_mint_all(&mint_for_trades).await
                                {
                                    let entry_block_time = trades
                                        .iter()
                                        .find(|t| t.tx_signature == entry_tx)
                                        .map(|t| t.block_time)
                                        .unwrap_or(entry_time);
                                    if let Some((exit_price, exit_tx, exit_time, reason)) =
                                        super::simulation_tpsl::find_exit(
                                            &trades,
                                            entry_block_time,
                                            entry_price,
                                            take_profit,
                                            stop_loss,
                                        )
                                    {
                                        if let Ok(Some(prev)) = position_repo.find_by_id(position_id).await {
                                            let _ = position_repo
                                                .update_exit(position_id, &exit_tx, exit_price, exit_time)
                                                .await;
                                            if let Ok(Some(current)) =
                                                position_repo.find_by_id(position_id).await
                                            {
                                                runtime.sync_position(Some(&prev), &current);
                                            }
                                        }
                                        info!(
                                            "[PAPER] Set exit for position {}: {} (tx: {}, reason: {})",
                                            position_id, exit_price, exit_tx, reason
                                        );
                                        found = true;
                                        break;
                                    }
                                }
                                sleep(Duration::from_millis(500)).await;
                            }
                            if !found {
                                if let Ok(Some(prev)) = position_repo.find_by_id(position_id).await {
                                    let _ = position_repo.revert_exit_pending(position_id).await;
                                    if let Ok(Some(current)) = position_repo.find_by_id(position_id).await {
                                        runtime.sync_position(Some(&prev), &current);
                                    }
                                }
                                info!(
                                    "[PAPER] ExitPending timed out, reverted to Holding for position {}",
                                    position_id
                                );
                            }
                        });
                    } else if rule.trade_mode == "real" {
                        let prev = position.clone();
                        position.mark_exit_pending();
                        if let Err(err) = self.position_repo.update(&position).await {
                            warn!(
                                "Failed to mark position {} as ExitPending: {err}",
                                position.id
                            );
                        } else {
                            self.runtime.sync_position(Some(&prev), &position);
                            info!(position_id = %position.id, mint = %mint,
                                "[REAL] Position marked ExitPending");
                        }
                        let trader = self.trader.clone();
                        let position_repo = self.position_repo.clone();
                        let trade_repo = self.trade_repo.clone();
                        let runtime = self.runtime.clone();
                        let mut retries = 0;
                        let max_retries = 10;
                        let mut found = false;
                        while retries < max_retries {
                            // Re-read routing from the WS-fed cache every attempt:
                            // is_cashback gates the bonding-curve cashback account;
                            // is_migrated selects the PumpSwap AMM path. A held token
                            // can migrate mid-exit and the cache flips is_migrated
                            // within ~a slot — reading it once would pin all retries
                            // to the stale bonding-curve route, so the sell would
                            // keep failing until a later trade event re-triggered it.
                            let (is_cashback, is_migrated) = match cache.get(&mint) {
                                Some(e) => (e.token.is_cashback_enabled, e.is_migrated),
                                None => (false, false),
                            };
                            execute_sell_for_position(
                                trader.clone(),
                                position.clone(),
                                position_repo.clone(),
                                trade_repo.clone(),
                                runtime.clone(),
                                is_cashback,
                                is_migrated,
                            )
                            .await;
                            if let Ok(pos) = position_repo.find_by_id(position.id).await {
                                if let Some(pos) = pos {
                                    if pos.exit_price.is_some() {
                                        found = true;
                                        break;
                                    }
                                }
                            }
                            retries += 1;
                            sleep(Duration::from_secs(1)).await;
                        }
                        if !found {
                            info!(
                                "[REAL] Sell not confirmed after {} retries for position {}",
                                max_retries, position.id
                            );
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Trading helpers — used only by TPSL for now
// ---------------------------------------------------------------------------

async fn buy_with_retries(
    trader: Arc<PumpFunTrader>,
    mint: String,
    creator: String,
    token_program_id: String,
    buy_amount: f64,
    position_id: Uuid,
    position_repo: PositionRepo,
    trade_repo: TradeRepo,
    runtime: Arc<TpslRuntimeCache>,
) {
    let mut attempt = 0usize;
    let max_attempts = TpslStrategyService::BUY_MAX_ATTEMPTS;
    let mut backoff_ms = 500u64;

    while attempt < max_attempts {
        attempt += 1;
        // Snipe path: this token was just seen via the create event, so the
        // wallet provably holds no account for it yet — skip the ATA-existence
        // RPC and go straight to the seed-account buy.
        match trader
            .buy_token_snipe(&mint, &creator, &token_program_id, buy_amount, None)
            .await
        {
            Ok(true) => {
                info!(mint = %mint, attempt, "buy succeeded");
                let wallet = trader.wallet_pubkey();
                let mut poll_attempts = 0usize;
                while poll_attempts < TpslStrategyService::BUY_POLL_MAX_ATTEMPTS {
                    poll_attempts += 1;
                    match trade_repo.find_by_mint(&mint, 20).await {
                        Ok(trades) => {
                            if let Some(t) = trades.into_iter().find(|t| {
                                t.wallet_address == wallet
                                    && t.trade_type == crate::models::trade::TradeType::Buy
                            }) {
                                if let Ok(Some(prev)) = position_repo.find_by_id(position_id).await {
                                    if let Err(err) = position_repo
                                        .update_entry(
                                            position_id,
                                            &t.tx_signature,
                                            t.token_amount,
                                            t.price_per_token,
                                            t.block_time,
                                        )
                                        .await
                                    {
                                        warn!("Failed to update position entry after buy confirmation: {err}");
                                    } else if let Ok(Some(current)) =
                                        position_repo.find_by_id(position_id).await
                                    {
                                        runtime.sync_position(Some(&prev), &current);
                                        info!(mint = %mint, tx = %t.tx_signature,
                                            "Position updated with buy confirmation");
                                    }
                                }
                                return;
                            }
                        }
                        Err(err) => {
                            warn!("Failed to query trades for confirmation: {err}");
                        }
                    }
                    sleep(Duration::from_millis(TpslStrategyService::BUY_POLL_INTERVAL_MS)).await;
                }
                warn!(mint = %mint, "Timed out waiting for buy confirmation events");
                return;
            }
            Ok(false) => warn!(mint = %mint, attempt, "buy returned false (no-op)"),
            Err(err) => warn!(mint = %mint, attempt, "buy error: {err}"),
        }

        if attempt < max_attempts {
            sleep(Duration::from_millis(backoff_ms)).await;
            backoff_ms = (backoff_ms * 2).saturating_add(100);
        } else {
            warn!(mint = %mint, "buy failed after {max_attempts} attempts");
        }
    }
}

async fn execute_sell_for_position(
    trader: Arc<PumpFunTrader>,
    mut position: Position,
    position_repo: PositionRepo,
    trade_repo: TradeRepo,
    runtime: Arc<TpslRuntimeCache>,
    is_cashback: bool,
    is_migrated: bool,
) {
    let mint = position.mint.clone();
    let wallet = trader.wallet_pubkey();
    let amount = position.entry_amount as u64;
    let base_token_program = position
        .token_program_id
        .clone()
        .unwrap_or_else(|| crate::config::constants::TOKEN_PROGRAM_ID.to_string());

    info!(
        position_id = %position.id, mint = %mint, amount,
        "Executing sell for exited position"
    );

    let completed = sell_with_retries(
        trader.clone(),
        mint.clone(),
        amount,
        trade_repo.clone(),
        is_cashback,
        is_migrated,
        base_token_program,
    )
    .await;
    if !completed {
        warn!(
            position_id = %position.id, mint = %mint,
            "Sell execution finished without clearing token balance; reverting position to Holding"
        );
        let prev = position.clone();
        position.reopen();
        if let Err(err) = position_repo.update(&position).await {
            warn!(
                position_id = %position.id, mint = %mint,
                "Failed to revert position {} to Holding: {err}", position.id
            );
        } else {
            runtime.sync_position(Some(&prev), &position);
        }
        return;
    }

    if let Ok(trades) = trade_repo.find_by_mint(&mint, 20).await {
        if let Some(last_sell) = trades.into_iter().find(|t| {
            t.wallet_address == wallet && t.trade_type == crate::models::trade::TradeType::Sell
        }) {
            let remaining = trade_repo
                .net_token_amount_by_wallet_and_mint(&wallet, &mint)
                .await
                .unwrap_or(0.0)
                .max(0.0);
            if remaining <= TpslStrategyService::PARTIAL_FILL_THRESHOLD {
                let exit_amount = last_sell.token_amount;
                position.close(
                    last_sell.price_per_token,
                    last_sell.tx_signature.clone(),
                    exit_amount,
                    last_sell.block_time,
                );
                let prev = position.clone();
                if let Err(err) = position_repo.update(&position).await {
                    warn!(
                        position_id = %position.id, mint = %mint,
                        "Failed to close position after confirmed sell: {err}"
                    );
                } else {
                    runtime.sync_position(Some(&prev), &position);
                    let pnl_percent = position.pnl_percentage().unwrap_or(0.0);
                    info!(
                        position_id = %position.id, mint = %mint,
                        tx = %last_sell.tx_signature, pnl_percent,
                        "Position closed after confirmed sell"
                    );
                }
                return;
            }
        }
    }

    warn!(
        position_id = %position.id, mint = %mint,
        "Sell completed but no confirmed sell record found, or token balance remained"
    );
}

async fn sell_with_retries(
    trader: Arc<PumpFunTrader>,
    mint: String,
    mut amount: u64,
    trade_repo: TradeRepo,
    is_cashback: bool,
    is_migrated: bool,
    base_token_program: String,
) -> bool {
    let mut attempt = 0usize;
    let max_attempts = TpslStrategyService::SELL_MAX_ATTEMPTS;
    let mut backoff_ms = 300u64;
    let wallet = trader.wallet_pubkey();

    if amount == 0 {
        info!(mint = %mint, "sell skipped because amount is zero");
        return true;
    }

    // Resolve the token account once (cache-first; at most one wallet scan) and
    // reuse it across every attempt — it never changes for a given mint. If this
    // is None, sell_token/amm_sell still fall back to their own internal lookup,
    // so correctness is preserved while the per-attempt wallet scan is removed.
    let token_account_override = trader
        .resolve_cached_token_account(&mint)
        .await
        .ok()
        .flatten()
        .map(|pk| pk.to_string());

    while attempt < max_attempts && amount > 0 {
        attempt += 1;
        let sell_result = if is_migrated {
            trader
                .amm_sell(
                    &mint,
                    amount,
                    &base_token_program,
                    None,
                    token_account_override.as_deref(),
                    None,
                )
                .await
        } else {
            trader
                .sell_token(&mint, amount, None, is_cashback, token_account_override.as_deref(), None)
                .await
        };
        match sell_result {
            Ok(true) => {
                info!(mint = %mint, attempt, amount, "sell submitted");
                let mut poll_attempts = 0usize;
                while poll_attempts < TpslStrategyService::SELL_POLL_MAX_ATTEMPTS {
                    poll_attempts += 1;
                    match trade_repo
                        .net_token_amount_by_wallet_and_mint(&wallet, &mint)
                        .await
                    {
                        Ok(balance) => {
                            let remaining = balance.max(0.0);
                            if remaining <= TpslStrategyService::PARTIAL_FILL_THRESHOLD {
                                info!(mint = %mint, attempt, amount,
                                    "sell completed, no remaining balance");
                                return true;
                            }
                            warn!(mint = %mint, attempt, remaining,
                                "partial fill detected, retrying remaining amount");
                            amount = remaining as u64;
                            break;
                        }
                        Err(err) => warn!("Failed to query net token balance: {err}"),
                    }
                    sleep(Duration::from_millis(TpslStrategyService::SELL_POLL_INTERVAL_MS)).await;
                }

                if poll_attempts >= TpslStrategyService::SELL_POLL_MAX_ATTEMPTS {
                    warn!(
                        mint = %mint,
                        "Timed out waiting for sell confirmations; remaining amount: {}", amount
                    );
                    return false;
                }
            }
            Ok(false) => warn!(mint = %mint, attempt, amount, "sell returned false (no-op)"),
            Err(err) => warn!(mint = %mint, attempt, amount, "sell error: {err}"),
        }

        if attempt < max_attempts {
            sleep(Duration::from_millis(backoff_ms)).await;
            backoff_ms = (backoff_ms * 2).saturating_add(100);
        } else {
            warn!(mint = %mint, amount, "sell failed after {max_attempts} attempts");
            return false;
        }
    }

    amount == 0
}
