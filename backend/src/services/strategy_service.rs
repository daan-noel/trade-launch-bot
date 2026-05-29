use std::sync::Arc;

use sqlx::PgPool;
use tokio::sync::broadcast;
use tracing::{info, warn};

use crate::{
    models::events::InternalEvent,
    services::TradingService,
    strategies::{tpsl::TpslStrategyService, StrategyHandler},
};

/// Thin orchestrator — receives events from the bus and dispatches to all registered strategies.
/// To add a new strategy: implement [`StrategyHandler`] and push an instance into `handlers`.
pub struct StrategyService {
    handlers: Vec<Arc<dyn StrategyHandler>>,
}

impl StrategyService {
    pub fn new(pool: PgPool, trading: TradingService) -> Self {
        let handlers: Vec<Arc<dyn StrategyHandler>> =
            vec![Arc::new(TpslStrategyService::new(pool, trading))];
        Self { handlers }
    }

    pub async fn run(self, mut event_rx: broadcast::Receiver<InternalEvent>) {
        info!("StrategyService: starting ({} handler(s))", self.handlers.len());

        for h in &self.handlers {
            h.spawn_background_tasks();
        }

        loop {
            match event_rx.recv().await {
                Ok(InternalEvent::TokenCreated(e)) => {
                    for h in &self.handlers {
                        h.on_token_created(&e).await;
                    }
                }
                Ok(InternalEvent::TradeExecuted(e)) => {
                    for h in &self.handlers {
                        h.on_trade_executed(&e).await;
                    }
                }
                Ok(_) => {}
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
