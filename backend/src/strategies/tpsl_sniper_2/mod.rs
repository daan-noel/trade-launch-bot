//! Backend-resident `tpsl_sniper_2`: re-exports the live strategy modules from
//! `backend-deploy` and adds the local-only `backtest` harness (depends on
//! `LocalState` + `crate::sweep`, which stay in `backend`).

pub use backend_deploy::strategies::tpsl_sniper_2::*;

pub mod backtest;
pub use backtest::run_backtest;
