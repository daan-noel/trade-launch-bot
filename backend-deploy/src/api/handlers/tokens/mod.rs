//! Deploy token handlers. Re-exports the core token handlers (so `sync.rs` can
//! reach `TokenDetail`) and adds the deploy-only DB-sync handler.

pub use backend_core::api::handlers::tokens::*;

pub mod sync;
pub use sync::*;
