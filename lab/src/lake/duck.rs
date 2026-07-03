//! DuckDB corpus source (hop 3) — assemble a sweep [`Corpus`] by querying the
//! Parquet lake instead of Postgres.
//!
//! This is the lake-fed analogue of [`crate::sweep::corpus::DbSource`]: same output
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
//! Runtime-unverified this session (no lake to read). The SQL mirrors the verified
//! `DbSource` shape; validation against a PG-sourced baseline is the Phase-4
//! "done-when" gate (needs the DB/EC2 pipeline).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use duckdb::Connection;

use trading_core::config::constants::approx_real_sol_reserves;

use crate::sweep::corpus::{Corpus, CorpusSource, Selection, TradeWindow};
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

    /// Open an in-memory DuckDB and confirm the lake has both datasets. A missing
    /// lake is a clear error (run the export first) rather than DuckDB's opaque
    /// "No files found".
    fn connect(&self) -> Result<(Connection, String, String)> {
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
        let tokens_lit = sql_str(&tokens.to_string_lossy().replace('\\', "/"));
        let trades_lit = sql_str(&trades_glob(&self.root));
        Ok((conn, trades_lit, tokens_lit))
    }
}

/// Quote a string as a DuckDB SQL literal (single-quote escaped). Lake paths are
/// ours, but quoting keeps a stray apostrophe in a temp path from breaking the SQL.
fn sql_str(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// DuckDB partition ordering for the per-mint `ROW_NUMBER` cap. Mirrors
/// [`DbSource`](crate::sweep::corpus)'s window exactly — `(slot, tx_index, leg_index,
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
        // DuckDB work is synchronous/CPU-bound; `lab` runs it from a batch context
        // (not a hot server path), so executing inline is acceptable.
        let (conn, trades_lit, tokens_lit) = self.connect()?;

        // 1. Resolve candidate (mint, symbol) — explicit list or newest non-mayhem
        //    tokens in the created window, capped — straight from the token dimension.
        let candidates = resolve_candidates(&conn, &tokens_lit, sel)?;
        if candidates.is_empty() {
            return Ok(Corpus { tokens: Vec::new(), hash: empty_hash(), has_fingerprints: true });
        }

        // 2. Stage the selected mints in a temp table the trade + fp queries join to.
        stage_mints(&conn, candidates.iter().map(|(m, _)| m.as_str()))?;

        // 3. Per-mint capped, ordered trade pull → per-token slim buffers. The SQL
        //    returns tokens in `mint ASC`; reorder to the candidate order
        //    (`created_at DESC`) so the corpus token order matches `DbSource` exactly
        //    — within-group fold order then matches, down to f64 summation.
        let mut tokens = load_corpus_tokens(&conn, &trades_lit, &candidates, sel)?;
        let rank: HashMap<&str, usize> =
            candidates.iter().enumerate().map(|(i, (m, _))| (m.as_str(), i)).collect();
        tokens.sort_by_key(|t| rank.get(t.mint.as_str()).copied().unwrap_or(usize::MAX));

        // 4. Attach grouping fingerprints from the token dimension (one query).
        attach_fingerprints(&conn, &tokens_lit, &mut tokens)?;

        tracing::info!(
            tokens = tokens.len(),
            trades = tokens.iter().map(|t| t.trades.len()).sum::<usize>(),
            "corpus: loaded from Parquet lake (DuckDB)"
        );
        Ok(Corpus { tokens, hash: lake_hash(&self.root, sel), has_fingerprints: true })
    }
}

/// Stable-ish hash naming the lake corpus for cache/log identity (the lake itself is
/// the durable store, so this need only disambiguate selections within a run).
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
    sel.created_after.map(|t| t.timestamp_millis()).hash(&mut h);
    sel.created_before.map(|t| t.timestamp_millis()).hash(&mut h);
    if let Some(mints) = &sel.mints {
        let mut ms: Vec<&str> = mints.iter().map(String::as_str).collect();
        ms.sort_unstable();
        for m in ms {
            m.hash(&mut h);
        }
    }
    format!("lake_{:016x}", h.finish())
}

fn empty_hash() -> String {
    "lake_empty".to_string()
}

