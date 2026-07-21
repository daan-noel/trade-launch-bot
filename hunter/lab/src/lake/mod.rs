//! The Parquet **lake** — hop 2 of the `lab` data pipeline
//! (`EC2-PG → local-PG → Parquet lake → DuckDB`, live-lab-remake-plan.md Phase 4).
//!
//! Hop 1 ([`scripts/db-incremental-sync.ps1`]) lands sealed daily Timescale chunks
//! into local Postgres. This module is hop 2: it exports each newly-sealed day of
//! the local `trades` table into an **immutable, day-partitioned** Parquet dataset,
//! plus a small `tokens` dimension carrying the grouping fingerprint. Hop 3
//! ([`super::lake::duck`]) then assembles a sweep corpus by querying the lake with
//! DuckDB.
//!
//! Layout (Hive-style, so DuckDB's `hive_partitioning=true` reads the partition key
//! straight from the path):
//!
//! ```text
//!   <lake_root>/trades/dt=YYYY-MM-DD/data.parquet   one immutable file per sealed day
//!   <lake_root>/trades/dt=YYYY-MM-DD/_meta.json     seal sidecar (row_count + exported_at)
//!   <lake_root>/tokens/tokens.parquet               token dimension (rewritten each run)
//! ```
//!
//! **Immutability + completeness.** A day file is written **once** and skipped on
//! later runs *only if* its `_meta.json` sidecar's `row_count` still matches
//! `COUNT(*)` in local PG for that UTC day. A missing/mismatched sidecar forces a
//! re-seal (truncated export, partial write, or a later sync that filled a gap).
//! The `tokens` dimension is small and mutates (new tokens, fingerprints), so it is
//! rewritten wholesale each export.
//!
//! **Unit parity.** Rows are written in the same f64 *decimal* units the sweep's
//! [`CorpusTrade`](crate::sweep::projection::CorpusTrade) uses, mirroring
//! `trade_repo`'s `TradeDbRow` read conversion exactly (lamports→SOL ÷1e9; raw token
//! units kept as f64; virtual reserves raw→f64; the dropped `real_*_reserves` become
//! `None`). The lake is therefore a columnar mirror of what the PG corpus source
//! would have produced — the precondition for "metrics match a PG-sourced baseline".

pub mod duck;
pub mod export;
pub mod schema;

use std::path::{Path, PathBuf};

use chrono::NaiveDate;

/// Where the Parquet lake lives: `$SWEEP_LAKE_DIR` if set, else a stable subdir of
/// the OS temp dir. Unlike the per-selection corpus *cache* (a pure optimisation),
/// the lake is the durable immutable history, so a real deployment sets
/// `SWEEP_LAKE_DIR` to a persistent path; the temp default keeps tests/dev zero-config.
pub fn lake_root() -> PathBuf {
    std::env::var_os("SWEEP_LAKE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("pumpfun-lake"))
}

/// On-disk last-simulation result cache (`<root>/sim-results/`).
///
/// Sibling of the Parquet lake under the same `$SWEEP_LAKE_DIR` root so a durable
/// lake path also keeps finished sim rows across `hunter-lab` restarts. See
/// [`crate::state::sim_results`].
pub fn sim_results_dir(root: &Path) -> PathBuf {
    root.join("sim-results")
}

/// On-disk last-flow-discovery-result file (`<root>/flow-discovery/last.json`).
///
/// Sibling of [`sim_results_dir`]: a discovery run folds the whole corpus and can
/// take a while, so persisting the last result under the same `$SWEEP_LAKE_DIR`
/// root lets it survive `hunter-lab` restarts instead of forcing a re-run just to
/// see it again. See [`crate::state::discovery_result_cache`].
pub fn discovery_result_path(root: &Path) -> PathBuf {
    root.join("flow-discovery").join("last.json")
}

/// Directory holding the day-partitioned trade files (`<root>/trades`).
pub fn trades_dir(root: &Path) -> PathBuf {
    root.join("trades")
}

/// The immutable Parquet file for one sealed day (`<root>/trades/dt=YYYY-MM-DD/data.parquet`).
pub fn trades_day_file(root: &Path, day: NaiveDate) -> PathBuf {
    trades_dir(root)
        .join(format!("dt={}", day.format("%Y-%m-%d")))
        .join("data.parquet")
}

/// Seal sidecar for one sealed day (`<root>/trades/dt=YYYY-MM-DD/_meta.json`).
/// Carries the PG `COUNT(*)` at export time so a later run can detect an incomplete
/// seal and re-export instead of skip-if-exists.
pub fn trades_day_meta(root: &Path, day: NaiveDate) -> PathBuf {
    trades_dir(root)
        .join(format!("dt={}", day.format("%Y-%m-%d")))
        .join("_meta.json")
}

/// The token-dimension file (`<root>/tokens/tokens.parquet`).
pub fn tokens_file(root: &Path) -> PathBuf {
    root.join("tokens").join("tokens.parquet")
}

/// A glob over every day file, for DuckDB's `read_parquet(..., hive_partitioning=true)`.
/// Returned as a forward-slash string so it is portable inside a DuckDB SQL literal
/// (DuckDB accepts `/` on Windows too).
pub fn trades_glob(root: &Path) -> String {
    let p = trades_dir(root).join("dt=*").join("data.parquet");
    p.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_file_path_is_hive_partitioned() {
        let root = Path::new("/lake");
        let day = NaiveDate::from_ymd_opt(2026, 6, 27).unwrap();
        let p = trades_day_file(root, day);
        let s = p.to_string_lossy().replace('\\', "/");
        assert!(s.ends_with("trades/dt=2026-06-27/data.parquet"), "got {s}");
        let m = trades_day_meta(root, day);
        let ms = m.to_string_lossy().replace('\\', "/");
        assert!(ms.ends_with("trades/dt=2026-06-27/_meta.json"), "got {ms}");
    }

    #[test]
    fn trades_glob_uses_forward_slashes_and_wildcard() {
        let g = trades_glob(Path::new("/lake"));
        assert!(g.contains("trades/dt=*/data.parquet"), "got {g}");
        assert!(!g.contains('\\'), "glob must be forward-slashed for DuckDB: {g}");
    }
}
