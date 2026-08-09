//! `lake` — cold columnar tier (LAB / workstation only).
//!
//! Mirrors `trades` + a `tokens` dimension to sealed-day Parquet with the
//! generalized columns (single-sourced in [`schema`]); a DuckDB name-based reader
//! will serve sweeps/backtests/simulate. Sealed-days-only + a PG fresh-tail union
//! for recent tokens; parity test vs PG. The writer/reader/DuckDB deps
//! (`duckdb`/`arrow`/`parquet`/`rayon`) land when this is filled — and must NEVER
//! reach EC2 (`live` must not appear in this crate's reverse deps).

// Only the column SSOT lives here. The export/reader/parity pipeline
// (DuckDB/arrow/parquet, LAB-only) builds on it and lands with its deps — there is
// deliberately no `run_export` entry point until then, because a stub returning
// `Ok(0)` reads exactly like an export that ran and found nothing.
pub mod schema;
