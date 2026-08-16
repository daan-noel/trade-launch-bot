//! `lab` library surface: the analysis/backtest stack — the param-sweep
//! engine (`sweep`), local-only state (`state`),
//! and the DB-backed backtest harness (`strategies`). The `lab` binary
//! (`main.rs`) is the composition root that wires these together; `backend` (until
//! deleted in T15) re-exports these modules so its existing `crate::sweep::…` /
//! `crate::state::local_state::…` paths keep resolving.
//!
//! Deps are deliberately trading-free: `trading_core` (the sweep CU cost model
//! reads constants from `trading_core::config::constants`) + `rayon`/`arrow`/
//! `parquet`. No `pump-trader`, no `ingest-laserstream` — the local box never
//! links the live trading/RPC stack.

pub use trading_core::models;

pub mod api;
pub mod discovery;
pub mod family_search;
pub mod lake;
pub mod rule_search;
pub mod state;
pub mod storage;
pub mod strategies;
pub mod sweep;
