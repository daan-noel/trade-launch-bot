//! DuckDB corpus source (hop 3) — assemble a sweep [`Corpus`] by querying the
//! Parquet lake instead of Postgres.
//!
//! This is the **sole** [`CorpusSource`](crate::sweep::corpus::CorpusSource) (the old
//! Postgres `DbSource` was retired at the lake cutover): it yields the same output
//! ([`CorpusToken`] with slim [`CorpusTrade`] buffers + a grouping
//! [`TokenFingerprint`]), but the trades come from the immutable day-partitioned
//! Parquet files and the fingerprint/symbol from the `tokens` dimension, queried
//! through an in-memory DuckDB connection. DuckDB does the per-mint windowing
//! (`ROW_NUMBER`) and the candidate selection over columnar Parquet — no DB in the
//! sweep loop, no full-`trades` scan.
//!
//! **Arrow-version isolation.** DuckDB bundles its own `arrow`, which may not match
//! lab's `arrow 53`. To avoid two arrow crates colliding in one type, this module
//! uses DuckDB's **row API** (`query_map`) only — never `query_arrow` — and
//! re-projects into our `CorpusTrade` by hand.
//!
//! Runtime-unverified this session (no lake to read). The SQL mirrors the
//! pre-cutover PG-sourced shape; validation against a PG-sourced baseline is the
//! Phase-4 "done-when" gate (needs the DB/EC2 pipeline).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use duckdb::Connection;

use trading_core::config::constants::approx_real_sol_reserves;

use crate::sweep::corpus::{
    Corpus, CorpusSource, Selection, TradeWindow, SWEEP_DEFAULT_PER_MINT_CAP,
};
use crate::sweep::grouping::TokenFingerprint;
use crate::sweep::projection::CorpusTrade;
use crate::sweep::corpus::CorpusToken;

use super::{tokens_file, trades_glob};

/// Reads a sweep corpus from the Parquet lake at `root` via DuckDB.
pub struct LakeSource {
    root: PathBuf,
}

