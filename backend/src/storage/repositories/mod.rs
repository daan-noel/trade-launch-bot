//! Re-exports the shared core repos and adds the backend-only `grouped_sweep_repo`
//! (sweep-coupled), so `crate::storage::repositories::…` paths resolve unchanged.

pub use backend_core::storage::repositories::*;

pub mod grouped_sweep_repo;
