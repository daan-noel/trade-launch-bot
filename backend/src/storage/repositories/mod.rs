//! Re-exports the shared core repos and the sweep-coupled `grouped_sweep_repo`
//! (now in `backend-local`), so `crate::storage::repositories::…` paths resolve
//! unchanged until the combined `backend` bin is deleted (T15).

pub use backend_core::storage::repositories::*;

pub use backend_local::storage::repositories::grouped_sweep_repo;
