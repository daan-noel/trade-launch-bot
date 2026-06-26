//! Backend-resident state. The shared live token cache + derived metrics live in
//! `backend-core::state`; this module re-exports them and keeps the aggregate
//! `core_state`/`token_list_cache` (api-coupled) plus all deploy/local-only state.

#[allow(unused_imports)]
pub use backend_core::state::{core_state, token_cache, token_list_cache, token_metrics, trade_signals};

// `DeployState`/`SyncGate` moved to `backend-deploy`; re-export so existing
// `crate::state::deploy_state::…` paths keep resolving.
pub use backend_deploy::state::deploy_state;

pub mod local_state;
pub mod backtest_trade_cache;
pub mod job_progress;
pub mod sim_results;
pub mod swing_results;
pub mod swing_run_cache;
