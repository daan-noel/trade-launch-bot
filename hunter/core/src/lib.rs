//! Shared core for the hunter backends.
//!
//! Shared lib consumed by the `live` and `lab` bins.
//! See `@plans/modes/crate-split.md` for the crate-split design.

pub mod analyzers;
pub mod api;
pub mod config;
pub mod grouping;
pub mod ingest;
pub mod models;
pub mod services;
pub mod state;
pub mod storage;
pub mod strategies;
pub mod wallet_interner;