impl LakeSource {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The corpus-cache identity for `sel` against this lake — **(selection, lake
    /// version)**, the same value a completed [`load`](CorpusSource::load) stamps on
    /// `Corpus::hash`. Lets a caller check the in-memory `sweep_corpus_cache` for a
    /// reusable corpus *before* paying for a DuckDB load (grouped sweep + drill-in).
    pub fn selection_hash(&self, sel: &Selection) -> String {
        lake_hash(&self.root, sel)
    }

    /// Open an in-memory DuckDB and confirm the lake has both datasets. A missing
    /// lake is a clear error (run the export first) rather than DuckDB's opaque
    /// "No files found".
    ///
    /// The connection is **bounded** before any query runs — see
    /// [`bound_duckdb_memory`]. An unbounded DuckDB is entitled to 80% of physical
    /// RAM, which on a 16 GB workstation is more than the whole sweep's own budget.
    fn connect(&self) -> Result<(DuckSession, String, String)> {
        let tokens = tokens_file(&self.root);
        if !tokens.exists() {
            bail!(
                "lake token dimension not found at {} — run `lab lake-export` first",
                tokens.display()
            );
        }
        if !super::trades_dir(&self.root).exists() {
            bail!(
                "lake trades dir not found under {} — run `lab lake-export` first",
                self.root.display()
            );
        }
        let conn = Connection::open_in_memory().context("opening in-memory DuckDB")?;
        let spill = bound_duckdb_memory(&conn);
        let tokens_lit = sql_str(&tokens.to_string_lossy().replace('\\', "/"));
        let trades_lit = sql_str(&trades_glob(&self.root));
        Ok((DuckSession { conn, _spill: spill }, trades_lit, tokens_lit))
    }

    /// Mints in `sel`'s creation window whose lake fingerprint **matches `fp`**,
    /// newest-first, clipped to `sel.token_cap`; plus whether the clip bit.
    ///
    /// **Reads the tokens dimension only** — no trade scan. This exists so a
    /// fingerprint-scoped caller can turn its scope into an explicit `sel.mints`
    /// *before* paying for trades: filtering after [`CorpusSource::load`] means
    /// loading the full history of every token in the window and then discarding all
    /// but the handful that match, which is the whole cost of the run. The token
    /// dimension is a single small Parquet file, so this is cheap by comparison.
    ///
    /// Matching goes through `hunter_engine::fingerprint::matches`, the same SSOT the
    /// live entry gate arms on — so the resulting mint set is exactly what a
    /// post-load `retain` would have kept, and `token_cap` now bounds *matched*
    /// tokens rather than candidates.
    pub async fn matching_mints(
        &self,
        sel: &Selection,
        fp: hunter_engine::fingerprint::Fingerprint,
    ) -> Result<(Vec<String>, bool)> {
        let this = LakeSource::new(self.root.clone());
        let sel = sel.clone();
        tokio::task::spawn_blocking(move || this.matching_mints_sync(&sel, &fp))
            .await
            .context("DuckDB fingerprint-match task panicked")?
    }

    fn matching_mints_sync(
        &self,
        sel: &Selection,
        fp: &hunter_engine::fingerprint::Fingerprint,
    ) -> Result<(Vec<String>, bool)> {
        let (session, _trades_lit, tokens_lit) = self.connect()?;
        let conn = &session.conn;
        // Microseconds + the `is_mayhem_mode` exclusion — the same candidate
        // predicate `resolve_candidates` applies, so a scoped run and an unscoped one
        // draw from the identical population.
        let after = sel.created_after.map(|t| t.timestamp_micros()).unwrap_or(i64::MIN);
        let before = sel.created_before.map(|t| t.timestamp_micros()).unwrap_or(i64::MAX);
        let sql = format!(
            "SELECT mint, {FP_COLUMNS} FROM read_parquet({tokens_lit}) \
             WHERE is_mayhem_mode = false AND created_at >= ? AND created_at < ? \
             ORDER BY created_at DESC"
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query(duckdb::params![after, before])?;
        let mut mints: Vec<String> = Vec::new();
        let mut scanned = 0_u64;
        while let Some(row) = rows.next()? {
            scanned += 1;
            let token_fp = fp_from_row(row, 1)?;
            if hunter_engine::fingerprint::matches(fp, &token_fp) {
                mints.push(row.get(0)?);
            }
        }
        // Rows arrive newest-first, so the clip keeps the newest matches — the same
        // slice semantics as the `LIMIT` in `resolve_candidates`.
        let capped = mints.len() > sel.token_cap;
        mints.truncate(sel.token_cap);
        tracing::info!(
            scanned,
            matched = mints.len(),
            capped,
            "corpus: fingerprint scope resolved on the tokens dimension (no trade scan)"
        );
        Ok((mints, capped))
    }

    /// Synchronous DuckDB corpus assembly (Parquet scan + per-mint windowing +
    /// fingerprint attach) — run on a blocking thread by [`CorpusSource::load`] so
    /// the read never occupies an actix HTTP worker / the async runtime.
    fn load_sync(&self, sel: &Selection) -> Result<Corpus> {
        let (session, trades_lit, tokens_lit) = self.connect()?;
        let conn = &session.conn;

        // 1. Resolve candidate (mint, symbol) — explicit list or newest non-mayhem
        //    tokens in the created window, capped — straight from the token dimension.
        //    `resolve_candidates` also stages `sel_mints` (the ONE staging for the
        //    run); the trade + fingerprint queries below join to it.
        let candidates = resolve_candidates(conn, &tokens_lit, sel)?;
        // Explicit mint lists are caller-sized; only the newest-N dim scan can
        // silently drop older tokens when `LIMIT token_cap` saturates.
        let candidates_capped =
            sel.mints.is_none() && !candidates.is_empty() && candidates.len() >= sel.token_cap;
        if candidates.is_empty() {
            return Ok(Corpus {
                tokens: Vec::new(),
                hash: empty_hash(),
                has_fingerprints: true,
                candidates_capped: false,
            });
        }

        // 2. Per-mint capped, ordered trade pull → per-token slim buffers. The SQL
        //    returns tokens in `mint ASC`; reorder to the candidate order
        //    (`created_at DESC`) so the corpus token order is deterministic
        //    — within-group fold order then matches, down to f64 summation.
        let mut tokens = load_corpus_tokens(conn, &trades_lit, &candidates, sel)?;
        let rank: HashMap<&str, usize> =
            candidates.iter().enumerate().map(|(i, (m, _, _))| (m.as_str(), i)).collect();
        tokens.sort_by_key(|t| rank.get(t.mint.as_str()).copied().unwrap_or(usize::MAX));

        // 3. Attach grouping fingerprints from the token dimension (one query).
        attach_fingerprints(conn, &tokens_lit, &mut tokens)?;

        tracing::info!(
            tokens = tokens.len(),
            trades = tokens.iter().map(|t| t.trades.len()).sum::<usize>(),
            candidates = candidates.len(),
            candidates_capped,
            "corpus: loaded from Parquet lake (DuckDB)"
        );
        Ok(Corpus {
            tokens,
            hash: lake_hash(&self.root, sel),
            has_fingerprints: true,
            candidates_capped,
        })
    }
}

/// Quote a string as a DuckDB SQL literal (single-quote escaped). Lake paths are
/// ours, but quoting keeps a stray apostrophe in a temp path from breaking the SQL.
fn sql_str(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Share of the sweep's own usable RAM (`available − desktop reserve`, read through
/// the ONE sizing SSOT [`crate::sweep::registry::usable_host_bytes`]) that the corpus
/// load may hand to DuckDB's buffer manager. Half: the other half is the corpus we
/// are *building out of* that query — the `CorpusTrade` buffers land in our own heap
/// while DuckDB still holds its scan/window state, so both are resident at the peak.
const DUCKDB_USABLE_SHARE_DIVISOR: u64 = 2;

/// Floor / ceiling (MB) on that share. The floor keeps a tight box able to run the
/// scan at all (DuckDB spills below it rather than failing); the ceiling stops a
/// roomy box from letting a window operator balloon far past what the load needs —
/// the corpus load is 6% of a sweep's wall-clock, so extra buffer pool buys nothing.
const DUCKDB_MEMORY_FLOOR_MB: u64 = 512;
const DUCKDB_MEMORY_CAP_MB: u64 = 4096;

/// One DuckDB connection plus the spill directory it owns, so the two are freed in
/// the right order: **fields drop in declaration order**, so `conn` closes (releasing
/// every temp block file) before `_spill` removes the directory.
struct DuckSession {
    conn: Connection,
    _spill: Option<SpillDir>,
}

/// Parent of every per-connection spill directory. Only ever holds child dirs —
/// DuckDB writes its temp blocks one level down, inside the dir it was handed.
fn spill_root() -> PathBuf {
    std::env::temp_dir().join("hunter-lab-duckdb")
}

/// Monotonic suffix making a spill dir unique within this process; the pid makes it
/// unique across processes.
static SPILL_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A spill directory owned by exactly one DuckDB connection, removed on drop.
///
/// **Why per-connection.** A DuckDB instance treats its `temp_directory` as private:
/// on open it *deletes* the `duckdb_temp_block-*` / `duckdb_temp_storage_*` files it
/// finds there, and it names those files by an internal block id, not by pid. Point
/// two live instances at one directory and the second one's startup wipes the first
/// one's spilled blocks — the first then dies mid-query, in whichever operator next
/// reads a spilled block back. `lab` runs up to `MAX_CONCURRENT_BACKTESTS` (4) loads
/// at once and a grouped sweep alongside them, and this corpus spills at any limit
/// under ~2 GB, so the collision is the normal case, not the rare one. Measured on
/// the bundled DuckDB 1.2.2: two concurrent loads on one shared directory fail in
/// 1.6 s with `Failed to delete file "…duckdb_temp_storage_S160K-0.tmp": being used
/// by another process`, and the racing-delete variant surfaces later, inside the
/// sort's merge, as the opaque `Invalid Error: Unknown exception in Finalize!`.
struct SpillDir(Option<PathBuf>);

impl SpillDir {
    /// Create this connection's own dir under [`spill_root`], or `None` if it can't
    /// be created (the caller then leaves DuckDB's memory limit unset — see
    /// [`bound_duckdb_memory`]).
    fn create() -> Option<Self> {
        let seq = SPILL_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = spill_root().join(format!("{}-{seq}", std::process::id()));
        match std::fs::create_dir_all(&dir) {
            Ok(()) => Some(Self(Some(dir))),
            Err(e) => {
                tracing::warn!(
                    dir = %dir.display(),
                    "corpus load: DuckDB spill dir unavailable ({e}) — leaving the memory \
                     limit unset so an over-budget query spills nowhere rather than erroring"
                );
                None
            }
        }
    }

    fn path(&self) -> &Path {
        self.0.as_deref().expect("spill dir present until drop")
    }
}

impl Drop for SpillDir {
    fn drop(&mut self) {
        if let Some(dir) = self.0.take() {
            if let Err(e) = std::fs::remove_dir_all(&dir) {
                tracing::debug!(dir = %dir.display(), "corpus load: spill dir cleanup failed ({e})");
            }
        }
    }
}

/// Best-effort removal of spill dirs left behind by a crashed/killed process.
///
/// A [`SpillDir`] is removed on drop, so the only leftovers are from a process that
/// died mid-query — and a spilled sort can leave gigabytes. Age-gated rather than
/// pid-gated (a pid is reused): a directory untouched for a day cannot belong to a
/// live load. Never touches a sibling of a *running* connection, and every error is
/// swallowed — this is hygiene, not a precondition.
fn prune_stale_spill_dirs() {
    const STALE_AFTER: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);
    let Ok(entries) = std::fs::read_dir(spill_root()) else { return };
    for e in entries.flatten() {
        let stale = e
            .metadata()
            .and_then(|m| m.modified())
            .map(|t| t.elapsed().map(|age| age > STALE_AFTER).unwrap_or(false))
            .unwrap_or(false);
        if stale {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

/// Bound the connection's buffer manager and give it somewhere to spill.
///
/// **Why this exists.** An unconfigured DuckDB sets `memory_limit` to 80% of
/// *physical* RAM — ~12.8 GB on a 16 GB workstation — and the corpus query is exactly
/// the shape that will take it: a Parquet scan of the whole trades glob feeding a
/// per-mint `ROW_NUMBER` window. That budget is set from hardware, so it knows nothing
/// about the desktop reserve the run was admitted under, nor about the previous run's
/// corpus still resident in `sweep_corpus_cache`. The result is a load phase that can
/// drive the box to 100% before the fold — whose own sizing ladder — has allocated
/// anything at all.
///
/// Bounding it does **not** cost the load: with `temp_directory` set, a query over the
/// limit spills to disk instead of failing, and the scan is IO/CPU-bound rather than
/// buffer-pool-bound. Without a temp directory an over-limit query is an
/// out-of-memory *error*, so the two settings are applied together or not at all.
///
/// Best-effort by design: if the host read or the `SET` fails we leave DuckDB on its
/// default rather than fail a load over a tuning knob — but we say so, because a
/// silently-unbounded load is the condition this function exists to make visible.
fn bound_duckdb_memory(conn: &Connection) -> Option<SpillDir> {
    let Some(usable) = crate::sweep::registry::usable_host_bytes() else {
        tracing::warn!(
            "corpus load: host RAM unreadable — DuckDB left on its default limit \
             (80% of physical RAM); the load phase is unbounded on this platform"
        );
        return None;
    };
    let limit_mb = (usable / DUCKDB_USABLE_SHARE_DIVISOR / (1024 * 1024))
        .clamp(DUCKDB_MEMORY_FLOOR_MB, DUCKDB_MEMORY_CAP_MB);

    // An in-memory database has no database file to derive a spill path from, so the
    // temp directory must be named explicitly for the limit to degrade (spill) rather
    // than fail. Lab-owned dir under the OS temp root — never the lake, which is the
    // immutable dataset this same connection is globbing — and **private to this
    // connection**, since DuckDB wipes the temp dir it is handed (see [`SpillDir`]).
    prune_stale_spill_dirs();
    let spill = SpillDir::create()?;
    let temp_sql = format!(
        "SET temp_directory={};",
        sql_str(&spill.path().to_string_lossy().replace('\\', "/"))
    );

    let sql = format!("SET memory_limit='{limit_mb}MB';{temp_sql}");
    match conn.execute_batch(&sql) {
        Ok(()) => tracing::info!(
            duckdb_memory_limit_mb = limit_mb,
            usable_mb = usable / (1024 * 1024),
            temp_dir = %spill.path().display(),
            "corpus load: DuckDB bounded to its share of the run's usable RAM"
        ),
        Err(e) => tracing::warn!(
            "corpus load: could not bound DuckDB ({e}) — it stays on its default \
             limit of 80% of physical RAM"
        ),
    }
    Some(spill)
}

/// DuckDB partition ordering for the per-mint `ROW_NUMBER` cap. Mirrors the
/// canonical trade-order window exactly — `(slot, tx_index, leg_index,
/// block_time)`. `block_time` is the final tiebreaker because ingest leaves
/// `tx_index`/`leg_index` = 0 for many trades, so the first three columns are NOT a
/// unique order; without `block_time`, Postgres and DuckDB resolve the ties in
/// different relative orders and the sweep replays a different sequence. The 4-tuple
/// IS unique per mint (verified), so both sources walk the identical trade order.
fn partition_order(window: TradeWindow) -> &'static str {
    match window {
        TradeWindow::LaunchWindow => "slot ASC, tx_index ASC, leg_index ASC, block_time ASC",
        TradeWindow::Recent => "slot DESC, tx_index DESC, leg_index DESC, block_time DESC",
    }
}

#[async_trait]
impl CorpusSource for LakeSource {
    async fn load(&self, sel: &Selection) -> Result<Corpus> {
        // The DuckDB read is synchronous + CPU-bound (columnar Parquet scan,
        // per-mint `ROW_NUMBER` windowing, fingerprint join). Run it on a blocking
        // thread so a sweep/simulate load can't pin an actix HTTP worker (or the
        // async runtime) for its whole duration — the longest pre-fold phase, and
        // on `lab` the box isn't the latency-critical path, so the thread hop is free.
        let this = LakeSource::new(self.root.clone());
        let sel = sel.clone();
        tokio::task::spawn_blocking(move || this.load_sync(&sel))
            .await
            .context("DuckDB corpus load task panicked")?
    }
}

/// Hash naming the lake corpus for cache identity — keyed by **(selection, lake
/// version)**. The `lake_version` term (partition set + token-dimension mtime/len)
/// makes the hash change whenever the lake is re-exported, so a `sweep_corpus_cache`
/// keyed on this can be reused across runs of the same selection yet is invalidated
/// the instant a fresh `lake-export` lands — a stale corpus can never be served.
fn lake_hash(root: &Path, sel: &Selection) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    root.to_string_lossy().hash(&mut h);
    sel.token_cap.hash(&mut h);
    sel.per_mint_cap.hash(&mut h);
    (sel.window as u8).hash(&mut h);
    sel.curve_only.hash(&mut h);
    // Distinguishes a signature-populated simulate load from a slim sweep load over
    // the same mints, so a hash-keyed corpus cache can't serve sig-less rows to sim.
    sel.with_signatures.hash(&mut h);
    sel.with_flow.hash(&mut h);
    sel.created_after.map(|t| t.timestamp_millis()).hash(&mut h);
    sel.created_before.map(|t| t.timestamp_millis()).hash(&mut h);
    if let Some(mints) = &sel.mints {
        let mut ms: Vec<&str> = mints.iter().map(String::as_str).collect();
        ms.sort_unstable();
        for m in ms {
            m.hash(&mut h);
        }
    }
    lake_version(root).hash(&mut h);
    format!("lake_{:016x}", h.finish())
}

/// Cheap fingerprint of the lake's on-disk state, folded into [`lake_hash`] so the
/// corpus cache invalidates on re-export. Covers the two things a `lake-export`
/// changes: the **set of day partitions** (a new day adds a `dt=…` dir) and the
/// **token dimension file** (rewritten wholesale each export → its len+mtime move).
/// A filesystem-metadata scan of a few dozen partition dirs — batch-path only (per
/// load / per sweep-start), never a hot path. Errors fold to `0` (miss-safe: two
/// unreadable states hash equal, so at worst a reload).
fn lake_version(root: &Path) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::time::UNIX_EPOCH;
    let mut h = DefaultHasher::new();

    // Sorted partition-dir names (a new/removed day flips this) + each dir's mtime.
    let mut parts: Vec<(String, u64)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(super::trades_dir(root)) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let mtime = e
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            parts.push((name, mtime));
        }
    }
    parts.sort_unstable();
    parts.hash(&mut h);

