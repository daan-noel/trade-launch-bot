use sqlx::PgPool;
use tokio::sync::broadcast;
use tokio::time::sleep;
use tracing::{debug, info, warn};

use std::time::Duration;

use crate::utils::{ignore_zero_f64, ignore_zero_u64};
use crate::{
    models::{
        events::{InternalEvent, TokenCreatedEvent, TradeExecutedEvent},
        Position,
    },
    services::TradingService,
    storage::repositories::{
        position_repo::PositionRepo, strategy_tpsl_rule_repo::StrategyTPSLRuleRepo,
        token_repo::TokenRepo, trade_repo::TradeRepo,
    },
    strategies::TPSLStrategyHandler,
};

/// Strategy Service — subscribes to the internal event bus and manages strategy-based trading:
///
///  `TokenCreated`  → Check if token matches any TPSL buy entry rule
///                  → If match, create Position with status="Holding"
///  `TradeExecuted` → Check all open positions for this token
///                  → If exit condition met, execute sell and close position after confirmed sell
pub struct StrategyService {
    rule_repo: StrategyTPSLRuleRepo,
    position_repo: PositionRepo,
    token_repo: TokenRepo,
    trade_repo: TradeRepo,
    trading: TradingService,
}

impl StrategyService {
    pub fn new(pool: PgPool, trading: TradingService) -> Self {
        Self {
            rule_repo: StrategyTPSLRuleRepo::new(pool.clone()),
            position_repo: PositionRepo::new(pool.clone()),
            token_repo: TokenRepo::new(pool.clone()),
            trade_repo: TradeRepo::new(pool),
            trading,
        }
    }

    /// Retry and polling configuration for trade execution.
    const BUY_MAX_ATTEMPTS: usize = 3;
    const BUY_POLL_MAX_ATTEMPTS: usize = 12;
    const BUY_POLL_INTERVAL_MS: u64 = 1_000;
    const SELL_MAX_ATTEMPTS: usize = 6;
    const SELL_POLL_MAX_ATTEMPTS: usize = 10;
    const SELL_POLL_INTERVAL_MS: u64 = 500;
    const PARTIAL_FILL_THRESHOLD: f64 = 0.0001;

    /// Timing constants for stale ExitPending cleanup.
    const EXIT_PENDING_CLEANUP_INTERVAL_MS: u64 = 60_000;
    const EXIT_PENDING_STALE_MS: u64 = 300_000;

