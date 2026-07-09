//! Deploy token handlers. Re-exports the core token handlers (so `sync.rs` can
//! reach `TokenDetail`) and adds the deploy-only handlers: the token list and the
//! DB-sync handler.

pub use trading_core::api::handlers::tokens::*;

mod list;
pub mod sync;
pub use list::*;
pub use sync::*;
