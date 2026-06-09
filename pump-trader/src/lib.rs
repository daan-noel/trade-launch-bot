//! Standalone, reusable Pump.fun trader.
//!
//! Self-contained: depends only on third-party crates (no project glue). Supply
//! a [`TraderConfig`] and run it on a Tokio runtime. Optionally initialise a
//! `tracing` subscriber in the host application to see its `info!`/`warn!` logs.
//!
//! ```ignore
//! let trader = pump_trader::PumpFunTrader::new(config);
//! trader.initialize().await?;
//! trader.buy_token(mint, sol_amount, /* ... */).await?;
//! ```

pub mod constants;
pub mod types;
mod trader;

pub use trader::{AmmSimulation, PumpFunTrader, TraderConfig};
pub use types::{BuyRouting, TokenBalance, WalletHolding};
