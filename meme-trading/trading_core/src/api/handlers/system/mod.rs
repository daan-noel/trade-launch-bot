//! Core (mode-agnostic) system handlers: SSE stream + render bridge, settings /
//! SOL price, and wallet/profile/tag management. Live-mode (deploy) and job-status
//! (local) handlers live in the `backend`/bin crates.

mod stream;
mod system;
mod wallets;

pub use stream::*;
pub use system::*;
pub use wallets::*;
