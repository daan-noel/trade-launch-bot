//! Backend-resident state. The shared live token cache + derived metrics live in
//! `backend-core::state`; this module re-exports them and keeps the aggregate
//! `core_state`/`token_list_cache` (api-coupled) plus all deploy/local-only state.

pub use backend_core::state::{core_state, token_cache, token_list_cache, token_metrics};

pub mod deploy_state;
pub mod local_state;
pub mod backtest_trade_cache;
pub mod ingest_health;
pub mod job_progress;
pub mod sim_results;
pub mod swing_results;
pub mod swing_run_cache;
pub mod trade_signals;
