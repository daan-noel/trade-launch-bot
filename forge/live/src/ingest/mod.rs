//! Ingest host-adapter (`forge/live/src/ingest/`) — translate the borrowed feed
//! into DB rows (LIVE box only). Folded in from the former `ingest-host` crate.
//!
//! `ingest-laserstream` is a standalone gRPC transport that emits raw
//! `IngestEvent`s; this module is the host adapter that bridges those onto
//! platform-core's `raw_txs` / `trades` / `tokens` schema through the pump.fun/SOL
//! venue adapter (`launchpad_id = pump_fun`, `quote_asset_id = SOL`, venue-neutral
//! reserve pair).
//!
//! - `pumpfun`   — the `LaunchpadAdapter` impl (resolves interned dimension ids).
//! - `map`       — pure event → row mappers (unit-testable, no DB/network).
//! - `consumer`  — `spawn_ingest` + the hot recv loop (no DB I/O).
//! - `db_writer` — the decoupled batched writer task (all DB I/O + interning).
//! - `watchdog`  — OS-thread process watchdog on the writer heartbeat.
//!
//! Dep partition: LIVE only. Must NOT appear in `lab`'s dep graph.

pub mod consumer;
pub mod db_writer;
pub mod map;
pub mod metrics;
pub mod pumpfun;
pub mod watchdog;

#[cfg(test)]
mod roundtrip_test;

pub use consumer::spawn_ingest;
pub use ingest_pumpfun::IngestHandle;
pub use metrics::IngestMetrics;
