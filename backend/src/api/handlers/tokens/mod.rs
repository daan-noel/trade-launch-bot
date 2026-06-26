//! Backend token handlers shim: re-exports the core token handlers (reads, list
//! builder, creation stats, batch), the deploy DB-sync handlers, and the local
//! analysis/list/swing handlers (now in `backend-local`), so `backend`'s paths
//! resolve unchanged until the combined bin is deleted (T15).

pub use backend_core::api::handlers::tokens::*;

// Deploy DB-sync.
pub use backend_deploy::api::handlers::tokens::{preview_sync, sync_token};

// Local analysis/list/swing.
pub use backend_local::api::handlers::tokens::{
    detect_token_swings, detect_tokens_swings_batch, get_creator, get_token_analysis,
    list_analysis_results, list_creators, list_tokens,
};