    // Token dimension: len + mtime (re-exported in full every run).
    if let Ok(m) = std::fs::metadata(tokens_file(root)) {
        m.len().hash(&mut h);
        m.modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0)
            .hash(&mut h);
    }
    h.finish()
}

fn empty_hash() -> String {
    "lake_empty".to_string()
}

/// Candidate `(mint, symbol)` pairs from the token dimension, in load order.
///
/// **Stages `sel_mints` with exactly the returned candidates — the single staging
/// for the run** — so the trade + fingerprint queries can `IN (SELECT mint FROM
/// sel_mints)` and [`LakeSource::load_sync`] never re-stages (the explicit path
/// previously staged the raw `mints`, then `load` staged the capped candidates
/// again — two rebuilds of the same temp table on every simulate read).
fn resolve_candidates(
    conn: &Connection,
    tokens_lit: &str,
    sel: &Selection,
) -> Result<Vec<(String, String, i64)>> {
    if let Some(mints) = &sel.mints {
        // Explicit list: cap to the caller's order first, stage that exact set, then
        // look up symbols against it (staging == candidates, so no re-stage in `load`).
        let capped: Vec<&str> = mints.iter().map(String::as_str).take(sel.token_cap).collect();
        stage_mints(conn, capped.iter().copied())?;
        let sql = format!(
            "SELECT mint, symbol, created_at FROM read_parquet({tokens_lit}) \
             WHERE mint IN (SELECT mint FROM sel_mints)"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, (r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))
            })?
            .collect::<duckdb::Result<Vec<_>>>()?;
        let by_mint: HashMap<String, (String, i64)> = rows.into_iter().collect();
        Ok(capped
            .into_iter()
            .map(|m| {
                let (sym, created) = by_mint.get(m).cloned().unwrap_or_default();
                (m.to_string(), sym, created)
            })
            .collect())
    } else {
        // Microseconds — `export_tokens` writes `created_at` as µs to match PG's
        // timestamptz precision (seconds would create spurious LIMIT-clipping ties).
        let after = sel.created_after.map(|t| t.timestamp_micros()).unwrap_or(i64::MIN);
        let before = sel.created_before.map(|t| t.timestamp_micros()).unwrap_or(i64::MAX);
        let sql = format!(
            "SELECT mint, symbol, created_at FROM read_parquet({tokens_lit}) \
             WHERE is_mayhem_mode = false AND created_at >= ? AND created_at < ? \
             ORDER BY created_at DESC LIMIT ?"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows: Vec<(String, String, i64)> = stmt
            .query_map(
                duckdb::params![after, before, sel.token_cap as i64],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)),
            )?
            .collect::<duckdb::Result<Vec<_>>>()?;
        // Stage the selected mints for the trade + fingerprint queries to join to.
        stage_mints(conn, rows.iter().map(|(m, _, _)| m.as_str()))?;
        Ok(rows)
    }
}

