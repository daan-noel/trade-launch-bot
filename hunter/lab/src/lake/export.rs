//! PG → Parquet lake export (hop 2). Reads sealed days out of the local `trades`
//! table and writes one immutable Parquet file per day, plus a `tokens` dimension.
//!
//! Streaming + row-group flushing keeps peak memory bounded regardless of a day's
//! trade count (a busy day is millions of legs): rows are pulled with `fetch()` and
//! flushed to a Parquet row group every [`FLUSH_ROWS`].

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use arrow::array::{
    BooleanBuilder, Float64Builder, Int32Builder, Int64Builder, StringBuilder,
};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use futures_util::TryStreamExt;
use parquet::arrow::ArrowWriter;
use sqlx::PgPool;

use serde::{Deserialize, Serialize};

use trading_core::grouping::{extract_lamports, normalize_labels};
use trading_core::storage::repositories::trade_repo::sig_bytes_to_base58;

use super::{tokens_file, trades_day_file, trades_day_meta};

/// Flush a Parquet row group roughly every this-many rows to bound peak builder RAM
/// (mirrors the corpus-cache writer).
const FLUSH_ROWS: usize = 1 << 20;

/// What an export run touched — surfaced to the caller/CLI for a one-line summary.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ExportSummary {
    /// Sealed days newly written this run.
    pub days_written: Vec<NaiveDate>,
    /// Sealed days skipped because their immutable file already existed
    /// **and** the `_meta.json` row_count still matched PG `COUNT(*)`.
    pub days_skipped: usize,
    /// Token-dimension rows rewritten.
    pub tokens_written: usize,
}

/// Seal sidecar written next to each day file so a later export can detect an
/// incomplete seal (truncated write / later sync fill) and re-export.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct DayMeta {
    day: String,
    row_count: u64,
    exported_at_utc: String,
}

// ---------------------------------------------------------------------------
// Schemas
// ---------------------------------------------------------------------------

/// Day-partitioned trade file schema — the f64 *decimal* fields the sweep +
/// simulate read. Carries `vsol`+`vtok` so the price is the GMGN curve-spot
/// (`vsol / vtok`) the same as live + chart, `tx_signature` for simulate's
/// Solscan links, and `ix_labels`/`wallet` for volume-flow metrics (loaded only
/// when `Selection::with_flow`). No `real_*_reserves`/fingerprint (those live in
/// the `tokens` dimension or are re-derived on read).
fn trades_schema() -> Schema {
    use super::schema as col;
    Schema::new(vec![
        Field::new(col::T_MINT, DataType::Utf8, false),
        Field::new(col::T_IS_BUY, DataType::Boolean, false),
        Field::new(col::T_SOL_AMOUNT, DataType::Float64, false),
        Field::new(col::T_TOKEN_AMOUNT, DataType::Float64, false),
        Field::new(col::T_PRICE, DataType::Float64, false),
        Field::new(col::T_SLOT, DataType::Int64, false),
        Field::new(col::T_BLOCK_TIME, DataType::Int64, false),
        Field::new(col::T_LEG_INDEX, DataType::Int32, false),
        Field::new(col::T_VSOL, DataType::Float64, true),
        // Virtual TOKEN reserves — carried so the sweep computes the same GMGN
        // curve-spot (`vsol / vtok`) as live + chart. `real_*_reserves` are NOT in
        // the `trades` table (dropped, re-derivable from raw_txs), so the lake can't
        // carry the pool-spot fallback; curve rows lack real reserves live too, so
        // curve-spot + execution fallback is full parity for the curve phase.
        Field::new(col::T_VTOK, DataType::Float64, true),
        Field::new(col::T_VENUE, DataType::Utf8, false),
        // Intra-block execution order. Carried so the lake's per-mint ordering
        // (slot, tx_index, leg_index) reproduces PG's exactly — many trades share a
        // slot, and block_time alone can't break those ties (corpus-parity bug fix).
        Field::new(col::T_TX_INDEX, DataType::Int32, false),
        // Base58 transaction signature. Carried for the *simulate* read path only —
        // the sweep's `CorpusTrade` ignores it (stays slim). Re-added in Stage 1 of the
        // simulate→lake migration so simulate result rows keep their Solscan links
        // (`entry_tx`/`exit_tx`/`target_tx`) without a PG round-trip. ~88 B/row; lands
        // on every lake file (the sweep reads them too but never projects this column).
        Field::new(col::T_TX_SIGNATURE, DataType::Utf8, false),
        // Volume-flow V0: normalized ix-label JSON + wallet address (nullable on
        // pre-V0 sealed days via DuckDB `union_by_name`).
        Field::new(col::T_IX_LABELS, DataType::Utf8, true),
        Field::new(col::T_WALLET, DataType::Utf8, true),
    ])
}

