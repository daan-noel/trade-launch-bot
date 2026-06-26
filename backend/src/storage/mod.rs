//! Backend-resident storage layer.
//!
//! The shared pools + repos live in `backend-core::storage`; this module
//! re-exports them and adds the two pieces that can't live in core yet:
//! - `seed` — token-cache seeding (depends on `state::token_cache`).
//! - `repositories::grouped_sweep_repo` — sweep-result store (depends on the
//!   sweep engine's `ComboMetrics`; destined for `backend-local`).
//!
//! Re-exporting keeps every `crate::storage::…` path working unchanged.

pub use backend_core::storage::postgres;

pub mod repositories;
pub mod seed;
