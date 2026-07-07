//! Standalone, reusable Pump.fun trader.
//!
//! Self-contained: depends only on third-party crates (no project glue). Supply
//! a [`TraderConfig`] and run it on a Tokio runtime. Optionally initialise a
//! `tracing` subscriber in the host application to see its `info!`/`warn!` logs.
//!
//! Three tiers of values:
//!   - [`protocol`] — compile-time invariants (program IDs, offsets, units);
//!   - [`config`]   — operational tuning ([`TraderConfig`] + `Default` sub-structs);
//!   - per-call params — buy amount / slippage / tip level (function arguments).
//!
//! Errors are a crate-owned [`TradeError`] (no `anyhow` leak across the boundary).
//!
//! ```ignore
//! let config = TraderConfig {
//!     rpc_url, helius_sender_urls, signer: Arc::new(my_signer), nonce_accounts,
//!     ..Default::default()
//! };
//! let mut trader = pump_trader::PumpFunTrader::new(Arc::new(config));
//! trader.initialize().await?;
//! trader.buy_token(&mint, &creator, token_program, sol_amount, slippage, cashback_enabled).await?;
//! ```

pub mod config;
pub mod constants;
pub mod error;
pub mod protocol;
pub mod types;
mod trader;

pub use config::{
    CacheCfg, ComputeBudgetCfg, JitoTipCfg, LimitsCfg, NonceCfg, RetryCfg, SlippageCfg, TraderConfig,
};
pub use error::{Result, TradeError};
#[cfg(feature = "claim")]
pub use trader::claim::{ClaimOutcome, PotStatus};
#[cfg(feature = "probe")]
pub use trader::probe::{EndpointResult, FanoutReport};
pub use trader::{
    classify_swap_revert, AccountDelta, BuySignedHook, NonceAuthCheck, PumpFunTrader, SigStatus,
    SimOutcome, SwapDirection, SwapRetryDecision, SwapRoute, AMM_BUY_SLIPPAGE_BELOW_MIN_BASE,
    AMM_EXCEEDED_SLIPPAGE, ANCHOR_CONSTRAINT_SEEDS, BONDING_CURVE_COMPLETE,
    CURVE_BUY_SLIPPAGE_BELOW_MIN_TOKENS_OUT, CURVE_MISSING_USER_VOLUME_ACCUMULATOR,
    CURVE_TOO_LITTLE_SOL_RECEIVED, CURVE_TOO_MUCH_SOL_REQUIRED,
};
pub use types::{BuyRouting, CreateTokenArgs, CreateTokenV2Args, TokenBalance, TokenProgram, WalletHolding};
pub use trader::BundleLegParams;
pub use trader::BundleBuyVariant;
