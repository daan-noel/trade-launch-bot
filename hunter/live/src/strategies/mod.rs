//! Live-trading strategies (deploy crate). Only the live event/timer path lives
//! here — the backtest/sweep harness is local-only (`lab`).
//!
//! The trading-free **decision** layer (`analysis`, and each strategy's
//! `entry`/`exit`/`util`) lives in `trading_core::strategies`; it's re-exported
//! here so `crate::strategies::analysis::…` paths keep resolving. The live crate
//! now holds only the generic fingerprint+metrics [`engine`] — the legacy
//! registry-dispatched orchestration (`execution` / `service` / `runner`) and the
//! hand-cloned `tpsl_sniper_{1,2}` live modules were retired in Phase 7.
pub use trading_core::strategies::analysis;

pub mod engine;
