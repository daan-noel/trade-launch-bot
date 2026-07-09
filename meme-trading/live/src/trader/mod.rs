//! The trader lives in the standalone `pump-trader` crate. Re-export it here so
//! the rest of the backend keeps using `crate::trader::{...}` unchanged.

pub use pump_trader::{PumpFunTrader, SigStatus, TraderConfig, WalletHolding};

mod trader_hook_impl;
pub use trader_hook_impl::TraderHookBridge;