/// (Re)create the `sel_mints` temp table holding the selected mints, for the
/// trade/fp queries to `IN (SELECT mint FROM sel_mints)` against.
fn stage_mints<'a>(conn: &Connection, mints: impl Iterator<Item = &'a str>) -> Result<()> {
    conn.execute_batch("CREATE OR REPLACE TEMP TABLE sel_mints (mint VARCHAR);")?;
    let mut app = conn.appender("sel_mints").context("DuckDB appender for sel_mints")?;
    for m in mints {
        app.append_row(duckdb::params![m])?;
    }
    app.flush()?;
    Ok(())
}

/// One extra day of slack on the [`trade_partition_floor`] bound. A token's
/// `created_at` and its first trade's `block_time` come from different sources, so a
/// sub-second skew across a UTC midnight must not be able to prune the day file the
/// launch trades actually landed in.
const PARTITION_FLOOR_SLACK_DAYS: i64 = 1;

/// The oldest `dt=` day partition a selection's trades can possibly live in, as a
/// `YYYY-MM-DD` literal — or `None` when the selection has no creation floor.
///
/// The lake partitions trades by the **UTC day of `block_time`** (see
/// `export::sealed_days`), and a token's trades all happen at or after its creation.
/// So for a selection bounded by `created_after`, no trade of a selected token can
/// sit in an older partition, and a `dt` predicate prunes those whole Parquet files
/// off the glob without changing a single loaded row. Without it, narrowing the date
/// range narrows only the token dimension while the trade scan still opens every day
/// in the lake.
///
/// **Only the floor is derivable.** `created_before` bounds token *creation*, but a
/// token keeps trading for days afterwards — an upper `dt` bound would silently
/// truncate live histories, which is a correctness bug, not a slower query.
///
/// Contract for an explicit-`mints` selection: passing `created_after` alongside a
/// mint list asserts those mints were themselves selected within that window (true of
/// every caller — the sweep drill-in reuses its run's window, `sim_fetch` sends
/// `None`). A mint list from a *wider* window than `created_after` would lose trades.
fn trade_partition_floor(sel: &Selection) -> Option<String> {
    let after = sel.created_after?;
    let floor = after.date_naive() - chrono::Duration::days(PARTITION_FLOOR_SLACK_DAYS);
    Some(floor.format("%Y-%m-%d").to_string())
}