/// Token-dimension schema — `mint`/`symbol` plus the 11 grouping-fingerprint columns
/// (same set the corpus cache carries) and the two filter columns the candidate
/// selection needs (`is_mayhem_mode`, `created_at` epoch secs). The two
/// `fp_first_slot_*` columns are trade-derived (sourced from `tokens_info`, not the
/// `tokens` creation row) — every other `fp_*` column is a `tokens` creation fact.
fn tokens_schema() -> Schema {
    use super::schema as col;
    Schema::new(vec![
        Field::new(col::K_MINT, DataType::Utf8, false),
        Field::new(col::K_SYMBOL, DataType::Utf8, false),
        Field::new(col::K_FP_TOKEN_PROGRAM_ID, DataType::Utf8, true),
        Field::new(col::K_FP_INITIAL_BUY_SOL, DataType::Float64, true),
        Field::new(col::K_FP_CU_LIMIT, DataType::Int64, true),
        Field::new(col::K_FP_CU_PRICE, DataType::Int64, true),
        Field::new(col::K_FP_IS_CASHBACK_ENABLED, DataType::Boolean, false),
        Field::new(col::K_FP_MAX_SOL_COST, DataType::Int64, true),
        Field::new(col::K_FP_SPENDABLE_SOL_IN, DataType::Int64, true),
        Field::new(col::K_FP_FIRST_SLOT_BUY_SOL, DataType::Float64, true),
        Field::new(col::K_FP_FIRST_SLOT_SELL_SOL, DataType::Float64, true),
        Field::new(col::K_FP_IX_LABELS, DataType::Utf8, true),
        Field::new(col::K_IS_MAYHEM_MODE, DataType::Boolean, false),
        Field::new(col::K_CREATED_AT, DataType::Int64, false),
    ])
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Export every newly-sealed day plus the current token dimension into the lake at
/// `root`. Idempotent: a day whose immutable file already exists **and** whose
/// `_meta.json` `row_count` still matches PG is skipped; a missing/mismatched
/// sidecar forces a re-seal.
///
/// `include_today` also exports today's still-open UTC day as a non-immutable
/// snapshot, force-overwriting it so a re-run refreshes rather than keeping a stale
/// file. The lake is the sole sweep corpus source, so this is the only way to sweep
/// *current-day* data; off by default to keep a plain export limited to sealed days.
pub async fn export_lake(pool: &PgPool, root: &Path, include_today: bool) -> Result<ExportSummary> {
    let mut summary = ExportSummary::default();

    // Today's UTC day is NOT immutable (ingest is still writing it), so the
    // skip-if-exists guard below must not apply to it under `include_today` — else a
    // re-export silently keeps a stale snapshot. Always rewrite today's file.
    let today = Utc::now().date_naive();

    for day in sealed_days(pool, include_today).await? {
        let path = trades_day_file(root, day);
        let force = include_today && day == today;
        if path.exists() && !force {
            let pg_count = count_day_trades(pool, day).await?;
            match day_seal_ok(root, day, pg_count) {
                SealVerdict::Complete => {
                    summary.days_skipped += 1;
                    continue;
                }
                SealVerdict::Reseal { reason } => {
                    tracing::warn!(
                        day = %day,
                        pg_count,
                        reason,
                        "lake: incomplete seal — re-exporting day"
                    );
                }
            }
        }
        let n = export_day(pool, root, day).await?;
        write_day_meta(root, day, n as u64)?;
        tracing::info!(day = %day, trades = n, force, "lake: exported day");
        summary.days_written.push(day);
    }

    summary.tokens_written = export_tokens(pool, root).await?;
    tracing::info!(
        days_written = summary.days_written.len(),
        days_skipped = summary.days_skipped,
        tokens = summary.tokens_written,
        "lake: export complete"
    );
    Ok(summary)
}

/// Whether an existing day file may be skipped (`Complete`) or must be rewritten.
#[derive(Debug, PartialEq, Eq)]
enum SealVerdict {
    Complete,
    Reseal { reason: &'static str },
}

/// Compare the on-disk sidecar to `pg_count`. Pure FS + numbers — unit-tested.
fn day_seal_ok(root: &Path, day: NaiveDate, pg_count: u64) -> SealVerdict {
    match read_day_meta(root, day) {
        None => SealVerdict::Reseal {
            reason: "missing _meta.json",
        },
        Some(meta) if meta.row_count != pg_count => SealVerdict::Reseal {
            reason: "row_count mismatch vs PG",
        },
        Some(_) => SealVerdict::Complete,
    }
}

async fn count_day_trades(pool: &PgPool, day: NaiveDate) -> Result<u64> {
    let start = Utc.from_utc_datetime(&day.and_hms_opt(0, 0, 0).unwrap());
    let end = Utc.from_utc_datetime(&(day + chrono::Duration::days(1)).and_hms_opt(0, 0, 0).unwrap());
    let (n,): (i64,) = sqlx::query_as(
        r#"SELECT COUNT(*)::bigint FROM trades WHERE block_time >= $1 AND block_time < $2"#,
    )
    .bind(start)
    .bind(end)
    .fetch_one(pool)
    .await
    .context("counting day trades for seal check")?;
    Ok(n as u64)
}

fn read_day_meta(root: &Path, day: NaiveDate) -> Option<DayMeta> {
    let path = trades_day_meta(root, day);
    let bytes = std::fs::read(&path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn write_day_meta(root: &Path, day: NaiveDate, row_count: u64) -> Result<()> {
    let path = trades_day_meta(root, day);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let meta = DayMeta {
        day: day.format("%Y-%m-%d").to_string(),
        row_count,
        exported_at_utc: Utc::now().to_rfc3339(),
    };
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(&meta)?)
        .with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, &path).with_context(|| format!("publishing {}", path.display()))?;
    Ok(())
}

/// Distinct UTC days present in `trades` that are **sealed** — strictly before the
/// start of today (today's chunk is still open on the server). Ordered oldest-first.
async fn sealed_days(pool: &PgPool, include_today: bool) -> Result<Vec<NaiveDate>> {
    // Cutoff = start of today (UTC). `include_today` pushes it to start of tomorrow so
    // today's open day is selected too (current-day export — see `export_lake`).
    let cutoff = if include_today {
        "date_trunc('day', now() AT TIME ZONE 'UTC') + interval '1 day'"
    } else {
        "date_trunc('day', now() AT TIME ZONE 'UTC')"
    };
    let sql = format!(
        r#"
        SELECT DISTINCT (block_time AT TIME ZONE 'UTC')::date AS d
        FROM trades
        WHERE block_time < {cutoff}
        ORDER BY d
        "#
    );
    let rows: Vec<(NaiveDate,)> = sqlx::query_as(&sql)
    .fetch_all(pool)
    .await
    .context("listing sealed trade days")?;
    Ok(rows.into_iter().map(|(d,)| d).collect())
}

// ---------------------------------------------------------------------------
// Trades day export
// ---------------------------------------------------------------------------

/// One streamed trade row out of the new-schema `trades`. Wallet comes from a
/// LEFT JOIN on `wallet_dict` with the `unknown:{id}` COALESCE fallback (same
/// gotcha as PG trade-history reads). Integer columns; converted to the model's
/// f64 decimal units on write.
#[derive(sqlx::FromRow)]
struct LakeTradeRow {
    mint_address: String,
    trade_type: String,
    venue: String,
    amount_lamports: i64,
    token_amount: i64,
    reserve_lamports: Option<i64>,
    reserve_token: Option<i64>,
    slot: i64,
    tx_index: i32,
    leg_index: i16,
    block_time: DateTime<Utc>,
    /// Raw 64-byte signature (BYTEA in `trades`); converted to base58 on write.
    tx_signature: Vec<u8>,
    /// Nullable: migration 0002 is forward-only (pre-column rows stay NULL), and a
    /// lab mirror may also have NULL when sync omitted the column. Empty/NULL →
    /// Parquet null (same as empty label array).
    ix_labels: Option<serde_json::Value>,
    wallet_address: String,
}

/// Stream one sealed day's trades and write them to the immutable day file. Returns
/// the row count. Written in `(mint, slot, tx_index, leg_index)` order so a token's
/// legs stay contiguous and in execution order (the corpus reader re-groups in one pass).
async fn export_day(pool: &PgPool, root: &Path, day: NaiveDate) -> Result<usize> {
    let start = Utc.from_utc_datetime(&day.and_hms_opt(0, 0, 0).unwrap());
    let end = Utc.from_utc_datetime(&(day + chrono::Duration::days(1)).and_hms_opt(0, 0, 0).unwrap());

    let path = trades_day_file(root, day);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    // Write to a temp sibling then rename, so a crashed export never leaves a
    // half-written file that the immutability check would treat as complete.
    let tmp = path.with_extension("parquet.tmp");

    let schema = Arc::new(trades_schema());
    let file = std::fs::File::create(&tmp)
        .with_context(|| format!("creating {}", tmp.display()))?;
    let mut writer = ArrowWriter::try_new(file, schema.clone(), None)?;

    let mut b = TradeBuilders::default();
    let mut pending = 0usize;
    let mut total = 0usize;

    let mut stream = sqlx::query_as::<_, LakeTradeRow>(
        r#"
        SELECT t.mint_address, t.trade_type, t.venue,
               t.amount_lamports, t.token_amount, t.reserve_lamports, t.reserve_token,
               t.slot, t.tx_index, t.leg_index, t.block_time, t.tx_signature,
               t.ix_labels,
               COALESCE(w.address, 'unknown:' || t.wallet_id::text) AS wallet_address
        FROM trades t
        LEFT JOIN wallet_dict w ON w.id = t.wallet_id
        WHERE t.block_time >= $1 AND t.block_time < $2
        ORDER BY t.mint_address ASC, t.slot ASC, t.tx_index ASC, t.leg_index ASC, t.block_time ASC
        "#,
    )
    .bind(start)
    .bind(end)
    .fetch(pool);

    while let Some(r) = stream.try_next().await.context("streaming day trades")? {
        b.push(&r);
        pending += 1;
        total += 1;
        if pending >= FLUSH_ROWS {
            writer.write(&b.finish(&schema)?)?;
            pending = 0;
        }
    }
    if pending > 0 {
        writer.write(&b.finish(&schema)?)?;
    }
    writer.close()?;
    std::fs::rename(&tmp, &path)
        .with_context(|| format!("publishing {}", path.display()))?;
    Ok(total)
}

/// Column builders for one trade row group. Mirrors `trade_repo`'s `TradeDbRow`
/// conversion: lamports→SOL (÷1e9), raw token units → f64 as-is, virtual reserves
/// raw→f64. `real_*_reserves` are dropped by the new schema (not stored here).
#[derive(Default)]
struct TradeBuilders {
    mint: StringBuilder,
    is_buy: BooleanBuilder,
    sol_amount: Float64Builder,
    token_amount: Float64Builder,
    price: Float64Builder,
    slot: Int64Builder,
    block_time: Int64Builder,
    leg_index: Int32Builder,
    vsol: Float64Builder,
    vtok: Float64Builder,
    venue: StringBuilder,
    tx_index: Int32Builder,
    tx_signature: StringBuilder,
    ix_labels: StringBuilder,
    wallet: StringBuilder,
}

impl TradeBuilders {
    fn push(&mut self, r: &LakeTradeRow) {
        let sol = r.amount_lamports as f64 / 1_000_000_000.0;
        let token = r.token_amount as f64;
        let price = if token > 0.0 { sol / token } else { 0.0 };
        self.mint.append_value(&r.mint_address);
        self.is_buy.append_value(r.trade_type == "buy");
        self.sol_amount.append_value(sol);
        self.token_amount.append_value(token);
        self.price.append_value(price);
        self.slot.append_value(r.slot);
        // Microseconds, not seconds: PG `block_time` is timestamptz (µs precision) and
        // the sweep's time gates (min_age/stall/time_stop) compare these timestamps.
        // Truncating to whole seconds shifts edge-case entry/exit ticks → metric drift
        // vs a fresh PG read (corpus-parity fix).
        self.block_time.append_value(r.block_time.timestamp_micros());
        self.leg_index.append_value(r.leg_index as i32);
        // `reserve_sol` is lamports in the BIGINT column but the model contract
        // (and every `TradeRow::reserve_sol` consumer: `spot_price`,
        // `approx_real_sol_reserves`) is **human SOL** — so convert ÷1e9 exactly
        // like `sol_amount` above. Without this `vsol` was 1e9× too large, which
        // silently cancelled in the `vsol/vtok` price ratio but corrupted any
        // absolute-SOL use of the reserve (the real-liquidity reconstruction).
        self.vsol.append_option(r.reserve_lamports.map(|v| v as f64 / 1_000_000_000.0));
        self.vtok.append_option(r.reserve_token.map(|v| v as f64));
        self.venue.append_value(&r.venue);
        self.tx_index.append_value(r.tx_index);
        // BYTEA → base58, the exact encoding `trade_repo` uses on the PG read path,
        // so a lake signature is byte-identical to what simulate showed pre-migration.
        self.tx_signature.append_value(sig_bytes_to_base58(&r.tx_signature));
        let empty = serde_json::Value::Null;
        let labels = normalize_labels(r.ix_labels.as_ref().unwrap_or(&empty));
        if labels.is_empty() {
            self.ix_labels.append_null();
        } else {
            self.ix_labels
                .append_option(serde_json::to_string(&labels).ok().as_deref());
        }
        self.wallet.append_value(&r.wallet_address);
    }

    fn finish(&mut self, schema: &Arc<Schema>) -> Result<RecordBatch> {
        Ok(RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(self.mint.finish()),
                Arc::new(self.is_buy.finish()),
                Arc::new(self.sol_amount.finish()),
                Arc::new(self.token_amount.finish()),
                Arc::new(self.price.finish()),
                Arc::new(self.slot.finish()),
                Arc::new(self.block_time.finish()),
                Arc::new(self.leg_index.finish()),
                Arc::new(self.vsol.finish()),
                Arc::new(self.vtok.finish()),
                Arc::new(self.venue.finish()),
                Arc::new(self.tx_index.finish()),
                Arc::new(self.tx_signature.finish()),
                Arc::new(self.ix_labels.finish()),
                Arc::new(self.wallet.finish()),
            ],
        )?)
    }
}

