//! Live-trading strategies (deploy crate). The backtest/sweep harness
//! (`backtest.rs`, `sim_progress.rs`) is local-only and stays in `backend`
//! (later `backend-local`); only the live event/timer path lives here.
//!
//! The trading-free **decision** layer (`analysis`, and each strategy's
//! `entry`/`exit`/`util`/`cohort`) moved to `backend_core::strategies` so the
//! local sweep/backtest path can reuse it without depending on this crate.
//! Re-exported here so existing `crate::strategies::analysis::…` /
//! `tpsl_sniper_N::{entry,exit,…}` paths keep resolving.

pub use backend_core::strategies::analysis;
pub mod runner;
pub mod tpsl_sniper_1;
pub mod tpsl_sniper_2;

pub use runner::StrategyRunner;
