//! `executor-pumpfun` — the pump.fun VENUE, and the back-compat `pump_trader`
//! façade over the split executor stack.
//!
//! This crate wraps `executor-core`'s venue-agnostic send engine (sign / send /
//! confirm / nonce / tip / retry / sim) as [`PumpFunTrader`] and adds the pump.fun
//! specifics: protocol invariants, the variant catalog, ix builders, and
//! curve+AMM pricing. Its lib is still named `pump_trader`, and it re-exports the
//! engine's public items alongside its own, so every `use pump_trader::…`
//! consumer compiles unchanged across the split.
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

pub mod alt;
pub mod catalog;
pub mod constants;
pub mod protocol;
pub mod types;
mod price;
mod trader;

// Re-export the engine's `config` + `error` modules at this crate's root so both
// the `pump_trader::config` / `pump_trader::error` consumer paths AND the
// internal `crate::config` / `crate::error` (incl. the `bail!` macro) call sites
// across the venue files keep resolving after the split.
pub use executor_core::{config, error};

// --- engine items, re-exported through the façade ---
pub use executor_core::config::{
    CacheCfg, ComputeBudgetCfg, JitoTipCfg, LimitsCfg, NonceCfg, RetryCfg, SlippageCfg, TraderConfig,
};
pub use executor_core::error::{Result, TradeError};
pub use executor_core::{
    classify_swap_revert, pump_error_name, AccountDelta, DecoStep, Engine, IxLayout, LayoutKind,
    NonceAuthCheck, SigStatus, SimOutcome, SwapDirection, SwapRetryDecision, SwapRoute, Venue,
    VenueId, AMM_BUY_SLIPPAGE_BELOW_MIN_BASE, AMM_EXCEEDED_SLIPPAGE, AMM_INVALID_POOL_V2,
    ANCHOR_CONSTRAINT_SEEDS, BONDING_CURVE_COMPLETE, CURVE_BUY_SLIPPAGE_BELOW_MIN_TOKENS_OUT,
    CURVE_MISSING_USER_VOLUME_ACCUMULATOR, CURVE_TOO_LITTLE_SOL_RECEIVED,
    CURVE_TOO_MUCH_SOL_REQUIRED,
};

// --- pump venue items ---
pub use alt::launch_alt_addresses;
#[cfg(feature = "claim")]
pub use trader::claim::{ClaimOutcome, PotStatus};
#[cfg(feature = "probe")]
pub use trader::probe::{EndpointResult, FanoutReport, SenderPinRecommendation};
pub use trader::{BundleBuyVariant, BundleLegParams, BuySignedHook, DevBuy, PumpFunTrader};
pub use types::{
    AmmPoolFacts, BuyRouting, CreateTokenArgs, CreateTokenV2Args, TokenBalance, TokenProgram,
    WalletHolding,
};

// The variant catalog (SSOT for on-chain instruction selection) — the orchestrator
// providers draw legal variants from here.
pub use catalog::{
    is_valid, spec, valid_of_kind, valid_variants, Denom, Stage, VariantKind, VariantSpec,
};
