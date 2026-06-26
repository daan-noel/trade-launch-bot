//! backend-local strategy harness: the DB-backed backtest engine (`tpsl_sniper_N::
//! backtest`) + its shared progress reporter (`sim_progress`). It reuses the
//! trading-free decision layer (`entry`/`exit`/`cohort`/`util`, `analysis`) from
//! `backend_core::strategies` — never the live runtime/execution path.

pub use backend_core::strategies::analysis;

pub mod sim_progress;
pub mod tpsl_sniper_1;
pub mod tpsl_sniper_2;