/// Stream the per-mint capped trades and group them into [`CorpusToken`] — one slim
/// [`CorpusTrade`] buffer per token, the single row type both sweep and simulate walk.
fn load_corpus_tokens(
    conn: &Connection,
    trades_lit: &str,
    candidates: &[(String, String, i64)],
    sel: &Selection,
) -> Result<Vec<CorpusToken>> {
    let symbols: HashMap<&str, &str> =
        candidates.iter().map(|(m, s, _)| (m.as_str(), s.as_str())).collect();
    // mint → creation time (µs epoch) — the metric-clock origin carried onto each
    // `CorpusToken` for the generic engine (simulate-parity, see `CorpusToken`).
    let created: HashMap<&str, i64> =
        candidates.iter().map(|(m, _, c)| (m.as_str(), *c)).collect();
    let order = partition_order(sel.window);
    let curve_filter = if sel.curve_only { "AND t.venue = 'curve'" } else { "" };
    // Hive `dt=` partition floor — prunes whole day files off the glob. See
    // `trade_partition_floor` for why only the floor is derivable.
    let dt_filter = trade_partition_floor(sel)
        .map(|day| format!("AND t.dt >= '{day}' "))
        .unwrap_or_default();
    // Simulate needs `tx_signature` (Solscan links); flow metrics need
    // `ix_labels`/`wallet`. The sweep leaves both out so rows stay slim.
    // Selecting columns only when asked keeps non-flow / non-sim reads from
    // touching the extra strings. `union_by_name=true` null-fills pre-column days.
    let sig_col = if sel.with_signatures { ", tx_signature" } else { "" };
    let flow_cols = if sel.with_flow { ", ix_labels, wallet" } else { "" };

    // The one projection both shapes below emit — the exact column order the row
    // reader decodes by ordinal.
    let projection = format!(
        "mint, is_buy, sol_amount, token_amount, price, slot, block_time, leg_index, \
         vsol, vtok, venue{sig_col}{flow_cols}"
    );
    // Restores execution order so a token's legs arrive contiguous + chronological
    // (single-pass group in the loop below).
    let row_order = "mint ASC, slot ASC, tx_index ASC, leg_index ASC, block_time ASC";

    // An **uncapped** per-mint selection (the analysis default — see
    // `SWEEP_DEFAULT_PER_MINT_CAP`) makes `rn <= ?` a no-op, so the `ROW_NUMBER`
    // window computes a rank nothing reads. Worse, that window sorts on
    // `(mint, slot, tx_index, leg_index, block_time)` — the SAME key `row_order`
    // needs — so keeping it sorts the whole corpus twice. Emit the plain scan
    // instead; the capped shape below is unchanged, since there the window is what
    // does the clipping.
    let capped = sel.per_mint_cap != SWEEP_DEFAULT_PER_MINT_CAP;
    let sql = if capped {
        // ROW_NUMBER window caps each mint's slice.
        format!(
            "WITH ranked AS ( \
                SELECT t.mint, t.is_buy, t.sol_amount, t.token_amount, t.price, \
                       t.slot, t.tx_index, t.block_time, t.leg_index, t.vsol, t.vtok, t.venue{sig_col}{flow_cols}, \
                       ROW_NUMBER() OVER (PARTITION BY t.mint ORDER BY {order}) AS rn \
                FROM read_parquet({trades_lit}, hive_partitioning=true, union_by_name=true) t \
                WHERE t.mint IN (SELECT mint FROM sel_mints) {curve_filter} {dt_filter}\
             ) \
             SELECT {projection} \
             FROM ranked WHERE rn <= ? \
             ORDER BY {row_order}"
        )
    } else {
        format!(
            "SELECT {projection} \
             FROM read_parquet({trades_lit}, hive_partitioning=true, union_by_name=true) t \
             WHERE t.mint IN (SELECT mint FROM sel_mints) {curve_filter} {dt_filter}\
             ORDER BY {row_order}"
        )
    };

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = if capped {
        stmt.query(duckdb::params![sel.per_mint_cap])?
    } else {
        stmt.query([])?
    };

    let mut tokens: Vec<CorpusToken> = Vec::with_capacity(candidates.len());
    let mut cur_mint: Option<String> = None;
    let mut cur_trades: Vec<CorpusTrade> = Vec::new();

    while let Some(row) = rows.next()? {
        let mint: String = row.get(0)?;
        let is_buy: bool = row.get(1)?;
        let amount_sol: f64 = row.get(2)?;
        let token_amount: f64 = row.get(3)?;
        let price: f64 = row.get(4)?;
        let slot: i64 = row.get(5)?;
        let block_time: i64 = row.get(6)?;
        let leg_index: i32 = row.get(7)?;
        let vsol: Option<f64> = row.get(8)?;
        let vtok: Option<f64> = row.get(9)?;
        let venue: String = row.get(10)?;
        // Optional trailing columns: signature then flow — ordinal advances only
        // when the matching Selection flag requested them.
        let mut col = 11usize;
        let tx_signature: Option<Box<str>> = if sel.with_signatures {
            let v = row.get::<_, Option<String>>(col)?.map(String::into_boxed_str);
            col += 1;
            v
        } else {
            None
        };
        let (ix_labels, wallet) = if sel.with_flow {
            let ix = row.get::<_, Option<String>>(col)?.map(String::into_boxed_str);
            col += 1;
            let w = row.get::<_, Option<String>>(col)?.map(String::into_boxed_str);
            (ix, w)
        } else {
            (None, None)
        };

        if cur_mint.as_deref() != Some(mint.as_str()) {
            if let Some(prev) = cur_mint.take() {
                push_token(&mut tokens, prev, &symbols, &created, &mut cur_trades);
            }
            cur_mint = Some(mint.clone());
        }
        cur_trades.push(CorpusTrade {
            block_time: micros_to_utc(block_time),
            amount_sol,
            token_amount,
            price_per_token: price,
            reserve_sol: vsol,
            reserve_token: vtok,
            // The program-emitted `real_*_reserves` aren't in the `trades` table
            // (dropped, re-derivable from raw_txs), so the lake **approximates**
            // real SOL from the priced reserve pair per venue: AMM → reserve_sol,
            // curve → reserve_sol − 30 (the initial virtual SOL), clamped at 0.
            // Same "true liquidity" the frontend chart shows; lets the sim's
            // real-reserve gates (e.g. tpsl2 `min_liq_sol`) resolve. This is an
            // approximation of the live/paper value, not lamport-identical.
            // `real_token_reserves` stays None — no configured gate reads it.
            real_reserve_sol: vsol.map(|s| approx_real_sol_reserves(s, &venue)),
            real_token_reserves: None,
            slot: slot as u64,
            leg_index: leg_index as u32,
            is_buy,
            tx_signature,
            ix_labels,
            wallet,
        });
    }
    if let Some(prev) = cur_mint.take() {
        push_token(&mut tokens, prev, &symbols, &created, &mut cur_trades);
    }
    Ok(tokens)
}

