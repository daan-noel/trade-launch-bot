//! Backend strategies shim. The live event/timer path lives in `backend-deploy`;
//! the backtest/sweep harness (`backtest`, `sim_progress`) lives in
//! `backend-local`. Both are re-exported so `backend`'s `crate::strategies::…`
//! paths resolve unchanged until the combined bin is deleted (T15).

#[allow(unused_imports)]
pub use backend_deploy::strategies::{analysis, runner, StrategyRunner};

#[allow(unused_imports)]
pub use backend_local::strategies::sim_progress;
pub mod tpsl_sniper_1;
pub mod tpsl_sniper_2;
