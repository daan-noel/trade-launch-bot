//! `executor-core` — the venue-**agnostic** Solana send engine.
//!
//! Extracted from the former `pump-trader` god-object: this crate is the half
//! that knows nothing about pump.fun (or any launchpad). It owns the reusable
//! engine — sign · fan-out send · feed-confirm · durable-nonce · Jito tip ·
//! retry-classify · simulate harness — plus the open [`Venue`] seam.
//!
//! A venue crate (e.g. `executor-pumpfun`) wraps an [`Engine`], adds its
//! protocol/discriminators/ix-builders/pricing, and consumes the engine's
//! send/nonce/tip/confirm/sim methods via `Deref`.
//!
//! Three tiers of values (mirrors the old crate):
//!   - protocol invariants — live in the **venue** crate (program IDs, offsets);
//!   - [`config`] — operational tuning ([`TraderConfig`] + `Default` sub-structs);
//!   - per-call params — amount / slippage / tip level (function arguments).
//!
//! Errors are a crate-owned [`TradeError`] (no `anyhow` leak across the boundary).

pub mod blockhash;
pub mod config;
pub mod engine;
pub mod error;
pub mod ix_layout;
pub mod jito_tip;
pub mod nonce;
pub mod retry;
pub mod send;
pub mod sim;
pub mod venue;

/// Lamports per SOL. A universal Solana unit (not pump-specific) that the engine
/// needs for tip/amount math; the venue crate keeps its own copy of the protocol
/// constants, and the two immutable copies are guarded equal by a cross-crate
/// SSOT test in `hunter/live`.
pub const LAMPORTS_PER_SOL: u64 = 1_000_000_000;

pub use config::{
    CacheCfg, ComputeBudgetCfg, JitoTipCfg, LimitsCfg, NonceCfg, RetryCfg, SlippageCfg, TraderConfig,
};
pub use engine::{buy_lamports_checked, Engine};
pub use error::{Context, Result, TradeError};
pub use ix_layout::{DecoStep, IxLayout, LayoutKind};
pub use nonce::NonceAuthCheck;
pub use retry::{
    classify_swap_revert, pump_error_name, SwapDirection, SwapRetryDecision, SwapRoute,
    AMM_BUY_SLIPPAGE_BELOW_MIN_BASE, AMM_EXCEEDED_SLIPPAGE, AMM_INVALID_POOL_V2,
    ANCHOR_CONSTRAINT_SEEDS, BONDING_CURVE_COMPLETE, CURVE_BUY_SLIPPAGE_BELOW_MIN_TOKENS_OUT,
    CURVE_MISSING_USER_VOLUME_ACCUMULATOR, CURVE_OVERFLOW, CURVE_TOO_LITTLE_SOL_RECEIVED,
    CURVE_TOO_MUCH_SOL_REQUIRED,
};
pub use send::{SigStatus, TxAnchor};
pub use sim::{AccountDelta, SimOutcome};
pub use venue::{Venue, VenueId};
