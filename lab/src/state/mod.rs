//! lab state. The shared live token cache + derived metrics + aggregate
//! `CoreState` live in `trading_core::state`; this module re-exports them (so
//! `LocalState` can `Deref` to / hold `CoreState`) and adds the local-only
//! backtest/sweep/swing state.

#[allow(unused_imports)]
pub use trading_core::state::{core_state, token_cache, token_list_cache, token_metrics, trade_signals};

pub mod local_state;
pub mod backtest_trade_cache;
pub mod job_progress;
pub mod matched_cache;
pub mod sim_results;
pub mod swing_results;
pub mod swing_run_cache;
