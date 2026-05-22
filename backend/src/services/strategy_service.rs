use sqlx::PgPool;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

use crate::{
    models::{
        events::{InternalEvent, TokenCreatedEvent, TradeExecutedEvent},
        Position,
    },
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
///                  → If exit condition met, close position with status="End"
pub struct StrategyService {
    rule_repo: StrategyTPSLRuleRepo,
    position_repo: PositionRepo,
    token_repo: TokenRepo,
    trade_repo: TradeRepo,
}

impl StrategyService {
    pub fn new(pool: PgPool) -> Self {
        Self {
            rule_repo: StrategyTPSLRuleRepo::new(pool.clone()),
            position_repo: PositionRepo::new(pool.clone()),
            token_repo: TokenRepo::new(pool.clone()),
            trade_repo: TradeRepo::new(pool),
        }
    }

    /// Drive the service until the event bus is closed.
    pub async fn run(self, mut event_rx: broadcast::Receiver<InternalEvent>) {
        info!("StrategyService: starting");

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
            debug!("Token {mint} matches TPSL buy entry rule {rule_id}");

            // Find the matched rule to get buy_amount
            if let Some(rule) = handler.get_rule(rule_id) {
                // Create a new position
                let position = Position::new(
                    mint.clone(),
                    e.token.creator_wallet.clone(),
                    0.0, // entry_price will be 0 initially; can be updated from first trade
                    e.tx_signature.clone(),
                    "TPSL".to_string(),
                    rule_id,
                    rule.buy_amount,
                );

                if let Err(err) = self.position_repo.insert(&position).await {
                    warn!("Failed to create position for token {mint}: {err}");
                } else {
                    info!(
                        "Created position {} for token {mint} under rule {rule_id}",
                        position.id
                    );
                }
            }
        } else {
            debug!("Token {mint} does not match any TPSL buy entry rule");
        }
    }

    async fn handle_trade_executed(&self, e: TradeExecutedEvent) {
        let mint = e.trade.mint_address.clone();

        // Find all holding positions for this token
        let mut positions = match self.position_repo.find_holding_by_mint(&mint).await {
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
        for position in &mut positions {
            if let Some(rule) = handler.get_rule(position.rule_id) {
                if let Some(exit_reason) = handler.check_exit(position, current_price, rule) {
                    debug!(
                        "Position {} for token {mint} triggered exit: {:?}",
                        position.id, exit_reason
                    );

                    // Close the position
                    position.close(
                        current_price,
                        e.tx_signature.clone(),
                        e.trade.token_amount, // Exit amount
                    );

                    if let Err(err) = self.position_repo.update(position).await {
                        warn!("Failed to close position {}: {err}", position.id);
                    } else {
                        let pnl_percent = position.pnl_percentage().unwrap_or(0.0);
                        info!(
                            "Closed position {} — reason: {}, PnL: {:.2}%",
                            position.id, exit_reason, pnl_percent
                        );
                    }
                }
            }
        }
    }
}
