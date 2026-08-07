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
use crate::models::trade::{Trade, TradeRow};
use crate::storage::repositories::trade_repo::TradeRepo;
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
    with_flow: bool,
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
        // `ix_labels` + `wallet` for volume-flow metrics (V2) — off unless the run
        // references `m_flow_*` so non-flow sims stay slim.
        with_flow,
        // Hash-resolved flow keys only — no consumer here reads label text.
        with_flow_text: false,
    };

    let corpus = LakeSource::new(root).load(&sel).await?;
    Ok(corpus.tokens.into_iter().map(|t| (t.mint, t.trades)).collect())
}

/// Single-mint variant for the per-token swing1 detect endpoint: the **same** uncapped
/// lake read the backtest uses, so the detect funnel and the sim resolve identical legs +
/// entry + exit by construction (no separate PG read, no `MAX_TRADES_RETAINED` cap). Returns
/// the mint's full history, or an empty buffer when the token has no lake rows.
pub async fn fetch_sim_history_one(
    mint: &str,
    curve_only: bool,
    with_flow: bool,
) -> Result<Arc<Vec<CorpusTrade>>> {
    let mut map =
        fetch_sim_histories(std::slice::from_ref(&mint.to_string()), curve_only, with_flow).await?;
    Ok(map.remove(mint).unwrap_or_default())
}

/// A token's **full** trade history for per-token analysis — the sealed Parquet lake
/// (deep past) unioned with the Postgres fresh tail (recent trades not yet exported).
///
/// The lake is sealed-days-only and frozen at the last `lake-export`, so a token
/// created/traded *after* that export is absent from it — the reason a fresh live
/// position's swing-detect overlay went blank while its entry/exit markers (read from
/// PG fills) still showed. PG keeps a rolling ~30-day window (its retention), so
/// appending every PG row on a slot the lake never reached fills that gap. The split
/// is symmetric: a brand-new token is PG-only, a token older than PG retention is
/// lake-only, and a token straddling the export boundary is stitched from both.
///
/// Spliced by **slot** (not row identity): the lake IS an export of PG, so keeping all
/// lake rows + only the PG rows on higher slots unions the two with no double-count and
/// no per-row dedup key. `curve_only` is applied on both sides — the lake at load, PG by
/// `venue` here (the projected [`CorpusTrade`] has dropped `venue`).
pub async fn fetch_full_history_one(
    repo: &TradeRepo,
    mint: &str,
    curve_only: bool,
) -> Result<Arc<Vec<CorpusTrade>>> {
    fetch_full_history_one_opts(repo, mint, curve_only, false).await
}

/// Like [`fetch_full_history_one`], optionally loading flow columns (`ix_labels` /
/// `wallet`) from the lake and projecting them from the PG tail.
pub async fn fetch_full_history_one_opts(
    repo: &TradeRepo,
    mint: &str,
    curve_only: bool,
    with_flow: bool,
) -> Result<Arc<Vec<CorpusTrade>>> {
    let lake = fetch_sim_history_one(mint, curve_only, with_flow).await?;

    // PG fresh tail: full per-mint history (`limit <= 0` ⇒ unbounded), venue-filtered
    // to match the lake's `curve_only`.
    let mut pg: Vec<Trade> = repo.find_by_mint_paged(mint, 0, 0).await?;
    if curve_only {
        pg.retain(|t| t.venue == "curve");
    }

    // Keep every lake row, then append the PG rows on slots the lake never reached.
    let tail = pg_tail_beyond_lake(&lake, pg);
    if tail.is_empty() {
        return Ok(lake); // no fresh tail — reuse the lake Arc, no copy
    }
    // Project the PG tail with real-reserve reconstruction (the persisted rows carry
    // only the virtual reserve; `project_pg_tail` derives `real_reserve_sol` the same
    // way the lake load does, so liquidity/deadness match the sealed corpus). `with_flow`
    // additionally loads the ix-label/wallet columns for the volume-flow metrics.
    let mut merged = (*lake).clone();
    merged.extend(crate::sweep::projection::project_pg_tail(&tail, with_flow));
    Ok(Arc::new(merged))
}

/// The PG rows to splice onto the lake: those on a **slot beyond the lake's newest**.
/// The lake is an export of PG, so within the lake's slot range the two hold the same
/// rows — taking only higher slots unions them with no double-count and no per-row
/// dedup key. An empty lake (`None` max) ⇒ the whole PG history is the tail. Pure so
/// the slot-splice is unit-testable without a live PG/lake.
fn pg_tail_beyond_lake(lake: &[CorpusTrade], pg: Vec<Trade>) -> Vec<Trade> {
    match lake.iter().map(|t| t.slot).max() {
        Some(s) => pg.into_iter().filter(|t| t.slot() > s).collect(),
        None => pg,
    }
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
                     truncated; run `./scripts/db-incremental-sync.ps1 -IncludeToday -ExportLake` \
                     (or `lab lake-export --include-today` if local PG is already fresh)"
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

    fn corpus_at_slot(slot: u64) -> CorpusTrade {
        CorpusTrade {
            flow: crate::sweep::projection::FlowKeys::default(),
            block_time: Utc::now(),
            amount_sol: 1.0,
            token_amount: 100.0,
            price_per_token: 0.01,
            reserve_sol: None,
            reserve_token: None,
            real_reserve_sol: None,
            real_token_reserves: None,
            slot,
            leg_index: 0,
            is_buy: true,
            tx_signature: None,
            ix_labels: None,
            wallet: None,
        }
    }

    fn pg_trade_at_slot(slot: u64) -> Trade {
        use trading_core::models::trade::TradeType;
        Trade::new(
            "mint".into(),
            "wallet".into(),
            TradeType::Buy,
            1.0,
            100,
            "sig".into(),
            slot,
            Utc::now(),
        )
    }

    /// The splice takes only PG rows on slots beyond the lake's newest — the lake
    /// already holds everything up to its max slot (it's a PG export), so the shared
    /// slot 20 must NOT be re-appended.
    #[test]
    fn pg_tail_takes_only_slots_beyond_lake() {
        let lake = vec![corpus_at_slot(10), corpus_at_slot(20)];
        let pg = vec![
            pg_trade_at_slot(15),
            pg_trade_at_slot(20),
            pg_trade_at_slot(25),
            pg_trade_at_slot(30),
        ];
        let tail: Vec<u64> = pg_tail_beyond_lake(&lake, pg).iter().map(|t| t.slot()).collect();
        assert_eq!(tail, vec![25, 30], "only slots past the lake max (20) splice in");
    }

    /// Empty lake (token created after the last export, or older than the lake's
    /// coverage) ⇒ the whole PG history is the tail. This is the reported-bug case:
    /// a fresh token is PG-only, so the overlay must still get every trade.
    #[test]
    fn pg_tail_is_whole_pg_when_lake_empty() {
        let pg = vec![pg_trade_at_slot(5), pg_trade_at_slot(6)];
        assert_eq!(pg_tail_beyond_lake(&[], pg).len(), 2, "empty lake ⇒ entire PG history");
    }
}
