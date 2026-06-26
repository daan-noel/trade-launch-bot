//! Backend `tpsl_sniper_1` shim: re-exports the live strategy modules from
//! `backend-deploy` (which re-exports the decision layer from `backend-core`) and
//! the `backtest` harness from `backend-local`, so `backend`'s paths resolve
//! unchanged until the combined bin is deleted (T15).

pub use backend_deploy::strategies::tpsl_sniper_1::*;

pub use backend_local::strategies::tpsl_sniper_1::{backtest, run_backtest};
