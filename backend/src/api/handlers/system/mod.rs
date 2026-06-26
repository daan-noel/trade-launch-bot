//! Backend system handlers: re-exports the core system handlers (SSE, settings,
//! price, profiles/wallets/tags) and adds the backend-only ones — live-mode
//! (deploy) and job status/cancel (local).

pub use backend_core::api::handlers::system::*;

mod jobs;
mod live_mode;

pub use jobs::*;
pub use live_mode::*;
