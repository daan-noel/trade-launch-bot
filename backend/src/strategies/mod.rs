pub mod tpsl;

pub use tpsl::{ExitReason, TPSLStrategyHandler};

/// Base trait for all strategy handlers.
/// Future strategies (e.g., MACD, RSI, etc.) can implement this trait.
pub trait StrategyHandler {
    fn name(&self) -> &str;
    fn version(&self) -> &str;
}
