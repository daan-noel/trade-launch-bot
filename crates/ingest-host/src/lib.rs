//! `ingest-host` — translate the borrowed feed into DB rows (LIVE box only).
//!
//! `ingest-laserstream` is a standalone gRPC transport that emits raw
//! `IngestEvent`s; this crate is the host adapter that bridges those onto
//! platform-core's `raw_txs` / `trades` schema through the pump.fun/SOL venue
//! adapter (`launchpad_id = pump_fun`, `quote_asset_id = SOL`, venue-neutral
//! reserve pair). Wired in Phase 6.
//!
//! Dep partition: LIVE only. Must NOT appear in `lab`'s dep graph.

/// Phase-6 seam: build the transport and spawn the ingest → DB pipeline.
pub fn spawn_ingest() {
    todo!("Phase 6: bridge ingest-laserstream IngestEvents onto raw_txs/trades")
}
