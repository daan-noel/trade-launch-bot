//! Shared core for the hunter backends.
//!
//! Shared lib consumed by the `live` and `lab` bins.
//! See `@plans/modes/crate-split.md` for the crate-split design.

pub mod api;
pub mod config;
pub mod ingest;
pub mod models;
pub mod serde_wire;

// Moved into the pure `hunter-engine` crate (strategy redesign) — re-exported
// here so `trading_core::grouping`/`::metrics` paths keep working during the
// transition. New code may import `hunter_engine::*` directly.
pub use hunter_engine::{grouping, metrics};
pub mod services;
pub mod state;
pub mod storage;
pub mod strategies;
pub mod wallet_interner;
