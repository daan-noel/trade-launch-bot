//! `lake` — cold columnar tier (LAB / workstation only).
//!
//! Mirrors `trades` + a `tokens` dimension to sealed-day Parquet with the
//! generalized columns (single-sourced in [`schema`]); a DuckDB name-based reader
//! will serve sweeps/backtests/simulate. Sealed-days-only + a PG fresh-tail union
//! for recent tokens; parity test vs PG. The writer/reader/DuckDB deps
//! (`duckdb`/`arrow`/`parquet`/`rayon`) land when this is filled — and must NEVER
//! reach EC2 (`live` must not appear in this crate's reverse deps).

// The export/reader/parity pipeline (DuckDB/arrow/parquet, LAB-only) fills in a
// later phase and will build on the column-SSOT seam below. The previous
// `run_export` do-nothing stub was removed rather than left returning `Ok(0)`.
pub mod schema;
