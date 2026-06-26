//! Live-trading strategies (deploy crate). The backtest/sweep harness
//! (`backtest.rs`, `sim_progress.rs`) is local-only and stays in `backend`
//! (later `backend-local`); only the live event/timer path moves here.

pub mod analysis;
pub mod runner;
pub mod tpsl_sniper_1;
pub mod tpsl_sniper_2;

pub use runner::StrategyRunner;