/// Candidate `(mint, symbol)` pairs from the token dimension.
fn resolve_candidates(
    conn: &Connection,
    tokens_lit: &str,
    sel: &Selection,
) -> Result<Vec<(String, String)>> {
    if let Some(mints) = &sel.mints {
        // Explicit list: look up symbols, preserve the caller's order + cap.
        stage_mints(conn, mints.iter().map(String::as_str))?;
        let sql = format!(
            "SELECT mint, symbol FROM read_parquet({tokens_lit}) \
             WHERE mint IN (SELECT mint FROM sel_mints)"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<duckdb::Result<Vec<_>>>()?;
        let by_mint: HashMap<String, String> = rows.into_iter().collect();
        Ok(mints
            .iter()
            .take(sel.token_cap)
            .map(|m| (m.clone(), by_mint.get(m).cloned().unwrap_or_default()))
            .collect())
    } else {
        // Microseconds — `export_tokens` writes `created_at` as µs to match PG's
        // timestamptz precision (seconds would create spurious LIMIT-clipping ties).
        let after = sel.created_after.map(|t| t.timestamp_micros()).unwrap_or(i64::MIN);
        let before = sel.created_before.map(|t| t.timestamp_micros()).unwrap_or(i64::MAX);
        let sql = format!(
            "SELECT mint, symbol FROM read_parquet({tokens_lit}) \
             WHERE is_mayhem_mode = false AND created_at >= ? AND created_at < ? \
             ORDER BY created_at DESC LIMIT ?"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(
                duckdb::params![after, before, sel.token_cap as i64],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )?
            .collect::<duckdb::Result<Vec<_>>>()?;
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

/// Stream the per-mint capped trades and group them into [`CorpusToken`] — one slim
/// [`CorpusTrade`] buffer per token, the single row type both sweep and simulate walk.
fn load_corpus_tokens(
    conn: &Connection,
    trades_lit: &str,
    candidates: &[(String, String)],
    sel: &Selection,
) -> Result<Vec<CorpusToken>> {
    let symbols: HashMap<&str, &str> =
        candidates.iter().map(|(m, s)| (m.as_str(), s.as_str())).collect();
    let order = partition_order(sel.window);
    let curve_filter = if sel.curve_only { "AND t.venue = 'curve'" } else { "" };
    // Simulate needs `tx_signature` (Solscan links); the sweep leaves it out so its
    // rows stay slim (the trigger is resolved by index, not signature). Selecting the
    // column only when asked keeps the sweep read from touching the ~88 B/row string.
    let sig_col = if sel.with_signatures { ", tx_signature" } else { "" };

    // ROW_NUMBER window caps each mint's slice; outer ORDER BY restores execution
    // order so a token's legs arrive contiguous + chronological (single-pass group).
    // `union_by_name=true`: `tx_signature` exists only on files exported after the
    // Stage-1 schema change. Without name-union, DuckDB's positional glob-read errors
    // on the mixed old/new schema until the full re-export makes every day uniform;
    // name-union tolerates the gap (a pre-column day null-fills → empty signature).
    let sql = format!(
        "WITH ranked AS ( \
            SELECT t.mint, t.is_buy, t.sol_amount, t.token_amount, t.price, \
                   t.slot, t.tx_index, t.block_time, t.leg_index, t.vsol, t.vtok, t.venue{sig_col}, \
                   ROW_NUMBER() OVER (PARTITION BY t.mint ORDER BY {order}) AS rn \
            FROM read_parquet({trades_lit}, hive_partitioning=true, union_by_name=true) t \
            WHERE t.mint IN (SELECT mint FROM sel_mints) {curve_filter} \
         ) \
         SELECT mint, is_buy, sol_amount, token_amount, price, slot, block_time, leg_index, vsol, vtok, venue{sig_col} \
         FROM ranked WHERE rn <= ? \
         ORDER BY mint ASC, slot ASC, tx_index ASC, leg_index ASC, block_time ASC"
    );

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(duckdb::params![sel.per_mint_cap])?;

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
        // Column 11 only exists in the SELECT when `with_signatures`; a NULL from a
        // pre-migration (union_by_name) day → `None` → empty display link.
        let tx_signature: Option<Box<str>> = if sel.with_signatures {
            row.get::<_, Option<String>>(11)?.map(String::into_boxed_str)
        } else {
            None
        };

        if cur_mint.as_deref() != Some(mint.as_str()) {
            if let Some(prev) = cur_mint.take() {
                push_token(&mut tokens, prev, &symbols, &mut cur_trades);
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
        });
    }
    if let Some(prev) = cur_mint.take() {
        push_token(&mut tokens, prev, &symbols, &mut cur_trades);
    }
    Ok(tokens)
}

/// Close out one token's accumulated buffer into a [`CorpusToken`], resetting the
/// per-token trade buffer for the next mint.
fn push_token(
    out: &mut Vec<CorpusToken>,
    mint: String,
    symbols: &HashMap<&str, &str>,
    trades: &mut Vec<CorpusTrade>,
) {
    let symbol = symbols.get(mint.as_str()).copied().unwrap_or_default().to_string();
    out.push(CorpusToken {
        symbol,
        mint,
        trades: Arc::new(std::mem::take(trades)),
        fp: TokenFingerprint::default(),
    });
}

/// Read the grouping [`TokenFingerprint`] for every staged mint from the `tokens`
/// dimension into a `mint → fp` map (one query), for [`attach_fingerprints`]. Relies
/// on `sel_mints` being staged by the caller.
fn load_fingerprints(conn: &Connection, tokens_lit: &str) -> Result<HashMap<String, TokenFingerprint>> {
    let sql = format!(
        "SELECT mint, fp_token_program_id, fp_initial_buy_sol, \
                fp_cu_limit, fp_cu_price, fp_is_cashback_enabled, fp_max_sol_cost, \
                fp_spendable_sol_in, fp_first_slot_buy_sol, fp_first_slot_sell_sol, fp_ix_labels \
         FROM read_parquet({tokens_lit}) WHERE mint IN (SELECT mint FROM sel_mints)"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([])?;
    let mut by_mint: HashMap<String, TokenFingerprint> = HashMap::new();
    while let Some(row) = rows.next()? {
        let mint: String = row.get(0)?;
        let ix_labels_json: Option<String> = row.get(10)?;
        by_mint.insert(
            mint,
            TokenFingerprint {
                token_program_id: row.get(1)?,
                initial_buy_sol: row.get(2)?,
                cu_limit: row.get(3)?,
                cu_price: row.get(4)?,
                is_cashback_enabled: row.get::<_, Option<bool>>(5)?.unwrap_or(false),
                max_cost_lamports: row.get(6)?,
                spendable_lamports_in: row.get(7)?,
                first_slot_buy_sol: row.get(8)?,
                first_slot_sell_sol: row.get(9)?,
                ix_labels: ix_labels_json
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or_default(),
            },
        );
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
    fn missing_lake_is_a_clear_error() {
        let src = LakeSource::new(std::env::temp_dir().join("definitely-no-lake-here-xyz"));
        let err = src.connect().unwrap_err().to_string();
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
            created_before: None,
            per_mint_cap: 5_000,
            window: TradeWindow::LaunchWindow,
            curve_only: false,
            with_signatures: false,
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
}
