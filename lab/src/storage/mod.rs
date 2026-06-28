//! lab storage layer: re-exports the shared core pools + repos and adds
//! the sweep-coupled `repositories::grouped_sweep_repo` (depends on the sweep
//! engine's `ComboMetrics`), so `crate::storage::…` paths resolve unchanged.

pub use trading_core::storage::postgres;

pub mod repositories;
