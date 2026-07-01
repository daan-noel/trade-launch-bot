//! lab `swing_1`: the DB-backed backtest harness over the shared decision layer.
//! Re-exports core's `entry`/`exit` so `backtest`'s `super::{entry,exit}` resolve.
//! (swing1 has no `util` module — it borrows tpsl1's `none_if_zero_*` helpers.)

pub use trading_core::strategies::swing_1::{entry, exit};

pub mod backtest;
pub use backtest::run_backtest;
