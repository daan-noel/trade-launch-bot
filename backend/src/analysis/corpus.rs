//! Corpus loader: turn the live cache or the DB into an in-memory set of
//! per-token trade histories the sweep can call `simulate` over many times
//! without ever touching the DB in the loop.
//!
//! Two `CorpusSource` impls behind one trait:
//!   - [`CacheSource`] — the hot/recent window from the live `TokenCache`,
//!     refcount-cloned (`Arc`), no DB.
//!   - [`DbSource`]    — the historical / cache-miss tail via a chunked batch
//!     query (one round-trip per mint chunk, per-mint capped via a `ROW_NUMBER`
//!     window), replacing any per-token N+1 loop.
//!
//! The loaded corpus is cached to a compact columnar Parquet file keyed by a
//! corpus hash, so a re-run with the same selection loads instantly off disk
//! instead of re-hitting the DB. The full `Trade` never enters the sweep loop —
//! only the fields entry/exit/cohort/fingerprint read are projected.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use arrow::array::{
    Array, BooleanArray, BooleanBuilder, Float64Array, Float64Builder, Int32Array, Int32Builder,
    Int64Array, Int64Builder, StringArray, StringBuilder,
};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::ArrowWriter;
use sqlx::PgPool;

use crate::models::trade::{Trade, TradeType};
use crate::state::token_cache::TokenCache;
use crate::storage::repositories::trade_repo::TradeSlimRow;

/// One token's trade history, ready for `simulate`. `trades` is shared (`Arc`)
/// so the cache source clones a refcount rather than deep-copying up to the
/// retention window of `Trade`s while the ingest writer runs.
#[derive(Clone)]
pub struct TokenTrades {
    pub mint: String,
    pub symbol: String,
    pub trades: Arc<Vec<Trade>>,
}

/// The whole loaded population plus the hash that keys its Parquet cache.
pub struct Corpus {
    pub tokens: Vec<TokenTrades>,
    /// Stable hash of the selection's mints + per-token trade counts.
    pub hash: String,
}

impl Corpus {
    pub fn token_count(&self) -> usize {
        self.tokens.len()
    }

    pub fn trade_count(&self) -> usize {
        self.tokens.iter().map(|t| t.trades.len()).sum()
    }
}

/// Explicit population scope — never "load everything". A loader clips to
/// `token_cap` and/or `created_after`, logging which bound bit so the run never
/// silently truncates.
#[derive(Clone, Debug)]
pub struct Selection {
    /// Explicit mint list; when `None`, the DB source picks the newest tokens.
    pub mints: Option<Vec<String>>,
    /// Hard cap on number of tokens loaded.
    pub token_cap: usize,
    /// Only tokens created at/after this instant (DB source).
    pub created_after: Option<DateTime<Utc>>,
    /// Per-mint trade cap for the DB batch query (newest N per mint).
    pub per_mint_cap: i64,
    /// Drop AMM (post-migration) legs, keeping only bonding-curve trades.
    pub curve_only: bool,
}

impl Default for Selection {
    fn default() -> Self {
        Self {
            mints: None,
            token_cap: 5_000,
            created_after: None,
            per_mint_cap: crate::state::token_cache::MAX_TRADES_RETAINED as i64,
            curve_only: false,
        }
    }
}

/// Source of a corpus — cache or DB, one interface.
#[async_trait]
pub trait CorpusSource {
    async fn load(&self, sel: &Selection) -> Result<Corpus>;
}

/// Apply the shared corpus-wide filters (curve-only) to a freshly built token.
fn finalize_token(mint: String, symbol: String, mut trades: Vec<Trade>, sel: &Selection) -> TokenTrades {
    if sel.curve_only {
        trades.retain(|t| t.venue == "curve");
    }
    TokenTrades {
        mint,
        symbol,
        trades: Arc::new(trades),
    }
}