/// Close out one token's accumulated buffer into a [`CorpusToken`], resetting the
/// per-token trade buffer for the next mint.
fn push_token(
    out: &mut Vec<CorpusToken>,
    mint: String,
    symbols: &HashMap<&str, &str>,
    created: &HashMap<&str, i64>,
    trades: &mut Vec<CorpusTrade>,
) {
    let symbol = symbols.get(mint.as_str()).copied().unwrap_or_default().to_string();
    // Fall back to the first trade's block time if the token row is missing a
    // creation stamp (should never happen — every candidate carries one).
    let created_at = created
        .get(mint.as_str())
        .copied()
        .map(micros_to_utc)
        .unwrap_or_else(|| trades.first().map(|t| t.block_time).unwrap_or_else(Utc::now));
    out.push(CorpusToken {
        symbol,
        mint,
        created_at,
        trades: Arc::new(std::mem::take(trades)),
        fp: TokenFingerprint::default(),
    });
}

/// The tokens-dimension fingerprint columns, in the ONE order [`fp_from_row`] decodes
/// them by ordinal. Both fingerprint readers (the post-load attach and the
/// pre-load [`LakeSource::matching_mints`] scan) select exactly this list, so a column
/// added here can never be decoded at a stale offset by the other.
const FP_COLUMNS: &str = "fp_token_program_id, fp_initial_buy_sol, \
     fp_cu_limit, fp_cu_price, fp_is_cashback_enabled, fp_max_sol_cost, \
     fp_spendable_sol_in, fp_first_slot_buy_sol, fp_first_slot_sell_sol, fp_ix_labels";

/// Decode a [`FP_COLUMNS`] block starting at ordinal `base` into a grouping
/// [`TokenFingerprint`].
fn fp_from_row(row: &duckdb::Row<'_>, base: usize) -> duckdb::Result<TokenFingerprint> {
    let ix_labels_json: Option<String> = row.get(base + 9)?;
    Ok(TokenFingerprint {
        token_program_id: row.get(base)?,
        initial_buy_sol: row.get(base + 1)?,
        cu_limit: row.get(base + 2)?,
        cu_price: row.get(base + 3)?,
        is_cashback_enabled: row.get::<_, Option<bool>>(base + 4)?.unwrap_or(false),
        max_cost_lamports: row.get(base + 5)?,
        spendable_lamports_in: row.get(base + 6)?,
        first_slot_buy_sol: row.get(base + 7)?,
        first_slot_sell_sol: row.get(base + 8)?,
        ix_labels: ix_labels_json
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default(),
    })
}

