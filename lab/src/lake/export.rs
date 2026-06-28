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

use trading_core::grouping::{extract_lamports, normalize_labels};

use super::{tokens_file, trades_day_file};

/// Flush a Parquet row group roughly every this-many rows to bound peak builder RAM
/// (mirrors the corpus-cache writer).
const FLUSH_ROWS: usize = 1 << 20;

/// What an export run touched — surfaced to the caller/CLI for a one-line summary.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ExportSummary {
    /// Sealed days newly written this run.
    pub days_written: Vec<NaiveDate>,
    /// Sealed days skipped because their immutable file already existed.
    pub days_skipped: usize,
    /// Token-dimension rows rewritten.
    pub tokens_written: usize,
}

// ---------------------------------------------------------------------------
// Schemas
// ---------------------------------------------------------------------------

/// Day-partitioned trade file schema — the f64 *decimal* fields the sweep reads
/// (no `tx_signature`, no `real_*_reserves`, no fingerprint: those live in the
/// `tokens` dimension or are dropped by the new schema).
fn trades_schema() -> Schema {
    Schema::new(vec![
        Field::new("mint", DataType::Utf8, false),
        Field::new("wallet", DataType::Utf8, false),
        Field::new("is_buy", DataType::Boolean, false),
        Field::new("sol_amount", DataType::Float64, false),
        Field::new("token_amount", DataType::Float64, false),
        Field::new("price", DataType::Float64, false),
        Field::new("slot", DataType::Int64, false),
        Field::new("block_time", DataType::Int64, false),
        Field::new("leg_index", DataType::Int32, false),
        Field::new("vsol", DataType::Float64, true),
        Field::new("venue", DataType::Utf8, false),
    ])
}