    /// Drive the service until the event bus is closed.
    pub async fn run(self, mut event_rx: broadcast::Receiver<InternalEvent>) {
        info!("StrategyService: starting");

        let cleanup_repo = self.position_repo.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(
                StrategyService::EXIT_PENDING_CLEANUP_INTERVAL_MS,
            ));
            loop {
                interval.tick().await;
                match cleanup_repo
                    .reopen_stale_exit_pending(Duration::from_millis(
                        StrategyService::EXIT_PENDING_STALE_MS,
                    ))
                    .await
                {
                    Ok(reopened) => {
                        if reopened > 0 {
                            info!("Reopened {reopened} stale ExitPending positions");
                        }
                    }
                    Err(err) => {
                        warn!("Failed to clean stale ExitPending positions: {err}");
                    }
                }
            }
        });

        loop {
            match event_rx.recv().await {
                Ok(InternalEvent::TokenCreated(e)) => {
                    self.handle_token_created(e).await;
                }
                Ok(InternalEvent::TradeExecuted(e)) => {
                    self.handle_trade_executed(e).await;
                }
                Ok(_) => {
                    // Ignore other event types
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!(
                        "StrategyService lagged {n} events — \
                         consider increasing the broadcast channel capacity"
                    );
                }
                Err(broadcast::error::RecvError::Closed) => {
                    info!("StrategyService: event bus closed — stopping");
                    break;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Event handlers
// ---------------------------------------------------------------------------

impl StrategyService {
    async fn handle_token_created(&self, e: TokenCreatedEvent) {
        let mint = e.token.mint_address.clone();

        // Load all active TPSL rules
        let rules = match self.rule_repo.find_active().await {
            Ok(r) => r,
            Err(err) => {
                warn!("Failed to load active TPSL rules: {err}");
                return;
            }
        };

        if rules.is_empty() {
            debug!("No active TPSL rules — skipping token {mint}");
            return;
        }

        // Check if this token matches any rule
        let handler = TPSLStrategyHandler::new(rules);
        if let Some(rule_id) = handler.check_buy_entry(&e.token) {
            info!("Token {mint} matches TPSL buy entry rule {rule_id}");

            // Find the matched rule to get buy_amount
            if let Some(rule) = handler.get_rule(rule_id) {
                let max_holding = ignore_zero_u64(rule.p_max_holding_tokens).map(|v| v as usize);
                let total_max_trade_tokens =
                    ignore_zero_u64(rule.p_total_max_trade_tokens).map(|v| v as usize);

                // Enforce max total held tokens for this rule, if configured.
                if let Some(max_holding_tokens) = max_holding {
                    match self.position_repo.count_holding_by_rule(rule_id).await {
                        Ok(current_holding) => {
                            if current_holding >= max_holding_tokens as i64 {
                                debug!(
                                    "Rule {rule_id} reached max holding tokens ({current_holding}/{max_holding_tokens}), skipping token {mint}"
                                );
                                return;
                            }
                        }
                        Err(err) => {
                            warn!("Failed to count holding positions for rule {rule_id}: {err}");
                            return;
                        }
                    }
                }

                if let Some(total_max_trade_tokens) = total_max_trade_tokens {
                    match self.position_repo.count_by_rule(rule_id).await {
                        Ok(total_traded) => {
                            if total_traded >= total_max_trade_tokens as i64 {
                                debug!(
                                    "Rule {rule_id} reached total max trade tokens ({total_traded}/{total_max_trade_tokens}), skipping token {mint}"
                                );
                                return;
                            }
                        }
                        Err(err) => {
                            warn!("Failed to count total positions for rule {rule_id}: {err}");
                            return;
                        }
                    }
                }

                // Insert empty position
                let mut position = Position::new(
                    mint.clone(),
                    self.trading.wallet_pubkey(),
                    0.0,
                    e.tx_signature.clone(),
                    "TPSL".to_string(),
                    rule_id,
                    rule.buy_amount,
                );
                position.token_program_id = e.token.token_program_id.clone();

                if let Err(err) = self.position_repo.insert(&position).await {
                    warn!("Failed to create position for token {mint}: {err}");
                    return;
                }

                info!(
                    "Created position {} for token {mint} under rule {rule_id}",
                    position.id
                );

                if rule.trade_mode == "paper" {
                    // Wait for block complete event (simulate with async block for now)
                    let trade_repo = self.trade_repo.clone();
                    let mint_for_trades = mint.clone();
                    let position_repo = self.position_repo.clone();
                    let position_id = position.id;
                    let buy_amount = rule.buy_amount;
                    tokio::spawn(async move {
                        // TODO: Replace with actual block-complete event
                        sleep(Duration::from_secs(1)).await;
                        if let Ok(trades) = trade_repo.find_by_mint_all(&mint_for_trades).await {
                            if let Some((entry_price, entry_tx, _entry_time)) =
                                crate::strategies::tpsl::simulation_tpsl::find_entry(&trades, 5)
                            {
                                let _ = position_repo
                                    .update_entry(position_id, &entry_tx, buy_amount, entry_price)
                                    .await;
                                info!(
                                    "[PAPER TEST] Set entry price for position {}: {} (tx: {})",
                                    position_id, entry_price, entry_tx
                                );
                            } else {
                                info!(
                                    "[PAPER TEST] No entry price found for position {} (mint {})",
                                    position_id, mint_for_trades
                                );
                            }
                        }
                    });
                } else if rule.trade_mode == "real" {
                    // Real mode: wait for trade event, then send buy, then confirm in history
                    let trading = self.trading.clone();
                    let mint_clone = mint.clone();
                    let creator = e.token.creator_wallet.clone();
                    let buy_amount = rule.buy_amount;
                    let position_id = position.id;
                    let position_repo = self.position_repo.clone();
                    let trade_repo = self.trade_repo.clone();
                    let token_program_id = position
                        .token_program_id
                        .clone()
                        .unwrap_or_else(|| crate::config::constants::TOKEN_PROGRAM_ID.to_string());
                    tokio::spawn(async move {
                        let mint_for_log = mint_clone.clone();
                        buy_with_retries(
                            trading,
                            mint_clone,
                            creator,
                            token_program_id,
                            buy_amount,
                            position_id,
                            position_repo.clone(),
                            trade_repo.clone(),
                        )
                        .await;
                        // If not found, remove position
                        if let Ok(pos) = position_repo.find_by_id(position_id).await {
                            if let Some(pos) = pos {
                                if pos.entry_price == 0.0 {
                                    let _ = position_repo.delete_position(position_id).await;
                                    info!(
                                        "[REAL] Removed position {} for mint {}: buy not found",
                                        position_id, mint_for_log
                                    );
                                }
                            }
                        }
                    });
                }
            }
        } else {
            debug!("Token {mint} does not match any TPSL buy entry rule");
        }
    }

    async fn handle_trade_executed(&self, e: TradeExecutedEvent) {
        let mint = e.trade.mint_address.clone();

        // Find all holding positions for this token
        let positions = match self.position_repo.find_holding_by_mint(&mint).await {
            Ok(p) => p,
            Err(err) => {
                warn!("Failed to load positions for token {mint}: {err}");
                return;
            }
        };

        if positions.is_empty() {
            return; // No open positions for this token
        }

        // Load all active TPSL rules for exit checking
        let rules = match self.rule_repo.find_active().await {
            Ok(r) => r,
            Err(err) => {
                warn!("Failed to load active TPSL rules: {err}");
                return;
            }
        };

        let handler = TPSLStrategyHandler::new(rules);
        let current_price = e.trade.price_per_token;

        // Check each open position for exit conditions
        for mut position in positions {
            if let Some(rule) = handler.get_rule(position.rule_id) {
                if let Some(exit_reason) = handler.check_exit(&position, current_price, rule) {
                    debug!(
                        "Position {} for token {mint} triggered exit: {:?}",
                        position.id, exit_reason
                    );

                    if rule.trade_mode == "paper" {
                        // Mark ExitPending, start 10s timer for exit price
                        position.mark_exit_pending();
                        if let Err(err) = self.position_repo.update(&position).await {
                            warn!(
                                "Failed to mark position {} as ExitPending: {err}",
                                position.id
                            );
                        } else {
                            info!(position_id = %position.id, mint = %mint, "[PAPER TEST] Position marked ExitPending");
                        }
                        let trade_repo = self.trade_repo.clone();
                        let mint_for_trades = mint.clone();
                        let position_repo = self.position_repo.clone();
                        let position_id = position.id;
                        let entry_tx = position.entry_tx.clone();
                        let entry_price = position.entry_price;
                        let take_profit = rule.take_profit;
                        let stop_loss = rule.stop_loss;
                        tokio::spawn(async move {
                            let mut found = false;
                            let start = std::time::Instant::now();
                            let entry_time = chrono::Utc::now(); // fallback if not available
                            while start.elapsed() < Duration::from_secs(10) {
                                if let Ok(trades) =
                                    trade_repo.find_by_mint_all(&mint_for_trades).await
                                {
                                    // Try to find the entry trade's block_time
                                    let entry_block_time = trades
                                        .iter()
                                        .find(|t| t.tx_signature == entry_tx)
                                        .map(|t| t.block_time)
                                        .unwrap_or(entry_time);
                                    if let Some((exit_price, exit_tx, _exit_time, reason)) =
                                        crate::strategies::tpsl::simulation_tpsl::find_exit(
                                            &trades,
                                            entry_block_time,
                                            entry_price,
                                            take_profit,
                                            stop_loss,
                                        )
                                    {
                                        let _ = position_repo
                                            .update_exit(position_id, &exit_tx, exit_price)
                                            .await;
                                        info!("[PAPER TEST] Set exit price for position {}: {} (tx: {}, reason: {})", position_id, exit_price, exit_tx, reason);
                                        found = true;
                                        break;
                                    }
                                }
                                sleep(Duration::from_millis(500)).await;
                            }
                            if !found {
                                // Timeout: revert ExitPending to Holding
                                let _ = position_repo.revert_exit_pending(position_id).await;
                                info!("[PAPER TEST] ExitPending timed out, reverted to Holding for position {}", position_id);
                            }
                        });
                    } else if rule.trade_mode == "real" {
                        // Real mode: send sell, retry up to 10 times if not found
                        position.mark_exit_pending();
                        if let Err(err) = self.position_repo.update(&position).await {
                            warn!(
                                "Failed to mark position {} as ExitPending: {err}",
                                position.id
                            );
                        } else {
                            info!(position_id = %position.id, mint = %mint, "[REAL] Position marked ExitPending");
                        }
                        let trading = self.trading.clone();
                        let position_repo = self.position_repo.clone();
                        let trade_repo = self.trade_repo.clone();
                        let mut retries = 0;
                        let max_retries = 10;
                        let mut found = false;
                        while retries < max_retries {
                            execute_sell_for_position(
                                trading.clone(),
                                position.clone(),
                                current_price,
                                position_repo.clone(),
                                trade_repo.clone(),
                            )
                            .await;
                            // Check if sell succeeded
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

/// Attempt a buy with a small retry/backoff strategy and poll for trade confirmation.
async fn buy_with_retries(
    trading: TradingService,
    mint: String,
    creator: String,
    token_program_id: String,
    buy_amount: f64,
    position_id: uuid::Uuid,
    position_repo: PositionRepo,
    trade_repo: TradeRepo,
) {
    let mut attempt = 0usize;
    let max_attempts = StrategyService::BUY_MAX_ATTEMPTS;
    let mut backoff_ms = 500u64;

    while attempt < max_attempts {
        attempt += 1;
        match trading
            .buy_token(&mint, &creator, &token_program_id, buy_amount)
            .await
        {
            Ok(true) => {
                info!(mint = %mint, attempt, "buy succeeded");

                // Poll trades for confirmation that our wallet executed a buy for this mint.
                let wallet = trading.wallet_pubkey();
                let mut poll_attempts = 0usize;
                while poll_attempts < StrategyService::BUY_POLL_MAX_ATTEMPTS {
                    poll_attempts += 1;
                    match trade_repo.find_by_mint(&mint, 20).await {
                        Ok(trades) => {
                            if let Some(t) = trades.into_iter().find(|t| {
                                t.wallet_address == wallet
                                    && t.trade_type == crate::models::trade::TradeType::Buy
                            }) {
                                if let Err(err) = position_repo
                                    .update_entry(
                                        position_id,
                                        &t.tx_signature,
                                        t.token_amount,
                                        t.price_per_token,
                                    )
                                    .await
                                {
                                    warn!("Failed to update position entry after buy confirmation: {err}");
                                } else {
                                    info!(mint = %mint, tx = %t.tx_signature, "Position updated with buy confirmation");
                                }
                                return;
                            }
                        }
                        Err(err) => {
                            warn!("Failed to query trades for confirmation: {err}");
                        }
                    }

                    sleep(Duration::from_millis(StrategyService::BUY_POLL_INTERVAL_MS)).await;
                }

                warn!(mint = %mint, "Timed out waiting for buy confirmation events");
                return;
            }
            Ok(false) => {
                warn!(mint = %mint, attempt, "buy returned false (no-op)");
            }
            Err(err) => {
                warn!(mint = %mint, attempt, "buy error: {err}");
            }
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
    trading: TradingService,
    mut position: Position,
    current_price: f64,
    position_repo: PositionRepo,
    trade_repo: TradeRepo,
) {
    let mint = position.mint.clone();
    let wallet = trading.wallet_pubkey();
    let amount = position.entry_amount as u64;

    info!(
        position_id = %position.id,
        mint = %mint,
        amount,
        "Executing sell for exited position"
    );

    let completed =
        sell_with_retries(trading.clone(), mint.clone(), amount, trade_repo.clone()).await;
    if !completed {
        warn!(
            position_id = %position.id,
            mint = %mint,
            "Sell execution finished without clearing token balance; reverting position to Holding"
        );
        position.reopen();
        if let Err(err) = position_repo.update(&position).await {
            warn!(
                position_id = %position.id,
                mint = %mint,
                "Failed to revert position {} to Holding: {err}",
                position.id
            );
        }
        return;
    }

    // After a successful sell, confirm the sell trade and close the position.
    if let Ok(trades) = trade_repo.find_by_mint(&mint, 20).await {
        if let Some(last_sell) = trades.into_iter().find(|t| {
            t.wallet_address == wallet && t.trade_type == crate::models::trade::TradeType::Sell
        }) {
            let remaining = trade_repo
                .net_token_amount_by_wallet_and_mint(&wallet, &mint)
                .await
                .unwrap_or(0.0)
                .max(0.0);
            if remaining <= StrategyService::PARTIAL_FILL_THRESHOLD {
                let exit_amount = last_sell.token_amount;
                position.close(
                    last_sell.price_per_token,
                    last_sell.tx_signature.clone(),
                    exit_amount,
                );
                if let Err(err) = position_repo.update(&position).await {
                    warn!(
                        position_id = %position.id,
                        mint = %mint,
                        "Failed to close position after confirmed sell: {err}"
                    );
                } else {
                    let pnl_percent = position.pnl_percentage().unwrap_or(0.0);
                    info!(
                        position_id = %position.id,
                        mint = %mint,
                        tx = %last_sell.tx_signature,
                        pnl_percent,
                        "Position closed after confirmed sell"
                    );
                }
                return;
            }
        }
    }

    warn!(
        position_id = %position.id,
        mint = %mint,
        "Sell completed but no confirmed sell record was found, or token balance remained after sell"
    );
}

/// Attempt a sell with retries. If the sell may partially fill, this will retry
/// a few times to attempt selling the remaining quantity.
async fn sell_with_retries(
    trading: TradingService,
    mint: String,
    mut amount: u64,
    trade_repo: TradeRepo,
) -> bool {
    let mut attempt = 0usize;
    let max_attempts = StrategyService::SELL_MAX_ATTEMPTS;
    let mut backoff_ms = 300u64;

    let wallet = trading.wallet_pubkey();

    if amount == 0 {
        info!(mint = %mint, "sell skipped because amount is zero");
        return true;
    }

    while attempt < max_attempts && amount > 0 {
        attempt += 1;
        match trading.sell_token(&mint, amount, None, false).await {
            Ok(true) => {
                info!(mint = %mint, attempt, amount, "sell submitted");

                // Poll the trade repo to determine if any remaining tokens exist for our wallet.
                let mut poll_attempts = 0usize;
                while poll_attempts < StrategyService::SELL_POLL_MAX_ATTEMPTS {
                    poll_attempts += 1;
                    match trade_repo
                        .net_token_amount_by_wallet_and_mint(&wallet, &mint)
                        .await
                    {
                        Ok(balance) => {
                            let remaining = balance.max(0.0);
                            if remaining <= StrategyService::PARTIAL_FILL_THRESHOLD {
                                info!(mint = %mint, attempt, amount, "sell completed, no remaining balance");
                                return true;
                            }
                            warn!(mint = %mint, attempt, remaining, "partial fill detected, retrying remaining amount");
                            amount = remaining as u64;
                            break; // break polling loop and retry sell of remaining amount
                        }
                        Err(err) => {
                            warn!("Failed to query net token balance: {err}");
                        }
                    }

                    sleep(Duration::from_millis(
                        StrategyService::SELL_POLL_INTERVAL_MS,
                    ))
                    .await;
                }

                if poll_attempts >= StrategyService::SELL_POLL_MAX_ATTEMPTS {
                    warn!(mint = %mint, "Timed out waiting for sell confirmations; remaining amount: {}", amount);
                    return false;
                }
            }
            Ok(false) => {
                warn!(mint = %mint, attempt, amount, "sell returned false (no-op)");
            }
            Err(err) => {
                warn!(mint = %mint, attempt, amount, "sell error: {err}");
            }
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