/// Stable hash of a selection's realised population — sorted mints plus each
/// mint's trade count — used as the Parquet cache key. `DefaultHasher` is seeded
/// deterministically, so the same population hashes the same across runs.
fn corpus_hash(tokens: &[TokenTrades], curve_only: bool) -> String {
    let mut pairs: Vec<(&str, usize)> =
        tokens.iter().map(|t| (t.mint.as_str(), t.trades.len())).collect();
    pairs.sort_unstable();
    let mut h = DefaultHasher::new();
    curve_only.hash(&mut h);
    for (m, n) in pairs {
        m.hash(&mut h);
        n.hash(&mut h);
    }
    format!("{:016x}", h.finish())
}

// ---------------------------------------------------------------------------
// Cache source
// ---------------------------------------------------------------------------

/// Loads the hot/recent window straight from the live `TokenCache` — zero DB,
/// refcount-clone of each token's shared trade buffer.
pub struct CacheSource {
    cache: Arc<TokenCache>,
}

impl CacheSource {
    pub fn new(cache: Arc<TokenCache>) -> Self {
        Self { cache }
    }
}

#[async_trait]
impl CorpusSource for CacheSource {
    async fn load(&self, sel: &Selection) -> Result<Corpus> {
        let allow: Option<std::collections::HashSet<&String>> =
            sel.mints.as_ref().map(|m| m.iter().collect());

        let mut tokens: Vec<TokenTrades> = Vec::new();
        for entry in self.cache.iter() {
            let st = entry.value();
            if st.token.is_mayhem_mode {
                continue;
            }
            if let Some(after) = sel.created_after {
                if st.token.created_at < after {
                    continue;
                }
            }
            if let Some(allow) = &allow {
                if !allow.contains(&st.token.mint_address) {
                    continue;
                }
            }
            // Refcount-clone the shared buffer under the shard guard; if curve_only
            // filtering is requested we materialise a filtered copy, else reuse the
            // Arc directly (zero copy).
            let tt = if sel.curve_only {
                let filtered: Vec<Trade> =
                    st.trades.iter().filter(|t| t.venue == "curve").cloned().collect();
                TokenTrades {
                    mint: st.token.mint_address.clone(),
                    symbol: st.token.symbol.clone(),
                    trades: Arc::new(filtered),
                }
            } else {
                TokenTrades {
                    mint: st.token.mint_address.clone(),
                    symbol: st.token.symbol.clone(),
                    trades: st.trades.clone(),
                }
            };
            if !tt.trades.is_empty() {
                tokens.push(tt);
            }
            if tokens.len() >= sel.token_cap {
                tracing::warn!(
                    cap = sel.token_cap,
                    "corpus: cache token_cap reached — population clipped (not truncated silently)"
                );
                break;
            }
        }
        let hash = corpus_hash(&tokens, sel.curve_only);
        Ok(Corpus { tokens, hash })
    }
}

// ---------------------------------------------------------------------------
// DB source
// ---------------------------------------------------------------------------

/// Loads the historical / cache-miss tail via a chunked batch query — one
/// round-trip per mint chunk, never the per-token N+1 loop.
pub struct DbSource {
    pool: PgPool,
    /// Mints per `ANY($1)` so a single statement's result stays bounded.
    chunk: usize,
}

impl DbSource {
    pub fn new(pool: PgPool) -> Self {
        Self { pool, chunk: 200 }
    }

