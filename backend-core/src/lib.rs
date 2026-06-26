//! Shared core for the meme-trading backends.
//!
//! Modules are migrated in here task-by-task per `local-deploy-mode-split-plan.md`
//! (Phase 2). `backend` depends on it.

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
