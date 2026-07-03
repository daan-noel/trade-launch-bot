//! lab strategy harness: the DB-backed backtest engine (`tpsl_sniper_N::
//! backtest`) + its shared progress reporter (`sim_progress`). It reuses the
//! trading-free decision layer (`entry`/`exit`/`util`, `analysis`) from
//! `trading_core::strategies` — never the live runtime/execution path.

pub use trading_core::strategies::analysis;

pub mod sim_fetch;
pub mod sim_progress;
pub mod sim_query;
pub mod swing_1;
pub mod token_enrich;
pub mod tpsl_sniper_1;
pub mod tpsl_sniper_2;