    /// Trades for a chunk of mints in one round-trip, each mint capped to its
    /// newest `per_mint_cap` trades (a per-mint `ROW_NUMBER` window bounds the
    /// worst case to `mints.len() * per_mint_cap` rows), grouped chronological
    /// per mint. Mints with no trades are simply absent. Slim projection (no
    /// `ix_labels` JSONB) — the sweep never reads per-trade labels.
    async fn fetch_chunk(
        &self,
        mints: &[String],
        per_mint_cap: i64,
    ) -> Result<HashMap<String, Vec<Trade>>> {
        // Newest `per_mint_cap` per mint via the DESC window, then re-sorted ASC
        // so each mint's run arrives chronological — the launch→recent window the
        // live cache holds.
        let rows = sqlx::query_as::<_, TradeSlimRow>(
            r#"
            WITH ranked AS (
                SELECT id, mint_address, wallet_address, trade_type,
                       sol_amount, token_amount, price_per_token,
                       tx_signature, leg_index, slot, block_time, received_at,
                       virtual_sol_reserves, virtual_token_reserves,
                       real_sol_reserves, real_token_reserves,
                       ix_type, venue,
                       ROW_NUMBER() OVER (
                         PARTITION BY mint_address
                         ORDER BY slot DESC, block_time DESC, tx_signature DESC, leg_index DESC
                       ) AS rn
                FROM trades
                WHERE mint_address = ANY($1)
            )
            SELECT id, mint_address, wallet_address, trade_type,
                   sol_amount, token_amount, price_per_token,
                   tx_signature, leg_index, slot, block_time, received_at,
                   virtual_sol_reserves, virtual_token_reserves,
                   real_sol_reserves, real_token_reserves,
                   ix_type, venue
            FROM ranked
            WHERE rn <= $2
            ORDER BY mint_address ASC, slot ASC, block_time ASC, tx_signature ASC, leg_index ASC
            "#,
        )
        .bind(mints)
        .bind(per_mint_cap)
        .fetch_all(&self.pool)
        .await?;

        let mut grouped: HashMap<String, Vec<Trade>> = HashMap::with_capacity(mints.len());
        for row in rows {
            let trade = Trade::try_from(row)?;
            grouped
                .entry(trade.mint_address.clone())
                .or_default()
                .push(trade);
        }
        Ok(grouped)
    }

    /// Resolve `(mint, symbol)` candidates: the explicit list (symbols looked up)
    /// or the newest non-mayhem tokens within the window, capped.
    async fn candidates(&self, sel: &Selection) -> Result<Vec<(String, String)>> {
        if let Some(mints) = &sel.mints {
            let rows: Vec<(String, String)> = sqlx::query_as(
                "SELECT mint_address, symbol FROM tokens WHERE mint_address = ANY($1)",
            )
            .bind(mints)
            .fetch_all(&self.pool)
            .await
            .context("loading symbols for explicit mint selection")?;
            let by_mint: HashMap<String, String> = rows.into_iter().collect();
            Ok(mints
                .iter()
                .take(sel.token_cap)
                .map(|m| (m.clone(), by_mint.get(m).cloned().unwrap_or_default()))
                .collect())
        } else {
            let rows: Vec<(String, String)> = sqlx::query_as(
                "SELECT mint_address, symbol FROM tokens \
                 WHERE is_mayhem_mode = FALSE AND ($1::timestamptz IS NULL OR created_at >= $1) \
                 ORDER BY created_at DESC LIMIT $2",
            )
            .bind(sel.created_after)
            .bind(sel.token_cap as i64)
            .fetch_all(&self.pool)
            .await
            .context("selecting candidate tokens")?;
            Ok(rows)
        }
    }
}

#[async_trait]
impl CorpusSource for DbSource {
    async fn load(&self, sel: &Selection) -> Result<Corpus> {
        let candidates = self.candidates(sel).await?;
        let symbols: HashMap<String, String> = candidates.iter().cloned().collect();
        let mints: Vec<String> = candidates.into_iter().map(|(m, _)| m).collect();

        let mut tokens: Vec<TokenTrades> = Vec::with_capacity(mints.len());
        for chunk in mints.chunks(self.chunk) {
            let grouped = self
                .fetch_chunk(chunk, sel.per_mint_cap)
                .await
                .context("batched trade fetch for corpus chunk")?;
            for mint in chunk {
                if let Some(trades) = grouped.get(mint) {
                    if trades.is_empty() {
                        continue;
                    }
                    let symbol = symbols.get(mint).cloned().unwrap_or_default();
                    tokens.push(finalize_token(mint.clone(), symbol, trades.clone(), sel));
                }
            }
        }
        tracing::info!(
            tokens = tokens.len(),
            trades = tokens.iter().map(|t| t.trades.len()).sum::<usize>(),
            "corpus: loaded from DB"
        );
        let hash = corpus_hash(&tokens, sel.curve_only);
        Ok(Corpus { tokens, hash })
    }
}

