//! backend state shim. The state types now live in the split crates — shared in
//! `backend-core`, deploy-only in `backend-deploy`, local-only in `backend-local`.
//! This re-exports all three so `backend`'s `crate::state::…` paths resolve
//! unchanged until the combined `backend` bin is deleted (T15).

#[allow(unused_imports)]
pub use backend_core::state::{core_state, token_cache, token_list_cache, token_metrics, trade_signals};

// Deploy-only (`DeployState`/`SyncGate`).
pub use backend_deploy::state::deploy_state;

// Local-only (backtest/sweep/swing).
#[allow(unused_imports)]
pub use backend_local::state::{
    backtest_trade_cache, job_progress, local_state, sim_results, swing_results, swing_run_cache,
};