/// Read the grouping [`TokenFingerprint`] for every staged mint from the `tokens`
/// dimension into a `mint → fp` map (one query), for [`attach_fingerprints`]. Relies
/// on `sel_mints` being staged by the caller.
fn load_fingerprints(conn: &Connection, tokens_lit: &str) -> Result<HashMap<String, TokenFingerprint>> {
    let sql = format!(
        "SELECT mint, {FP_COLUMNS} \
         FROM read_parquet({tokens_lit}) WHERE mint IN (SELECT mint FROM sel_mints)"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([])?;
    let mut by_mint: HashMap<String, TokenFingerprint> = HashMap::new();
    while let Some(row) = rows.next()? {
        let mint: String = row.get(0)?;
        by_mint.insert(mint, fp_from_row(row, 1)?);
    }
    Ok(by_mint)
}

/// Attach the grouping [`TokenFingerprint`] (+ nothing else) to each token from the
/// dimension file, mirroring [`crate::sweep::corpus::attach_fingerprints`].
fn attach_fingerprints(conn: &Connection, tokens_lit: &str, tokens: &mut [CorpusToken]) -> Result<()> {
    let by_mint = load_fingerprints(conn, tokens_lit)?;
    let mut missing = 0u64;
    for tt in tokens.iter_mut() {
        match by_mint.get(&tt.mint) {
            Some(fp) => tt.fp = fp.clone(),
            None => missing += 1,
        }
    }
    if missing > 0 {
        tracing::warn!(missing, "lake attach_fingerprints: some corpus tokens had no tokens-dim row");
    }
    Ok(())
}

/// Epoch **microseconds** → `DateTime<Utc>` (falls back to now on the impossible
/// overflow). Matches `export.rs`, which writes `block_time` as µs to preserve PG's
/// timestamptz precision for the sweep's time gates.
fn micros_to_utc(micros: i64) -> DateTime<Utc> {
    DateTime::<Utc>::from_timestamp_micros(micros).unwrap_or_else(Utc::now)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sql_str_escapes_quotes() {
        assert_eq!(sql_str("/a/b"), "'/a/b'");
        assert_eq!(sql_str("o'brien"), "'o''brien'");
    }

    #[test]
    fn partition_order_matches_window() {
        assert!(partition_order(TradeWindow::LaunchWindow).contains("ASC"));
        assert!(partition_order(TradeWindow::Recent).contains("DESC"));
    }

    #[test]
    fn partition_floor_is_a_day_before_the_creation_bound() {
        let sel = Selection {
            created_after: Some(
                DateTime::parse_from_rfc3339("2026-08-03T00:00:01Z").unwrap().with_timezone(&Utc),
            ),
            ..Selection::default()
        };
        // A day of slack: the floor must never land ON the creation day, or a
        // sub-second skew between `created_at` and the first trade's `block_time`
        // across UTC midnight could prune the launch trades' own partition.
        assert_eq!(trade_partition_floor(&sel).as_deref(), Some("2026-08-02"));
    }

    /// `created_before` bounds token *creation*, not trading — a token keeps trading
    /// for days after it launches, so there is no derivable upper `dt` bound and an
    /// unbounded-below selection gets no predicate at all.
    #[test]
    fn partition_floor_is_absent_without_a_creation_floor() {
        let sel = Selection {
            created_after: None,
            created_before: Some(Utc::now()),
            ..Selection::default()
        };
        assert_eq!(trade_partition_floor(&sel), None);
    }

    /// Two live connections must never be handed the same `temp_directory`: DuckDB
    /// wipes the dir it is given on open, so a shared one lets a starting load
    /// destroy a running load's spilled blocks (the `Unknown exception in Finalize!`
    /// / `duckdb_temp_storage_*.tmp is being used by another process` failures). Also
    /// pins the removal-on-drop, since a spilled sort leaves gigabytes behind.
    #[test]
    fn each_connection_gets_its_own_spill_dir() {
        let (a, b) = (SpillDir::create().expect("spill a"), SpillDir::create().expect("spill b"));
        let (pa, pb) = (a.path().to_path_buf(), b.path().to_path_buf());
        assert_ne!(pa, pb, "concurrent connections must not share a DuckDB temp dir");
        assert!(pa.is_dir() && pb.is_dir(), "both spill dirs must exist while held");
        drop(a);
        assert!(!pa.exists(), "spill dir must be removed on drop");
        assert!(pb.is_dir(), "dropping one connection must not touch another's spill dir");
        drop(b);
    }

    #[test]
    fn missing_lake_is_a_clear_error() {
        let src = LakeSource::new(std::env::temp_dir().join("definitely-no-lake-here-xyz"));
        // `map(drop)` because a `DuckSession` (a live DuckDB handle) isn't `Debug`.
        let err = src.connect().map(drop).unwrap_err().to_string();
        assert!(err.contains("lake"), "expected a lake-not-found message, got: {err}");
    }

    /// Every column name the reader `SELECT`s / reads must exist in the writer's
    /// canonical column set ([`crate::lake::schema`]). DuckDB matches columns by name,
    /// so a writer rename that isn't mirrored here would otherwise surface only as a
    /// runtime "column not found" against a populated lake — this pins it at build time.
    #[test]
    fn reader_columns_are_canonical() {
        use crate::lake::schema::{TOKEN_WRITE_COLS, TRADE_WRITE_COLS};
        // Columns the trade query references (outer SELECT + the `tx_index` ordering key).
        for c in [
            "mint", "is_buy", "sol_amount", "token_amount", "price", "slot", "block_time",
            "leg_index", "vsol", "vtok", "venue", "tx_index", "tx_signature",
        ] {
            assert!(TRADE_WRITE_COLS.contains(&c), "reader trade col `{c}` is not a canonical write column");
        }
        // Columns the candidate + fingerprint queries reference.
        for c in [
            "mint", "symbol", "fp_token_program_id", "fp_initial_buy_sol", "fp_cu_limit",
            "fp_cu_price", "fp_is_cashback_enabled", "fp_max_sol_cost", "fp_spendable_sol_in",
            "fp_first_slot_buy_sol", "fp_first_slot_sell_sol", "fp_ix_labels", "is_mayhem_mode",
            "created_at",
        ] {
            assert!(TOKEN_WRITE_COLS.contains(&c), "reader token col `{c}` is not a canonical write column");
        }
    }
}

/// Simulate ↔ sweep parity — the guarantee the migration exists to make: a rule
/// prices identically whether swept or drilled into. Both now load ONE row type
/// ([`CorpusTrade`]) through ONE loader; the sole difference is the display-only
/// `with_signatures` flag. This test pins that the flag populates `tx_signature` and
/// changes **nothing else** — so simulate can never diverge from the sweep on any
/// decision field. `--ignored` because it needs a populated `$SWEEP_LAKE_DIR`; the
/// analogue of `token_repo::parity_tests` for the two token-list engines.
///
/// [`CorpusTrade`]: crate::sweep::projection::CorpusTrade
#[cfg(test)]
mod parity_tests {
    use super::*;
    use crate::lake::lake_root;
    use crate::sweep::corpus::{CorpusSource, Selection};
    use trading_core::models::trade::TradeRow;

    /// Exact-bits compare for an `Option<f64>` so a reserve that must be identical
    /// can't slip through on a rounding-equal-but-not-bit-equal value.
    fn opt_bits(v: Option<f64>) -> Option<u64> {
        v.map(f64::to_bits)
    }

    /// Start of the newest **sealed** `dt=` partition — the `created_before` bound
    /// every lake test here needs.
    ///
    /// `export_tokens` rewrites the token dimension in full on each run, so it carries
    /// tokens created *today*, whose trades are still in the open day and therefore
    /// live in no partition at all. `resolve_candidates` orders `created_at DESC`, so
    /// an unbounded test selection picks exactly those first and loads a corpus of N
    /// tokens with zero trades — the assertions then fail on empty input rather than
    /// on the behaviour under test.
    fn sealed_created_before(root: &Path) -> Option<DateTime<Utc>> {
        use chrono::TimeZone;
        let mut days: Vec<String> = std::fs::read_dir(super::super::trades_dir(root))
            .ok()?
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                name.strip_prefix("dt=").map(str::to_string)
            })
            .collect();
        days.sort();
        let day = chrono::NaiveDate::parse_from_str(days.last()?, "%Y-%m-%d").ok()?;
        Some(Utc.from_utc_datetime(&day.and_hms_opt(0, 0, 0)?))
    }

    // NOT `#[ignore]`: self-skips when no lake is present (keyless CI stays green) but
    // runs automatically once `$SWEEP_LAKE_DIR` points at a populated lake, so the
    // sim↔sweep parity is enforced without an `--ignored` opt-in.
    #[tokio::test]
    async fn signature_flag_changes_only_the_signature() {
        let root = lake_root();
        if !tokens_file(&root).exists() || !super::super::trades_dir(&root).exists() {
            eprintln!("skipping sim/sweep parity: no lake at {}", root.display());
            return;
        }

        // Same selection but for the display-only `with_signatures` flag: the sweep
        // read (false) and the simulate read (true) must agree on every row and every
        // decision field, differing ONLY in the populated `tx_signature`.
        let base = Selection {
            mints: None,
            token_cap: 200,
            created_after: None,
            // Sealed days only — see `sealed_created_before`.
            created_before: sealed_created_before(&root),
            per_mint_cap: 5_000,
            window: TradeWindow::LaunchWindow,
            curve_only: false,
            with_signatures: false,
            with_flow: false,
        };
        let with_sigs = Selection { with_signatures: true, ..base.clone() };

        let src = LakeSource::new(root);
        let sweep = src.load(&base).await.expect("sweep corpus load");
        let sim = src.load(&with_sigs).await.expect("sim corpus load");

        assert_eq!(sweep.tokens.len(), sim.tokens.len(), "token count must match");
        assert!(!sim.tokens.is_empty(), "lake returned no tokens — populate it first");

        let mut any_sig = false;
        for (st, mt) in sweep.tokens.iter().zip(&sim.tokens) {
            assert_eq!(st.mint, mt.mint, "token order must match");
            assert_eq!(st.trades.len(), mt.trades.len(), "trade count for {}", st.mint);

            for (a, b) in st.trades.iter().zip(mt.trades.iter()) {
                // Every decision field bit-identical, so any `TradeRow`-generic
                // resolver replays identically. Only `tx_signature` may differ.
                assert_eq!(a.is_buy(), b.is_buy(), "is_buy {}", st.mint);
                assert_eq!(a.slot(), b.slot(), "slot {}", st.mint);
                assert_eq!(a.leg_index(), b.leg_index(), "leg_index {}", st.mint);
                assert_eq!(a.block_time(), b.block_time(), "block_time {}", st.mint);
                assert_eq!(a.amount_sol().to_bits(), b.amount_sol().to_bits(), "amount_sol {}", st.mint);
                assert_eq!(a.token_amount().to_bits(), b.token_amount().to_bits(), "token_amount {}", st.mint);
                assert_eq!(a.price_per_token().to_bits(), b.price_per_token().to_bits(), "price {}", st.mint);
                assert_eq!(opt_bits(a.reserve_sol()), opt_bits(b.reserve_sol()), "reserve_sol {}", st.mint);
                assert_eq!(opt_bits(a.reserve_token()), opt_bits(b.reserve_token()), "reserve_token {}", st.mint);
                assert_eq!(opt_bits(a.real_reserve_sol()), opt_bits(b.real_reserve_sol()), "real_reserve_sol {}", st.mint);

                // The sweep row never carries a signature; the sim row carries one on
                // any day exported after the schema change (pre-migration days null-fill).
                assert!(a.tx_signature.is_none(), "sweep row must stay signature-free ({})", st.mint);
                any_sig |= b.tx_signature.is_some();
            }
        }
        assert!(any_sig, "simulate load populated no signatures — is the lake re-exported with tx_signature?");
    }

    /// Assert two corpora are the same tokens in the same order carrying bit-identical
    /// trades. Shared by the two query-shape tests below, which each change *how* the
    /// trades are read and must change nothing about *what* is read.
    fn assert_same_corpus(a: &Corpus, b: &Corpus, what: &str) {
        assert_eq!(a.tokens.len(), b.tokens.len(), "{what}: token count");
        assert!(!a.tokens.is_empty(), "{what}: lake returned no tokens — populate it first");
        for (ta, tb) in a.tokens.iter().zip(&b.tokens) {
            assert_eq!(ta.mint, tb.mint, "{what}: token order");
            assert_eq!(ta.trades.len(), tb.trades.len(), "{what}: trade count for {}", ta.mint);
            for (x, y) in ta.trades.iter().zip(tb.trades.iter()) {
                assert_eq!(x.slot, y.slot, "{what}: slot {}", ta.mint);
                assert_eq!(x.leg_index, y.leg_index, "{what}: leg_index {}", ta.mint);
                assert_eq!(x.block_time, y.block_time, "{what}: block_time {}", ta.mint);
                assert_eq!(x.is_buy, y.is_buy, "{what}: is_buy {}", ta.mint);
                assert_eq!(
                    x.amount_sol.to_bits(),
                    y.amount_sol.to_bits(),
                    "{what}: amount_sol {}",
                    ta.mint
                );
            }
        }
    }

    /// Dropping the `ROW_NUMBER` CTE on an uncapped selection must be a pure query-plan
    /// change. `i64::MAX - 1` still clips nothing, so it takes the windowed path and
    /// yields the reference rows the plain scan has to reproduce exactly — order
    /// included, since the fold's f64 summation is order-sensitive.
    #[tokio::test]
    async fn uncapped_plain_scan_matches_the_windowed_scan() {
        let root = lake_root();
        if !tokens_file(&root).exists() || !super::super::trades_dir(&root).exists() {
            eprintln!("skipping uncapped-scan parity: no lake at {}", root.display());
            return;
        }
        let windowed = Selection {
            mints: None,
            token_cap: 100,
            created_after: None,
            created_before: sealed_created_before(&root),
            // One below the sentinel: still a no-op clip, but takes the CTE path.
            per_mint_cap: SWEEP_DEFAULT_PER_MINT_CAP - 1,
            window: TradeWindow::LaunchWindow,
            curve_only: false,
            with_signatures: false,
            with_flow: true,
        };
        let plain = Selection { per_mint_cap: SWEEP_DEFAULT_PER_MINT_CAP, ..windowed.clone() };

        let src = LakeSource::new(root);
        let a = src.load(&windowed).await.expect("windowed corpus load");
        let b = src.load(&plain).await.expect("plain corpus load");
        assert_same_corpus(&a, &b, "uncapped scan");
    }

    /// The `dt=` partition floor must prune only files that cannot hold a selected
    /// token's trades. Pinning the token set with an explicit mint list (which makes
    /// `resolve_candidates` ignore `created_after`) isolates the predicate: the only
    /// difference between the two loads is whether the floor is applied, so any
    /// dropped trade shows up as a mismatch.
    #[tokio::test]
    async fn partition_floor_drops_no_trades() {
        let root = lake_root();
        if !tokens_file(&root).exists() || !super::super::trades_dir(&root).exists() {
            eprintln!("skipping partition-floor parity: no lake at {}", root.display());
            return;
        }
        let sealed_before = sealed_created_before(&root);
        let src = LakeSource::new(root);
        let probe = Selection {
            mints: None,
            token_cap: 100,
            created_after: None,
            created_before: sealed_before,
            per_mint_cap: SWEEP_DEFAULT_PER_MINT_CAP,
            window: TradeWindow::LaunchWindow,
            curve_only: false,
            with_signatures: false,
            with_flow: true,
        };
        let seed = src.load(&probe).await.expect("probe corpus load");
        assert!(!seed.tokens.is_empty(), "lake returned no tokens — populate it first");

        let mints: Vec<String> = seed.tokens.iter().map(|t| t.mint.clone()).collect();
        // The oldest creation in the set — the tightest floor that is still legal for
        // every one of these mints, i.e. the worst case for over-pruning.
        let oldest = seed.tokens.iter().map(|t| t.created_at).min().expect("non-empty");

        let unpruned = Selection { mints: Some(mints), ..probe };
        let pruned = Selection { created_after: Some(oldest), ..unpruned.clone() };
        assert!(
            trade_partition_floor(&pruned).is_some(),
            "the pruned selection must actually carry a dt floor, or this proves nothing"
        );

        let a = src.load(&unpruned).await.expect("unpruned corpus load");
        let b = src.load(&pruned).await.expect("pruned corpus load");
        assert_same_corpus(&a, &b, "partition floor");
    }
}