/// Token-dimension schema — `mint`/`symbol` plus the 9 grouping-fingerprint columns
/// (same set the corpus cache carries) and the two filter columns the candidate
/// selection needs (`is_mayhem_mode`, `created_at` epoch secs).
fn tokens_schema() -> Schema {
    Schema::new(vec![
        Field::new("mint", DataType::Utf8, false),
        Field::new("symbol", DataType::Utf8, false),
        Field::new("fp_creator_wallet", DataType::Utf8, false),
        Field::new("fp_token_program_id", DataType::Utf8, true),
        Field::new("fp_initial_buy_sol", DataType::Float64, true),
        Field::new("fp_cu_limit", DataType::Int64, true),
        Field::new("fp_cu_price", DataType::Int64, true),
        Field::new("fp_is_cashback_enabled", DataType::Boolean, false),
        Field::new("fp_max_sol_cost", DataType::Int64, true),
        Field::new("fp_spendable_sol_in", DataType::Int64, true),
        Field::new("fp_ix_labels", DataType::Utf8, true),
        Field::new("is_mayhem_mode", DataType::Boolean, false),
        Field::new("created_at", DataType::Int64, false),
    ])
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Export every newly-sealed day plus the current token dimension into the lake at
/// `root`. Idempotent: a day whose immutable file already exists is skipped, so
/// re-running only writes days that landed since the last run.
pub async fn export_lake(pool: &PgPool, root: &Path) -> Result<ExportSummary> {
    let mut summary = ExportSummary::default();

    for day in sealed_days(pool).await? {
        let path = trades_day_file(root, day);
        if path.exists() {
            summary.days_skipped += 1;
            continue;
        }
        let n = export_day(pool, root, day).await?;
        tracing::info!(day = %day, trades = n, "lake: exported sealed day");
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

/// Distinct UTC days present in `trades` that are **sealed** — strictly before the
/// start of today (today's chunk is still open on the server). Ordered oldest-first.
async fn sealed_days(pool: &PgPool) -> Result<Vec<NaiveDate>> {
    let rows: Vec<(NaiveDate,)> = sqlx::query_as(
        r#"
        SELECT DISTINCT (block_time AT TIME ZONE 'UTC')::date AS d
        FROM trades
        WHERE block_time < date_trunc('day', now() AT TIME ZONE 'UTC')
        ORDER BY d
        "#,
    )
    .fetch_all(pool)
    .await
    .context("listing sealed trade days")?;
    Ok(rows.into_iter().map(|(d,)| d).collect())
}

// ---------------------------------------------------------------------------
// Trades day export
// ---------------------------------------------------------------------------

/// One streamed trade row out of the new-schema `trades` joined to `wallet_dict`.
/// Integer columns; converted to the model's f64 decimal units on write.
#[derive(sqlx::FromRow)]
struct LakeTradeRow {
    mint_address: String,
    wallet: String,
    trade_type: String,
    venue: String,
    sol_amount: i64,
    token_amount: i64,
    virtual_sol_reserves: Option<i64>,
    slot: i64,
    leg_index: i16,
    block_time: DateTime<Utc>,
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
        SELECT t.mint_address, w.address AS wallet, t.trade_type, t.venue,
               t.sol_amount, t.token_amount, t.virtual_sol_reserves,
               t.slot, t.leg_index, t.block_time
        FROM trades t
        JOIN wallet_dict w ON w.id = t.wallet_id
        WHERE t.block_time >= $1 AND t.block_time < $2
        ORDER BY t.mint_address ASC, t.slot ASC, t.tx_index ASC, t.leg_index ASC
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
    wallet: StringBuilder,
    is_buy: BooleanBuilder,
    sol_amount: Float64Builder,
    token_amount: Float64Builder,
    price: Float64Builder,
    slot: Int64Builder,
    block_time: Int64Builder,
    leg_index: Int32Builder,
    vsol: Float64Builder,
    venue: StringBuilder,
}

impl TradeBuilders {
    fn push(&mut self, r: &LakeTradeRow) {
        let sol = r.sol_amount as f64 / 1_000_000_000.0;
        let token = r.token_amount as f64;
        let price = if token > 0.0 { sol / token } else { 0.0 };
        self.mint.append_value(&r.mint_address);
        self.wallet.append_value(&r.wallet);
        self.is_buy.append_value(r.trade_type == "buy");
        self.sol_amount.append_value(sol);
        self.token_amount.append_value(token);
        self.price.append_value(price);
        self.slot.append_value(r.slot);
        self.block_time.append_value(r.block_time.timestamp());
        self.leg_index.append_value(r.leg_index as i32);
        self.vsol.append_option(r.virtual_sol_reserves.map(|v| v as f64));
        self.venue.append_value(&r.venue);
    }

    fn finish(&mut self, schema: &Arc<Schema>) -> Result<RecordBatch> {
        Ok(RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(self.mint.finish()),
                Arc::new(self.wallet.finish()),
                Arc::new(self.is_buy.finish()),
                Arc::new(self.sol_amount.finish()),
                Arc::new(self.token_amount.finish()),
                Arc::new(self.price.finish()),
                Arc::new(self.slot.finish()),
                Arc::new(self.block_time.finish()),
                Arc::new(self.leg_index.finish()),
                Arc::new(self.vsol.finish()),
                Arc::new(self.venue.finish()),
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
    creator_wallet: String,
    token_program_id: Option<String>,
    initial_buy_sol: Option<f64>,
    cu_limit: Option<i64>,
    cu_price: Option<i64>,
    is_cashback_enabled: bool,
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
    let mut creator = StringBuilder::new();
    let mut program = StringBuilder::new();
    let mut buy_sol = Float64Builder::new();
    let mut cu_limit = Int64Builder::new();
    let mut cu_price = Int64Builder::new();
    let mut cashback = BooleanBuilder::new();
    let mut max_sol_cost = Int64Builder::new();
    let mut spendable_sol_in = Int64Builder::new();
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
                    Arc::new(creator.finish()),
                    Arc::new(program.finish()),
                    Arc::new(buy_sol.finish()),
                    Arc::new(cu_limit.finish()),
                    Arc::new(cu_price.finish()),
                    Arc::new(cashback.finish()),
                    Arc::new(max_sol_cost.finish()),
                    Arc::new(spendable_sol_in.finish()),
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
        SELECT mint_address, symbol, creator_wallet, token_program_id,
               initial_buy_sol, cu_limit, cu_price, is_cashback_enabled,
               is_mayhem_mode, created_at, initial_buy_instruction, ix_labels
        FROM tokens
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
        creator.append_value(&r.creator_wallet);
        program.append_option(r.token_program_id.as_deref());
        buy_sol.append_option(r.initial_buy_sol);
        cu_limit.append_option(r.cu_limit);
        cu_price.append_option(r.cu_price);
        cashback.append_value(r.is_cashback_enabled);
        max_sol_cost.append_option(extract_lamports(r.initial_buy_instruction.as_ref(), "max_sol_cost"));
        spendable_sol_in
            .append_option(extract_lamports(r.initial_buy_instruction.as_ref(), "spendable_sol_in"));
        ix_labels.append_option(labels_json.as_deref());
        mayhem.append_value(r.is_mayhem_mode);
        created.append_value(r.created_at.timestamp());
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

    fn row(mint: &str, wallet: &str, buy: bool, lamports: i64, raw_tok: i64) -> LakeTradeRow {
        LakeTradeRow {
            mint_address: mint.into(),
            wallet: wallet.into(),
            trade_type: if buy { "buy".into() } else { "sell".into() },
            venue: "curve".into(),
            sol_amount: lamports,
            token_amount: raw_tok,
            virtual_sol_reserves: Some(42),
            slot: 7,
            leg_index: 0,
            block_time: Utc.timestamp_opt(1_700_000_000, 0).unwrap(),
        }
    }

    #[test]
    fn trade_builders_apply_trade_repo_unit_conversion() {
        // 1.5 SOL = 1_500_000_000 lamports; price = sol / raw_token.
        let mut b = TradeBuilders::default();
        b.push(&row("m1", "wA", true, 1_500_000_000, 3_000_000));
        let schema = Arc::new(trades_schema());
        let batch = b.finish(&schema).unwrap();

        let sol = batch.column(3).as_any().downcast_ref::<Float64Array>().unwrap();
        let token = batch.column(4).as_any().downcast_ref::<Float64Array>().unwrap();
        let price = batch.column(5).as_any().downcast_ref::<Float64Array>().unwrap();
        let vsol = batch.column(9).as_any().downcast_ref::<Float64Array>().unwrap();
        assert!((sol.value(0) - 1.5).abs() < 1e-12, "lamports→SOL ÷1e9");
        assert!((token.value(0) - 3_000_000.0).abs() < 1e-6, "raw token units kept as f64");
        assert!((price.value(0) - 1.5 / 3_000_000.0).abs() < 1e-18, "price = sol/token");
        assert!((vsol.value(0) - 42.0).abs() < 1e-12, "vsol raw→f64");
    }

    #[test]
    fn zero_token_amount_yields_zero_price_not_nan() {
        let mut b = TradeBuilders::default();
        b.push(&row("m1", "wA", false, 1_000, 0));
        let schema = Arc::new(trades_schema());
        let batch = b.finish(&schema).unwrap();
        let price = batch.column(5).as_any().downcast_ref::<Float64Array>().unwrap();
        assert_eq!(price.value(0), 0.0, "guard divide-by-zero like price_of()");
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
        b.push(&row("m1", "wA", true, 2_000_000_000, 1_000_000));
        b.push(&row("m1", "wB", false, 500_000_000, 250_000));
        writer.write(&b.finish(&schema).unwrap()).unwrap();
        writer.close().unwrap();

        let f = std::fs::File::open(&path).unwrap();
        let mut reader = ParquetRecordBatchReaderBuilder::try_new(f).unwrap().build().unwrap();
        let batch = reader.next().unwrap().unwrap();
        assert_eq!(batch.num_rows(), 2);
        let mint = batch.column(0).as_any().downcast_ref::<StringArray>().unwrap();
        let is_buy = batch.column(2).as_any().downcast_ref::<BooleanArray>().unwrap();
        let sol = batch.column(3).as_any().downcast_ref::<Float64Array>().unwrap();
        let slot = batch.column(6).as_any().downcast_ref::<Int64Array>().unwrap();
        assert_eq!(mint.value(0), "m1");
        assert!(is_buy.value(0));
        assert!(!is_buy.value(1));
        assert!((sol.value(0) - 2.0).abs() < 1e-12);
        assert_eq!(slot.value(0), 7);

        std::fs::remove_dir_all(&dir).ok();
    }
}