// ---------------------------------------------------------------------------
// Cache-first source: cache window + DB tail, then Parquet cache
// ---------------------------------------------------------------------------

/// Default on-disk location for cached corpora.
pub fn corpus_cache_path(dir: &Path, hash: &str) -> PathBuf {
    dir.join(format!("corpus_{hash}.parquet"))
}

/// Load a corpus, preferring an existing Parquet cache for the *DB selection*'s
/// hash. Because the hash depends on the realised population (mints + counts),
/// the first call must compute it from a live load; we therefore key the cache
/// on a cheap pre-hash of the selection inputs and store under the realised
/// hash. To keep it simple and correct, callers pass an explicit
/// `cache_key`-derived path; see [`load_or_build`].
pub async fn load_or_build<S: CorpusSource>(
    source: &S,
    sel: &Selection,
    cache_dir: &Path,
    cache_key: &str,
) -> Result<Corpus> {
    let path = corpus_cache_path(cache_dir, cache_key);
    if path.exists() {
        tracing::info!(path = %path.display(), "corpus: loading from Parquet cache");
        return read_corpus_parquet(&path);
    }
    let corpus = source.load(sel).await?;
    std::fs::create_dir_all(cache_dir).ok();
    if let Err(e) = write_corpus_parquet(&corpus, &path) {
        tracing::warn!("corpus: failed to write Parquet cache: {e}");
    }
    Ok(corpus)
}

// ---------------------------------------------------------------------------
// Compact columnar Parquet (write + read)
// ---------------------------------------------------------------------------

fn corpus_schema() -> Schema {
    Schema::new(vec![
        Field::new("mint", DataType::Utf8, false),
        Field::new("symbol", DataType::Utf8, false),
        Field::new("wallet", DataType::Utf8, false),
        Field::new("is_buy", DataType::Boolean, false),
        Field::new("sol_amount", DataType::Float64, false),
        Field::new("token_amount", DataType::Float64, false),
        Field::new("price", DataType::Float64, false),
        Field::new("slot", DataType::Int64, false),
        Field::new("block_time", DataType::Int64, false),
        Field::new("leg_index", DataType::Int32, false),
        Field::new("tx_signature", DataType::Utf8, false),
        Field::new("venue", DataType::Utf8, false),
        Field::new("vsol", DataType::Float64, true),
        Field::new("vtok", DataType::Float64, true),
        Field::new("rsol", DataType::Float64, true),
        Field::new("rtok", DataType::Float64, true),
    ])
}

