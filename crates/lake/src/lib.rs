//! `lake` — cold columnar tier (LAB / workstation only).
//!
//! Mirrors `trades` + a `tokens` dimension to sealed-day Parquet with the
//! generalized columns (`amount_quote`, `amount_base`, `reserve_quote`,
//! `reserve_base`, `launchpad_id`, `quote_asset_id`); a DuckDB name-based reader
//! serves sweeps/backtests/simulate. Column names will be single-sourced in one
//! `schema.rs` with a writer/reader guard test; sealed-days-only + PG fresh-tail
//! union for recent tokens; parity test vs PG. Filled after the feed is live.
//!
//! Dep partition: LAB only. `duckdb`/`arrow`/`parquet`/`rayon` live behind this
//! crate and must NEVER reach EC2 — `live` must not appear in its reverse deps.

/// Later-phase seam: export sealed days from the synced local PG mirror to Parquet.
pub fn lake_export() {
    todo!("Later: seal-day export PG → Parquet; DuckDB reader; parity test")
}
