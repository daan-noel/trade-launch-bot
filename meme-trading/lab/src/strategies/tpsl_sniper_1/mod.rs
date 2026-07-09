//! lab `tpsl_sniper_1`: the DB-backed backtest harness over the shared
//! decision layer. Re-exports core's `entry`/`exit`/`util` so `backtest`'s
//! `super::{entry,exit,util}` resolve.

pub use trading_core::strategies::tpsl_sniper_1::{entry, exit, util};

pub mod backtest;
pub use backtest::run_backtest;
