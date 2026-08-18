//! Shared services usable by every backend: external HTTP clients, the Helius
//! RPC client, the generic HTTP helper, and the SOL/USD price source.
//!
//! Ingest/trade-specific services (laserstream_replay, token_sync,
//! wallet_reconcile, wallet_tokens) stay in the `backend` crate.

pub mod clients;
pub mod helius_rpc;
pub mod http;
pub mod pda;
pub mod sol_price;
pub mod veteran_roster;
