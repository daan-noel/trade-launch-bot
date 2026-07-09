//! Deploy-crate state. Re-exports the shared core state and adds `DeployState`
//! (live-trading handles + the per-mint sync gate).

pub use trading_core::state::{
    core_state, token_cache, token_list_cache, token_metrics, trade_signals,
};

pub mod deploy_state;
