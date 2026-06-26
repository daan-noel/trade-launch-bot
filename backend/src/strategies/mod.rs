//! Backend-resident strategies module. The live event/timer strategy path moved
//! to `backend-deploy`; this module re-exports it and keeps the local-only
//! backtest/sweep harness (`backtest`, `sim_progress`) that depends on
//! `LocalState` + `crate::sweep`.

#[allow(unused_imports)]
pub use backend_deploy::strategies::{analysis, runner, StrategyRunner};

pub mod sim_progress;
pub mod tpsl_sniper_1;
pub mod tpsl_sniper_2;
