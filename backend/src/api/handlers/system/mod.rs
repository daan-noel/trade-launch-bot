//! Backend system handlers: re-exports the core system handlers (SSE, settings,
//! price, profiles/wallets/tags) and adds the backend-only ones — live-mode
//! (deploy) and job status/cancel (local).

pub use backend_core::api::handlers::system::*;

// `live_mode` (deploy) moved to `backend-deploy`; re-export it. `jobs` (local)
// stays in `backend`.
pub use backend_deploy::api::handlers::system::*;

mod jobs;
pub use jobs::*;
