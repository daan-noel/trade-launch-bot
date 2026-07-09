//! `lake` — cold columnar tier (LAB / workstation only).
//!
//! Mirrors `trades` + a `tokens` dimension to sealed-day Parquet with the
//! generalized columns (single-sourced in [`schema`]); a DuckDB name-based reader
//! will serve sweeps/backtests/simulate. Sealed-days-only + a PG fresh-tail union
//! for recent tokens; parity test vs PG. The writer/reader/DuckDB deps
//! (`duckdb`/`arrow`/`parquet`/`rayon`) land when this is filled — and must NEVER
//! reach EC2 (`live` must not appear in this crate's reverse deps).

pub mod schema;

/// Export sealed days from the synced local PG mirror to Parquet. Stub — the
/// export/reader/parity pipeline fills in a later phase. Returns the count
/// exported (0 today).
pub async fn run_export(_database_url: &str, include_today: bool) -> anyhow::Result<u64> {
    tracing::warn!(
        include_today,
        "lake export not yet implemented — schema seam only (lake::schema). \
         Fill after the feed is live; DuckDB/arrow/parquet land here (LAB only)."
    );
    Ok(0)
}
