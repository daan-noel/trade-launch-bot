pub mod tpsl;

#[allow(unused_imports)]
pub use tpsl::{ExitReason, TPSLStrategyHandler, TpslStrategyService};

use crate::models::events::{TokenCreatedEvent, TradeExecutedEvent};

#[async_trait::async_trait]
pub trait StrategyHandler: Send + Sync {
    fn name(&self) -> &str;
    async fn on_token_created(&self, event: &TokenCreatedEvent);
    async fn on_trade_executed(&self, event: &TradeExecutedEvent);
    /// Override to spawn background tasks (cleanup loops, etc.) when the service starts.
    fn spawn_background_tasks(&self) {}
}
