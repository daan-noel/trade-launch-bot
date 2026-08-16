//! lab state. The shared live token cache + derived metrics + aggregate
//! `CoreState` live in `trading_core::state`; this module re-exports them (so
//! `LocalState` can `Deref` to / hold `CoreState`) and adds the local-only
//! backtest/sweep state.

#[allow(unused_imports)]
pub use trading_core::state::{core_state, token_cache, token_list_cache, token_metrics, trade_signals};

pub mod local_state;
pub mod job_progress;
pub mod analysis_cache;
pub mod discovery_result_cache;
pub mod family_search_cache;
pub mod rule_search_cache;
pub mod sim_results;
pub mod sim_summary;
