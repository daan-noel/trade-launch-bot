//! Local system handlers: the background-job status/cancel/result endpoints
//! (sweep + simulation + swing). The mode-agnostic system handlers (SSE, settings,
//! price, profiles/wallets/tags) live in `trading_core` and are registered via
//! `configure_core_routes`.

mod jobs;
pub use jobs::*;
