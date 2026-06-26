//! Shared core for the meme-trading backends.
//!
//! Shared lib consumed by the `backend-deploy` and `backend-local` bins.
//! See `@plans/modes/crate-split.md` for the crate-split design.

pub mod analyzers;
pub mod api;
pub mod config;
pub mod grouping;
pub mod models;
pub mod services;
pub mod state;
pub mod storage;
pub mod strategies;
pub mod wallet_interner;
