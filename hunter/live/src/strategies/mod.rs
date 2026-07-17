//! Live-trading strategies (deploy crate). The backtest/sweep harness
//! (`backtest.rs`, `sim_progress.rs`) is local-only and stays in `backend`
//! (later `lab`); only the live event/timer path lives here.
//!
//! The trading-free **decision** layer (`analysis`, and each strategy's
//! `entry`/`exit`/`util`) moved to `trading_core::strategies` so the
//! local sweep/backtest path can reuse it without depending on this crate.
//! Re-exported here so existing `crate::strategies::analysis::…` /
//! `tpsl_sniper_N::{entry,exit,…}` paths keep resolving.

//! The trading-free **decision** layer (`analysis`, and each strategy's
//! `entry`/`exit`/`util`) lives in `trading_core::strategies`; the live
//! crate now holds only the unified, registry-dispatched orchestration
//! (`execution` / `service` / `runner`). The hand-cloned `tpsl_sniper_{1,2}` live
//! modules were retired in Phase 3.
pub use trading_core::strategies::analysis;

pub mod engine;
pub mod execution;
pub mod runner;
pub mod service;

pub use runner::StrategyRunner;
pub use service::{PaperActivation, StrategyService};
