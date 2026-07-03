//! Shared lake fetch for the single-rule backtests (tpsl1 / tpsl2 / swing1).
//!
//! All three `run_backtest`s share one skeleton: candidate scan on PG
//! (`collect_matching_tokens` — the `tokens` table) → fetch each candidate's trade
//! history → per-token entry/exit resolve. This module owns the **fetch** half,
//! reading the **same Parquet lake the grouped sweep reads** ([`LakeSource::load`],
//! just with `Selection::with_signatures = true`) instead of the old per-chunk PG
//! `find_by_mints_all` + `backtest_trade_cache` path — so a rule prices identically
//! whether swept or drilled into (simulate-lake-migration-plan.md).
//!
//! No `app_state`: the lake root is resolved from `SWEEP_LAKE_DIR` via
//! [`lake_root`](crate::lake::lake_root) exactly as the grouped-sweep handler does, so
//! the helper needs no DB pool and is testable in isolation.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use chrono::{NaiveDate, Utc};

use crate::lake::duck::LakeSource;
use crate::lake::{lake_root, trades_dir};
use crate::sweep::corpus::{CorpusSource, Selection, TradeWindow};
use crate::sweep::projection::CorpusTrade;

/// Uncapped per-mint history for simulate: keeps each token's **full** trade history so
/// ATH/exit match today's PG `find_by_mints_all`. Same as the grouped sweep's default
/// (`SWEEP_DEFAULT_PER_MINT_CAP = i64::MAX`) — both analysis paths run over full history,
/// so they price high-volume tokens identically. `i64::MAX` makes the DuckDB `rn <= ?`
/// clip a no-op.
const SIM_PER_MINT_CAP: i64 = i64::MAX;

/// Fetch each mint's full trade history from the Parquet lake as [`CorpusTrade`] rows,
/// keyed by mint — the single shared lake read behind all three backtests, replacing
/// the per-chunk PG fetch + `backtest_trade_cache`. A mint with no lake rows is simply
/// absent from the map (absent mint = no trades = no entry, same as the old
/// empty-history default).
///
/// Reads the same lake root (`SWEEP_LAKE_DIR`) the grouped sweep does, with simulate's
/// contract: explicit mint list, **uncapped** per-mint history. `with_signatures: true` is
/// the only thing distinguishing this from a sweep load — it populates each
/// `CorpusTrade::tx_signature` for the result tables' Solscan links.
///
/// `curve_only` is a **load-time** filter (`Selection.curve_only`) — the projected
/// [`CorpusTrade`] drops `venue`, so a venue filter can only be applied before projection.
/// The single-rule backtests pass `false` (matching the venue-unfiltered
/// `find_by_mints_all`); the per-token swing1 detect endpoint threads the request's flag.
pub async fn fetch_sim_histories(
    mints: &[String],
    curve_only: bool,
) -> Result<HashMap<String, Arc<Vec<CorpusTrade>>>> {
    if mints.is_empty() {
        return Ok(HashMap::new());
    }
    let root = lake_root();
    warn_if_stale(&root);

    let sel = Selection {
        mints: Some(mints.to_vec()),
        // No token clip: every requested mint must load (`resolve_candidates` `.take`s
        // `token_cap`). Candidate selection already happened upstream on PG.
        token_cap: mints.len(),
        created_after: None,
        created_before: None,
        per_mint_cap: SIM_PER_MINT_CAP,
        window: TradeWindow::LaunchWindow,
        curve_only,
        // Populate `tx_signature` (Solscan links) — the ONLY difference from a sweep
        // load; every other row/field is identical, so a rule prices the same either way.
        with_signatures: true,
    };

    let corpus = LakeSource::new(root).load(&sel).await?;
    Ok(corpus.tokens.into_iter().map(|t| (t.mint, t.trades)).collect())
}

/// Single-mint variant for the per-token swing1 detect endpoint: the **same** uncapped
/// lake read the backtest uses, so the detect funnel and the sim resolve identical legs +
/// entry + exit by construction (no separate PG read, no `MAX_TRADES_RETAINED` cap). Returns
/// the mint's full history, or an empty buffer when the token has no lake rows.
pub async fn fetch_sim_history_one(mint: &str, curve_only: bool) -> Result<Arc<Vec<CorpusTrade>>> {
    let mut map = fetch_sim_histories(std::slice::from_ref(&mint.to_string()), curve_only).await?;
    Ok(map.remove(mint).unwrap_or_default())
}

/// Log a non-fatal warning if the lake looks stale. The lake is **sealed-days-only**
/// (`lake-export` writes `< today`), so simulate on tokens created today gets
/// truncated/empty histories unless `--include-today` runs on a cadence. Cheap
/// filesystem check; never blocks the run.
fn warn_if_stale(root: &Path) {
    match newest_lake_day(root) {
        Some(newest) => {
            let today = Utc::now().date_naive();
            if newest < today {
                tracing::warn!(
                    newest = %newest, today = %today,
                    "simulate: lake newest day precedes today — histories for recent tokens may be \
                     truncated; run `lab lake-export --include-today`"
                );
            }
        }
        None => tracing::warn!(
            root = %root.display(),
            "simulate: no lake day files found — run `lab lake-export` first"
        ),
    }
}

/// Newest sealed day present in the lake trades dir (`<root>/trades/dt=YYYY-MM-DD/`),
/// or `None` if the dir is absent/empty. Pure filesystem read (no DuckDB), so the
/// staleness guard is cheap and unit-testable.
fn newest_lake_day(root: &Path) -> Option<NaiveDate> {
    let mut newest: Option<NaiveDate> = None;
    for entry in std::fs::read_dir(trades_dir(root)).ok()?.flatten() {
        let name = entry.file_name();
        if let Some(date_str) = name.to_string_lossy().strip_prefix("dt=") {
            if let Ok(d) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
                newest = Some(newest.map_or(d, |n| n.max(d)));
            }
        }
    }
    newest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_lake_day_picks_the_max_partition() {
        let root = std::env::temp_dir().join(format!("sim-fetch-test-{}", std::process::id()));
        let trades = trades_dir(&root);
        for day in ["dt=2026-06-27", "dt=2026-06-30", "dt=2026-06-28"] {
            std::fs::create_dir_all(trades.join(day)).unwrap();
        }
        // A non-partition dir must be ignored, not parsed.
        std::fs::create_dir_all(trades.join("not-a-day")).unwrap();

        let got = newest_lake_day(&root);
        std::fs::remove_dir_all(&root).ok();
        assert_eq!(got, NaiveDate::from_ymd_opt(2026, 6, 30));
    }

    #[test]
    fn newest_lake_day_is_none_when_absent() {
        let root = std::env::temp_dir().join("sim-fetch-test-definitely-absent-xyz");
        assert_eq!(newest_lake_day(&root), None);
    }
}
