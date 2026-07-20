//! lab strategy harness: the generic-engine simulate path (`engine_sim`) + its
//! shared progress reporter (`sim_progress`) and replay driver. It reuses the
//! trading-free decision layer (`analysis`) from `trading_core::strategies` —
//! never the live runtime/execution path.

pub use trading_core::strategies::analysis;

pub mod admission;
pub mod candidate_cache;
pub mod engine_sim;
pub mod flow_discovery;
pub mod replay;
pub mod replay_inspect;
pub mod sim_fetch;
pub mod sim_progress;
pub mod sim_query;
pub mod token_enrich;