/// Write the compact projection grouped by mint (one contiguous run per token),
/// so the reader can re-group in a single linear pass.
pub fn write_corpus_parquet(corpus: &Corpus, path: &Path) -> Result<()> {
    let schema = Arc::new(corpus_schema());
    let file = std::fs::File::create(path)?;
    let mut writer = ArrowWriter::try_new(file, schema.clone(), None)?;

    // Flush a row group roughly every this-many rows to bound peak builder RAM.
    const FLUSH_ROWS: usize = 1 << 20;

    let mut mint = StringBuilder::new();
    let mut symbol = StringBuilder::new();
    let mut wallet = StringBuilder::new();
    let mut is_buy = BooleanBuilder::new();
    let mut sol_amount = Float64Builder::new();
    let mut token_amount = Float64Builder::new();
    let mut price = Float64Builder::new();
    let mut slot = Int64Builder::new();
    let mut block_time = Int64Builder::new();
    let mut leg_index = Int32Builder::new();
    let mut tx = StringBuilder::new();
    let mut venue = StringBuilder::new();
    let mut vsol = Float64Builder::new();
    let mut vtok = Float64Builder::new();
    let mut rsol = Float64Builder::new();
    let mut rtok = Float64Builder::new();
    let mut pending = 0usize;

    macro_rules! flush {
        () => {{
            let batch = RecordBatch::try_new(
                schema.clone(),
                vec![
                    Arc::new(mint.finish()),
                    Arc::new(symbol.finish()),
                    Arc::new(wallet.finish()),
                    Arc::new(is_buy.finish()),
                    Arc::new(sol_amount.finish()),
                    Arc::new(token_amount.finish()),
                    Arc::new(price.finish()),
                    Arc::new(slot.finish()),
                    Arc::new(block_time.finish()),
                    Arc::new(leg_index.finish()),
                    Arc::new(tx.finish()),
                    Arc::new(venue.finish()),
                    Arc::new(vsol.finish()),
                    Arc::new(vtok.finish()),
                    Arc::new(rsol.finish()),
                    Arc::new(rtok.finish()),
                ],
            )?;
            writer.write(&batch)?;
        }};
    }

    for token in &corpus.tokens {
        for t in token.trades.iter() {
            mint.append_value(&token.mint);
            symbol.append_value(&token.symbol);
            wallet.append_value(&t.wallet_address);
            is_buy.append_value(matches!(t.trade_type, TradeType::Buy));
            sol_amount.append_value(t.sol_amount);
            token_amount.append_value(t.token_amount);
            price.append_value(t.price_per_token);
            slot.append_value(t.slot as i64);
            block_time.append_value(t.block_time.timestamp());
            leg_index.append_value(t.leg_index as i32);
            tx.append_value(&t.tx_signature);
            venue.append_value(&t.venue);
            vsol.append_option(t.virtual_sol_reserves);
            vtok.append_option(t.virtual_token_reserves);
            rsol.append_option(t.real_sol_reserves);
            rtok.append_option(t.real_token_reserves);
            pending += 1;
        }
        if pending >= FLUSH_ROWS {
            flush!();
            pending = 0;
        }
    }
    if pending > 0 {
        flush!();
    }
    writer.close()?;
    Ok(())
}

fn opt_f64(arr: &Float64Array, i: usize) -> Option<f64> {
    if arr.is_null(i) {
        None
    } else {
        Some(arr.value(i))
    }
}

