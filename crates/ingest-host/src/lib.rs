//! `ingest-host` — translate the borrowed feed into DB rows (LIVE box only).
//!
//! `ingest-laserstream` is a standalone gRPC transport that emits raw
//! `IngestEvent`s; this crate is the host adapter that bridges those onto
//! platform-core's `raw_txs` / `trades` / `tokens` schema through the pump.fun/SOL
//! venue adapter (`launchpad_id = pump_fun`, `quote_asset_id = SOL`, venue-neutral
//! reserve pair).
//!
//! - `pumpfun`  — the `LaunchpadAdapter` impl (resolves interned dimension ids).
//! - `map`      — pure event → row mappers (unit-testable, no DB/network).
//! - `consumer` — `spawn_ingest` + the batched ingest → DB pipeline.
//!
//! Dep partition: LIVE only. Must NOT appear in `lab`'s dep graph.

pub mod consumer;
pub mod map;
pub mod pumpfun;

pub use consumer::spawn_ingest;
pub use ingest_laserstream::IngestHandle;
pub use pumpfun::PumpFunAdapter;
