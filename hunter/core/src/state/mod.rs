//! Shared core state: the live token cache and its derived metrics.
//!
//! The aggregate `core_state` + `token_list_cache` live with the api layer
//! (they reference api-handler DTOs); deploy/local-only state stays in `backend`.

pub mod core_state;
pub mod token_cache;
pub mod token_list_cache;
pub mod token_metrics;
pub mod trade_signals;
