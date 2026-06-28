//! lab `tpsl_sniper_2`: the DB-backed backtest harness over the shared
//! decision layer. Re-exports core's `cohort`/`entry`/`exit`/`util` so
//! `backtest`'s `super::{cohort,entry,exit,util}` resolve.

pub use trading_core::strategies::tpsl_sniper_2::{cohort, entry, exit, util};

pub mod backtest;
pub use backtest::run_backtest;