/// Read a compact corpus Parquet back into [`Corpus`], re-grouping the
/// mint-contiguous rows into per-token histories. Unused `Trade` fields
/// (`id`, instruction labels) are rehydrated to inert defaults — the pure
/// entry/exit fns never read them.
pub fn read_corpus_parquet(path: &Path) -> Result<Corpus> {
    let file = std::fs::File::open(path)?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let reader = builder.build()?;

    let mut tokens: Vec<TokenTrades> = Vec::new();
    let mut cur_mint: Option<String> = None;
    let mut cur_symbol = String::new();
    let mut cur_trades: Vec<Trade> = Vec::new();

    for batch in reader {
        let batch = batch?;
        let mint = col_str(&batch, 0)?;
        let symbol = col_str(&batch, 1)?;
        let wallet = col_str(&batch, 2)?;
        let is_buy = col::<BooleanArray>(&batch, 3)?;
        let sol_amount = col::<Float64Array>(&batch, 4)?;
        let token_amount = col::<Float64Array>(&batch, 5)?;
        let price = col::<Float64Array>(&batch, 6)?;
        let slot = col::<Int64Array>(&batch, 7)?;
        let block_time = col::<Int64Array>(&batch, 8)?;
        let leg_index = col::<Int32Array>(&batch, 9)?;
        let tx = col_str(&batch, 10)?;
        let venue = col_str(&batch, 11)?;
        let vsol = col::<Float64Array>(&batch, 12)?;
        let vtok = col::<Float64Array>(&batch, 13)?;
        let rsol = col::<Float64Array>(&batch, 14)?;
        let rtok = col::<Float64Array>(&batch, 15)?;

        for i in 0..batch.num_rows() {
            let m = mint.value(i);
            if cur_mint.as_deref() != Some(m) {
                if let Some(prev) = cur_mint.take() {
                    tokens.push(TokenTrades {
                        mint: prev,
                        symbol: std::mem::take(&mut cur_symbol),
                        trades: Arc::new(std::mem::take(&mut cur_trades)),
                    });
                }
                cur_mint = Some(m.to_string());
                cur_symbol = symbol.value(i).to_string();
            }
            let bt = DateTime::<Utc>::from_timestamp(block_time.value(i), 0).unwrap_or_else(Utc::now);
            cur_trades.push(Trade {
                id: uuid::Uuid::nil(),
                mint_address: m.to_string(),
                wallet_address: wallet.value(i).to_string(),
                trade_type: if is_buy.value(i) { TradeType::Buy } else { TradeType::Sell },
                sol_amount: sol_amount.value(i),
                token_amount: token_amount.value(i),
                price_per_token: price.value(i),
                tx_signature: tx.value(i).to_string(),
                leg_index: leg_index.value(i) as u32,
                slot: slot.value(i) as u64,
                block_time: bt,
                received_at: bt,
                virtual_sol_reserves: opt_f64(vsol, i),
                virtual_token_reserves: opt_f64(vtok, i),
                real_sol_reserves: opt_f64(rsol, i),
                real_token_reserves: opt_f64(rtok, i),
                instruction_type: String::new(),
                instruction_labels: serde_json::Value::Null,
                venue: venue.value(i).to_string(),
            });
        }
    }
    if let Some(prev) = cur_mint.take() {
        tokens.push(TokenTrades {
            mint: prev,
            symbol: cur_symbol,
            trades: Arc::new(cur_trades),
        });
    }
    let hash = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("cached")
        .to_string();
    Ok(Corpus { tokens, hash })
}

fn col<'a, T: Array + 'static>(batch: &'a RecordBatch, i: usize) -> Result<&'a T> {
    batch
        .column(i)
        .as_any()
        .downcast_ref::<T>()
        .with_context(|| format!("corpus parquet: column {i} has unexpected type"))
}

fn col_str<'a>(batch: &'a RecordBatch, i: usize) -> Result<&'a StringArray> {
    col::<StringArray>(batch, i)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn trade(mint: &str, price: f64, buy: bool) -> Trade {
        let mut t = Trade::new(
            mint.into(),
            "wallet".into(),
            if buy { TradeType::Buy } else { TradeType::Sell },
            1.0,
            1.0 / price,
            "sig".into(),
            1,
            Utc::now(),
        );
        t.price_per_token = price;
        t.real_sol_reserves = Some(10.0);
        t.venue = "curve".into();
        t
    }

    #[test]
    fn corpus_parquet_round_trips_grouped_by_mint() {
        let tokens = vec![
            TokenTrades {
                mint: "m1".into(),
                symbol: "S1".into(),
                trades: Arc::new(vec![trade("m1", 1.0, true), trade("m1", 2.0, false)]),
            },
            TokenTrades {
                mint: "m2".into(),
                symbol: "S2".into(),
                trades: Arc::new(vec![trade("m2", 3.0, true)]),
            },
        ];
        let corpus = Corpus { tokens, hash: "h".into() };
        let path = std::env::temp_dir().join(format!("corpus_rt_{}.parquet", std::process::id()));
        write_corpus_parquet(&corpus, &path).unwrap();
        let back = read_corpus_parquet(&path).unwrap();

        assert_eq!(back.token_count(), 2);
        assert_eq!(back.trade_count(), 3);
        let m1 = back.tokens.iter().find(|t| t.mint == "m1").unwrap();
        assert_eq!(m1.trades.len(), 2);
        assert!((m1.trades[1].price_per_token - 2.0).abs() < 1e-9);
        assert_eq!(m1.trades[1].trade_type, TradeType::Sell);
        assert_eq!(m1.trades[0].real_sol_reserves, Some(10.0));
        std::fs::remove_file(&path).ok();
    }
}