// ---------------------------------------------------------------------------
// Token dimension export
// ---------------------------------------------------------------------------

/// One token row projected for the dimension file; JSONB instruction args are read
/// in Rust (mirroring the corpus `FingerprintRow` path).
#[derive(sqlx::FromRow)]
struct LakeTokenRow {
    mint_address: String,
    symbol: String,
    token_program_id: Option<String>,
    initial_buy_sol: Option<f64>,
    cu_limit: Option<i64>,
    cu_price: Option<i64>,
    is_cashback_enabled: bool,
    first_slot_buy_sol: Option<f64>,
    first_slot_sell_sol: Option<f64>,
    is_mayhem_mode: bool,
    created_at: DateTime<Utc>,
    initial_buy_instruction: Option<serde_json::Value>,
    ix_labels: serde_json::Value,
}

/// Rewrite the whole token dimension (`tokens/tokens.parquet`). Small and mutable
/// (new tokens / updated fingerprints), so it is replaced wholesale each run.
/// Returns the row count.
async fn export_tokens(pool: &PgPool, root: &Path) -> Result<usize> {
    let path = tokens_file(root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let tmp = path.with_extension("parquet.tmp");

    let schema = Arc::new(tokens_schema());
    let file = std::fs::File::create(&tmp)
        .with_context(|| format!("creating {}", tmp.display()))?;
    let mut writer = ArrowWriter::try_new(file, schema.clone(), None)?;

    let mut mint = StringBuilder::new();
    let mut symbol = StringBuilder::new();
    let mut program = StringBuilder::new();
    let mut buy_sol = Float64Builder::new();
    let mut cu_limit = Int64Builder::new();
    let mut cu_price = Int64Builder::new();
    let mut cashback = BooleanBuilder::new();
    let mut max_sol_cost = Int64Builder::new();
    let mut spendable_sol_in = Int64Builder::new();
    let mut first_slot_buy = Float64Builder::new();
    let mut first_slot_sell = Float64Builder::new();
    let mut ix_labels = StringBuilder::new();
    let mut mayhem = BooleanBuilder::new();
    let mut created = Int64Builder::new();
    let mut pending = 0usize;
    let mut total = 0usize;

    macro_rules! flush {
        () => {{
            let batch = RecordBatch::try_new(
                schema.clone(),
                vec![
                    Arc::new(mint.finish()),
                    Arc::new(symbol.finish()),
                    Arc::new(program.finish()),
                    Arc::new(buy_sol.finish()),
                    Arc::new(cu_limit.finish()),
                    Arc::new(cu_price.finish()),
                    Arc::new(cashback.finish()),
                    Arc::new(max_sol_cost.finish()),
                    Arc::new(spendable_sol_in.finish()),
                    Arc::new(first_slot_buy.finish()),
                    Arc::new(first_slot_sell.finish()),
                    Arc::new(ix_labels.finish()),
                    Arc::new(mayhem.finish()),
                    Arc::new(created.finish()),
                ],
            )?;
            writer.write(&batch)?;
        }};
    }

    let mut stream = sqlx::query_as::<_, LakeTokenRow>(
        r#"
        SELECT t.mint_address, t.symbol, t.token_program_id,
               t.initial_buy_lamports::float8 / 1e9 AS initial_buy_sol,
               t.cu_limit, t.cu_price, t.is_cashback_enabled,
               ti.first_slot_buy_lamports::float8 / 1e9 AS first_slot_buy_sol,
               ti.first_slot_sell_lamports::float8 / 1e9 AS first_slot_sell_sol,
               t.is_mayhem_mode, t.created_at, t.initial_buy_instruction, t.ix_labels
        FROM tokens t
        LEFT JOIN tokens_info ti ON ti.mint_address = t.mint_address
        "#,
    )
    .fetch(pool);

    while let Some(r) = stream.try_next().await.context("streaming tokens")? {
        let labels = normalize_labels(&r.ix_labels);
        let labels_json = if labels.is_empty() {
            None
        } else {
            serde_json::to_string(&labels).ok()
        };
        mint.append_value(&r.mint_address);
        symbol.append_value(&r.symbol);
        program.append_option(r.token_program_id.as_deref());
        buy_sol.append_option(r.initial_buy_sol);
        cu_limit.append_option(r.cu_limit);
        cu_price.append_option(r.cu_price);
        cashback.append_value(r.is_cashback_enabled);
        max_sol_cost.append_option(extract_lamports(r.initial_buy_instruction.as_ref(), "max_cost_lamports"));
        spendable_sol_in
            .append_option(extract_lamports(r.initial_buy_instruction.as_ref(), "spendable_lamports_in"));
        first_slot_buy.append_option(r.first_slot_buy_sol);
        first_slot_sell.append_option(r.first_slot_sell_sol);
        ix_labels.append_option(labels_json.as_deref());
        mayhem.append_value(r.is_mayhem_mode);
        // Microseconds, not seconds: PG `tokens.created_at` is timestamptz, and the
        // candidate window/`ORDER BY created_at DESC LIMIT` must match it — second
        // truncation creates spurious ties that shift the capped candidate set.
        created.append_value(r.created_at.timestamp_micros());
        pending += 1;
        total += 1;
        if pending >= FLUSH_ROWS {
            flush!();
            pending = 0;
        }
    }
    if pending > 0 {
        flush!();
    }
    writer.close()?;
    std::fs::rename(&tmp, &path)
        .with_context(|| format!("publishing {}", path.display()))?;
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow::array::{BooleanArray, Float64Array, Int64Array, StringArray};
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

    #[test]
    fn seal_ok_when_meta_matches_pg_count() {
        let dir = std::env::temp_dir().join(format!("lake_seal_ok_{}", std::process::id()));
        let day = NaiveDate::from_ymd_opt(2026, 6, 27).unwrap();
        write_day_meta(&dir, day, 42).unwrap();
        assert_eq!(day_seal_ok(&dir, day, 42), SealVerdict::Complete);
        assert!(matches!(
            day_seal_ok(&dir, day, 41),
            SealVerdict::Reseal { reason: "row_count mismatch vs PG" }
        ));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn seal_reseals_when_meta_missing() {
        let dir = std::env::temp_dir().join(format!("lake_seal_miss_{}", std::process::id()));
        let day = NaiveDate::from_ymd_opt(2026, 6, 28).unwrap();
        assert!(matches!(
            day_seal_ok(&dir, day, 0),
            SealVerdict::Reseal { reason: "missing _meta.json" }
        ));
    }

    fn row(mint: &str, buy: bool, lamports: i64, raw_tok: i64) -> LakeTradeRow {
        LakeTradeRow {
            mint_address: mint.into(),
            trade_type: if buy { "buy".into() } else { "sell".into() },
            venue: "curve".into(),
            amount_lamports: lamports,
            token_amount: raw_tok,
            reserve_lamports: Some(30_000_000_000), // 30 SOL in lamports
            reserve_token: Some(84),
            slot: 7,
            tx_index: 0,
            leg_index: 0,
            block_time: Utc.timestamp_opt(1_700_000_000, 0).unwrap(),
            tx_signature: vec![7u8; 64], // valid 64-byte sig → non-empty base58
            ix_labels: Some(serde_json::json!(["Pump.Fun: Buy"])),
            wallet_address: "Wallet111".into(),
        }
    }

    #[test]
    fn trade_builders_apply_trade_repo_unit_conversion() {
        // 1.5 SOL = 1_500_000_000 lamports; price = sol / raw_token.
        let mut b = TradeBuilders::default();
        b.push(&row("m1", true, 1_500_000_000, 3_000_000));
        let schema = Arc::new(trades_schema());
        let batch = b.finish(&schema).unwrap();

        let sol = batch.column(2).as_any().downcast_ref::<Float64Array>().unwrap();
        let token = batch.column(3).as_any().downcast_ref::<Float64Array>().unwrap();
        let price = batch.column(4).as_any().downcast_ref::<Float64Array>().unwrap();
        let vsol = batch.column(8).as_any().downcast_ref::<Float64Array>().unwrap();
        let vtok = batch.column(9).as_any().downcast_ref::<Float64Array>().unwrap();
        assert!((sol.value(0) - 1.5).abs() < 1e-12, "lamports→SOL ÷1e9");
        assert!((token.value(0) - 3_000_000.0).abs() < 1e-6, "raw token units kept as f64");
        assert!((price.value(0) - 1.5 / 3_000_000.0).abs() < 1e-18, "price = sol/token");
        assert!((vsol.value(0) - 30.0).abs() < 1e-12, "vsol lamports→SOL ÷1e9");
        assert!((vtok.value(0) - 84.0).abs() < 1e-12, "vtok raw→f64");
    }

    #[test]
    fn null_ix_labels_writes_parquet_null() {
        use arrow::array::Array;
        let mut r = row("m1", true, 1_000_000_000, 1_000);
        r.ix_labels = None;
        let mut b = TradeBuilders::default();
        b.push(&r);
        let schema = Arc::new(trades_schema());
        let batch = b.finish(&schema).unwrap();
        let ix = batch
            .column_by_name(crate::lake::schema::T_IX_LABELS)
            .unwrap()
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert!(ix.is_null(0));
    }

    #[test]
    fn zero_token_amount_yields_zero_price_not_nan() {
        let mut b = TradeBuilders::default();
        b.push(&row("m1", false, 1_000, 0));
        let schema = Arc::new(trades_schema());
        let batch = b.finish(&schema).unwrap();
        let price = batch.column(4).as_any().downcast_ref::<Float64Array>().unwrap();
        assert_eq!(price.value(0), 0.0, "guard divide-by-zero like price_of()");
    }

    #[test]
    fn schema_field_order_is_canonical() {
        // The writer's Arrow schema order must equal the single-source column arrays,
        // so the positional `finish()` vec can't silently diverge from the names.
        let trades_s = trades_schema();
        let trades: Vec<&str> = trades_s.fields().iter().map(|f| f.name().as_str()).collect();
        assert_eq!(trades, crate::lake::schema::TRADE_WRITE_COLS.to_vec());
        let tokens_s = tokens_schema();
        let tokens: Vec<&str> = tokens_s.fields().iter().map(|f| f.name().as_str()).collect();
        assert_eq!(tokens, crate::lake::schema::TOKEN_WRITE_COLS.to_vec());
    }

    #[test]
    fn finish_maps_each_builder_to_its_named_column() {
        // Distinct value per column so a same-typed builder swap in `finish()`
        // (slot↔block_time both Int64; vsol↔vtok / sol↔token↔price all Float64) is
        // caught by reading each column BACK BY NAME, not by position.
        use crate::lake::schema as col;
        let mut b = TradeBuilders::default();
        // 2 SOL, 1_234_567 raw tokens → price = 2/1_234_567; vsol 30, vtok 84, slot 7.
        b.push(&row("mZ", true, 2_000_000_000, 1_234_567));
        let schema = Arc::new(trades_schema());
        let batch = b.finish(&schema).unwrap();

        let f64_at = |name: &str| -> f64 {
            let i = batch.schema().index_of(name).unwrap();
            batch.column(i).as_any().downcast_ref::<Float64Array>().unwrap().value(0)
        };
        let i64_at = |name: &str| -> i64 {
            let i = batch.schema().index_of(name).unwrap();
            batch.column(i).as_any().downcast_ref::<Int64Array>().unwrap().value(0)
        };
        assert!((f64_at(col::T_SOL_AMOUNT) - 2.0).abs() < 1e-12);
        assert!((f64_at(col::T_TOKEN_AMOUNT) - 1_234_567.0).abs() < 1e-6);
        assert!((f64_at(col::T_PRICE) - 2.0 / 1_234_567.0).abs() < 1e-18);
        assert!((f64_at(col::T_VSOL) - 30.0).abs() < 1e-12);
        assert!((f64_at(col::T_VTOK) - 84.0).abs() < 1e-12);
        assert_eq!(i64_at(col::T_SLOT), 7);
        assert_eq!(i64_at(col::T_BLOCK_TIME), 1_700_000_000_000_000);
    }

    #[test]
    fn day_file_round_trips_through_parquet() {
        // Write a day file via the builder/writer path and read it back, asserting
        // the decimal projection survives. No DB needed.
        let dir = std::env::temp_dir().join(format!("lake_export_test_{}", std::process::id()));
        let day = NaiveDate::from_ymd_opt(2026, 6, 27).unwrap();
        let path = trades_day_file(&dir, day);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();

        let schema = Arc::new(trades_schema());
        let file = std::fs::File::create(&path).unwrap();
        let mut writer = ArrowWriter::try_new(file, schema.clone(), None).unwrap();
        let mut b = TradeBuilders::default();
        b.push(&row("m1", true, 2_000_000_000, 1_000_000));
        b.push(&row("m1", false, 500_000_000, 250_000));
        writer.write(&b.finish(&schema).unwrap()).unwrap();
        writer.close().unwrap();

        let f = std::fs::File::open(&path).unwrap();
        let mut reader = ParquetRecordBatchReaderBuilder::try_new(f).unwrap().build().unwrap();
        let batch = reader.next().unwrap().unwrap();
        assert_eq!(batch.num_rows(), 2);
        let mint = batch.column(0).as_any().downcast_ref::<StringArray>().unwrap();
        let is_buy = batch.column(1).as_any().downcast_ref::<BooleanArray>().unwrap();
        let sol = batch.column(2).as_any().downcast_ref::<Float64Array>().unwrap();
        let slot = batch.column(5).as_any().downcast_ref::<Int64Array>().unwrap();
        assert_eq!(mint.value(0), "m1");
        assert!(is_buy.value(0));
        assert!(!is_buy.value(1));
        assert!((sol.value(0) - 2.0).abs() < 1e-12);
        assert_eq!(slot.value(0), 7);

        std::fs::remove_dir_all(&dir).ok();
    }
}
