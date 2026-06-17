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
//! instead of re-hitting the DB. The full `Trade` never enters the sweep loop:
//! each token is projected **once at load** into a slim, wallet-interned
//! [`SweepTrade`] buffer (see [`crate::sweep::projection`]) — the hot loop walks
//! that, not the ~250 B `Trade`.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
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

use crate::models::trade::Trade;
use crate::state::token_cache::TokenCache;
use crate::storage::repositories::trade_repo::TradeSlimRow;
use crate::sweep::grouping::{extract_lamports, normalize_labels, TokenFingerprint};
use crate::sweep::projection::{project_trades, SweepTrade, WalletInterner};

/// One token's trade history, ready for `simulate`. `trades` is the slim
/// [`SweepTrade`] projection (wallet-interned, ~3× smaller than `Trade`), shared
/// (`Arc`) so building a sub-corpus per group is a refcount clone, not a copy.
/// `wallets` is the token-local `u32 → address` table the projection interns
/// against; the hot loop never reads it (cohort membership is pure `u32`) — it
/// exists only to write addresses back to the Parquet cache.
///
/// `fp` carries the token-creation fingerprint used only by the grouping layer
/// (never by `simulate`). The cache source fills it from the live token; the DB
/// source leaves it default until [`attach_fingerprints`] runs (so it works
/// whether trades came fresh from the DB or off the Parquet trade cache).
#[derive(Clone)]
pub struct TokenTrades {
    pub mint: String,
    pub symbol: String,
    pub trades: Arc<Vec<SweepTrade>>,
    /// `u32 → wallet address` for this token (interned by [`project_trades`]).
    pub wallets: Arc<Vec<Box<str>>>,
    pub fp: TokenFingerprint,
}

impl TokenTrades {
    /// Project a token's chronological `Trade` slice into the slim sweep buffer
    /// once, at load. Apply any corpus-wide trade filter (e.g. `curve_only`)
    /// **before** calling this — `SweepTrade` drops `venue`.
    pub fn from_trades(
        mint: String,
        symbol: String,
        fp: TokenFingerprint,
        trades: &[Trade],
    ) -> Self {
        let (rows, wallets) = project_trades(trades);
        Self {
            mint,
            symbol,
            fp,
            trades: Arc::new(rows),
            wallets: Arc::new(wallets),
        }
    }
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

/// Which slice of a token's lifetime the DB per-mint cap keeps when the token has
/// more than `per_mint_cap` lifetime trades. Only matters for high-volume tokens
/// past the cap; below it the whole history loads either way.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TradeWindow {
    /// Keep the **earliest** `per_mint_cap` trades — the launch window the entry
    /// logic (`find_scalp_entry`) decides on. The correct default for a historical
    /// backtest: replaying newest-first would silently drop the first minutes and
    /// resolve entry against a truncated mid-life slice (Rec 4).
    #[default]
    LaunchWindow,
    /// Keep the **newest** `per_mint_cap` trades — parity with the live cache's
    /// retained recent window, for a "what would I do right now" replay. A
    /// selectable mode; the grouped-sweep handler defaults to `LaunchWindow`.
    #[allow(dead_code)]
    Recent,
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
    /// Only tokens created strictly before this instant (DB source) — the upper
    /// bound of a date/time range. `None` ⇒ no upper bound.
    pub created_before: Option<DateTime<Utc>>,
    /// Per-mint trade cap for the DB batch query.
    pub per_mint_cap: i64,
    /// Which `per_mint_cap` slice to keep for over-cap tokens (DB source).
    pub window: TradeWindow,
    /// Drop AMM (post-migration) legs, keeping only bonding-curve trades.
    pub curve_only: bool,
}

impl Default for Selection {
    fn default() -> Self {
        Self {
            mints: None,
            token_cap: 5_000,
            created_after: None,
            created_before: None,
            per_mint_cap: crate::state::token_cache::MAX_TRADES_RETAINED as i64,
            window: TradeWindow::LaunchWindow,
            curve_only: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Memory bounding (Phase 0/1 of the sweep memory plan)
// ---------------------------------------------------------------------------

/// Default sweep per-mint trade cap (Phase 1.1). Far below the live
/// `MAX_TRADES_RETAINED` (50k): launch-window scalp entries decide on the first
/// minutes, so a few thousand trades/token is plenty and cuts a high-volume
/// token's corpus weight (and its entry/exit walk) ~10–25×. Override per box with
/// `SWEEP_PER_MINT_CAP`.
pub const SWEEP_DEFAULT_PER_MINT_CAP: i64 = 5_000;

/// Effective per-mint trade cap for a sweep `Selection`: `SWEEP_PER_MINT_CAP` if
/// set (≥1), else [`SWEEP_DEFAULT_PER_MINT_CAP`]. This is the second factor of the
/// corpus weight (`tokens × trades/token`); `token_cap` bounds the first.
pub fn sweep_per_mint_cap() -> i64 {
    std::env::var("SWEEP_PER_MINT_CAP")
        .ok()
        .and_then(|v| v.trim().parse::<i64>().ok())
        .filter(|&n| n >= 1)
        .unwrap_or(SWEEP_DEFAULT_PER_MINT_CAP)
}

/// Estimated resident bytes per projected [`SweepTrade`]: the struct itself plus a
/// small amortized share of the token-local `u32 → address` wallet table. Derived
/// from `size_of::<SweepTrade>()` so the Phase 0 corpus pre-check tracks struct
/// changes instead of silently drifting off a hardcoded constant. Phase 1.2 dropped
/// the heap-allocated base58 `tx_signature` the row used to carry (~88 B/trade), so
/// the only remaining off-struct cost is the amortized wallet-table share.
pub fn estimated_per_trade_bytes() -> usize {
    /// Amortized per-trade share of the per-token wallet intern table (the only
    /// heap a `SweepTrade` now contributes to beyond its own struct).
    const WALLET_TABLE_SHARE_BYTES: usize = 16;
    std::mem::size_of::<SweepTrade>() + WALLET_TABLE_SHARE_BYTES
}

/// Default ceiling (MB) on the **corpus** (the resident per-token trade buffers) a
/// single sweep may load. The corpus is the unbounded OOM-killer the
/// fold-accumulator guard (`SWEEP_MEMORY_BUDGET_MB`) does not cover; this turns a
/// process-killing mid-load OOM into a clean rejected request. ~512 MB suits a
/// 4 GB box (leaving room for the live caches + accumulators). Override with
/// `SWEEP_CORPUS_MEMORY_BUDGET_MB`.
const DEFAULT_CORPUS_MEMORY_BUDGET_MB: u64 = 512;

/// The corpus memory budget in bytes (`SWEEP_CORPUS_MEMORY_BUDGET_MB` or the
/// default), saturating so a fat-fingered override can't wrap.
pub fn corpus_memory_budget_bytes() -> u64 {
    std::env::var("SWEEP_CORPUS_MEMORY_BUDGET_MB")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|&m| m >= 1)
        .unwrap_or(DEFAULT_CORPUS_MEMORY_BUDGET_MB)
        .saturating_mul(1024 * 1024)
}

/// Default ceiling (MB) on **total** resident sweep memory at admission: current
/// process RSS (the live caches already resident — ~1 GB) + the corpus we're about
/// to load + the fold-accumulator budget. The corpus and accumulator budgets each
/// guard one axis, but they can pass *individually* yet *jointly* OOM once the live
/// caches are added — so Phase 0.2 sums all three against this one ceiling. ~3 GB
/// leaves headroom on a 4 GB box. Override with `SWEEP_TOTAL_MEMORY_BUDGET_MB`.
const DEFAULT_TOTAL_MEMORY_BUDGET_MB: u64 = 3_072;

/// The total-sweep memory budget in bytes (`SWEEP_TOTAL_MEMORY_BUDGET_MB` or the
/// default), saturating so an override can't wrap.
pub fn total_memory_budget_bytes() -> u64 {
    std::env::var("SWEEP_TOTAL_MEMORY_BUDGET_MB")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|&m| m >= 1)
        .unwrap_or(DEFAULT_TOTAL_MEMORY_BUDGET_MB)
        .saturating_mul(1024 * 1024)
}

/// Holistic admission gate (Phase 0.2). Rejects a corpus load that would either
/// (a) blow the **corpus-axis** budget on its own, or (b) push *total* resident
/// memory — current RSS (live caches) + this corpus + the fold-accumulator budget
/// — over [`total_memory_budget_bytes`]. The second check is the one the two
/// independent caps miss: each can clear in isolation while their sum OOMs the box.
/// RSS is best-effort; if it can't be read we fall back to the corpus-axis guard
/// alone (already the prior behaviour) rather than blocking the run.
fn admit_corpus_load(est_corpus: u64) -> Result<()> {
    let mb = |b: u64| b / (1024 * 1024);

    // (1) Corpus-axis guard — the unbounded mid-load OOM-killer.
    let corpus_budget = corpus_memory_budget_bytes();
    if est_corpus > corpus_budget {
        bail!(
            "corpus would need ~{} MB of resident trade buffers, over the {} MB corpus budget — \
             narrow the date range, lower token_cap/SWEEP_PER_MINT_CAP, or raise \
             SWEEP_CORPUS_MEMORY_BUDGET_MB",
            mb(est_corpus),
            mb(corpus_budget),
        );
    }

    // (2) Holistic guard — corpus + accumulators + already-resident live caches.
    let accum = crate::sweep::registry::sweep_memory_budget_bytes();
    let total_budget = total_memory_budget_bytes();
    match crate::sweep::obs::process_rss_bytes() {
        Some(rss) => {
            let projected = rss.saturating_add(est_corpus).saturating_add(accum);
            if projected > total_budget {
                bail!(
                    "sweep would need ~{} MB resident (live caches {} + corpus {} + fold {}), \
                     over the {} MB total budget — narrow the window, lower token_cap / \
                     SWEEP_PER_MINT_CAP / SWEEP_MEMORY_BUDGET_MB, or raise \
                     SWEEP_TOTAL_MEMORY_BUDGET_MB",
                    mb(projected),
                    mb(rss),
                    mb(est_corpus),
                    mb(accum),
                    mb(total_budget),
                );
            }
            tracing::info!(
                rss_mb = mb(rss),
                corpus_mb = mb(est_corpus),
                accum_budget_mb = mb(accum),
                projected_mb = mb(projected),
                total_budget_mb = mb(total_budget),
                "corpus: holistic admission within budget"
            );
        }
        None => {
            // RSS unreadable on this platform/run — corpus-axis check (passed) stands.
            tracing::info!(
                corpus_mb = mb(est_corpus),
                corpus_budget_mb = mb(corpus_budget),
                "corpus: pre-check within budget (RSS unavailable — corpus axis only)"
            );
        }
    }
    Ok(())
}

/// Source of a corpus — cache or DB, one interface.
#[async_trait]
pub trait CorpusSource {
    async fn load(&self, sel: &Selection) -> Result<Corpus>;
}

/// Apply the shared corpus-wide filters (curve-only) to a freshly built token,
/// then project to the slim [`SweepTrade`] buffer.
fn finalize_token(mint: String, symbol: String, mut trades: Vec<Trade>, sel: &Selection) -> TokenTrades {
    if sel.curve_only {
        trades.retain(|t| t.venue == "curve");
    }
    TokenTrades::from_trades(mint, symbol, TokenFingerprint::default(), &trades)
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
            if let Some(before) = sel.created_before {
                if st.token.created_at >= before {
                    continue;
                }
            }
            if let Some(allow) = &allow {
                if !allow.contains(&st.token.mint_address) {
                    continue;
                }
            }
            // The cache holds the decoded token, so fill the grouping fingerprint
            // here for free (the DB source defers it to `attach_fingerprints`).
            let fp = TokenFingerprint::from_token(&st.token);
            let mint = st.token.mint_address.clone();
            let symbol = st.token.symbol.clone();
            // Project to the slim sweep buffer under the shard guard. (We no longer
            // refcount-clone the live `Arc<Vec<Trade>>`: the sweep walks `SweepTrade`,
            // so the corpus is projected once here — a load-time cost, not the live
            // hot path.) `curve_only` filters `Trade` first, before `venue` is dropped.
            let tt = if sel.curve_only {
                let filtered: Vec<Trade> =
                    st.trades.iter().filter(|t| t.venue == "curve").cloned().collect();
                TokenTrades::from_trades(mint, symbol, fp, &filtered)
            } else {
                TokenTrades::from_trades(mint, symbol, fp, &st.trades)
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
    /// `per_mint_cap` slice (a per-mint `ROW_NUMBER` window bounds the worst case
    /// to `mints.len() * per_mint_cap` rows), grouped chronological per mint. The
    /// `window` picks **which** slice an over-cap token keeps: `LaunchWindow` ranks
    /// earliest-first so the entry-relevant launch prefix is always present (Rec 4);
    /// `Recent` ranks newest-first (live-cache parity). Mints with no trades are
    /// simply absent. Slim projection (no `ix_labels` JSONB) — the sweep never
    /// reads per-trade labels.
    async fn fetch_chunk(
        &self,
        mints: &[String],
        per_mint_cap: i64,
        window: TradeWindow,
    ) -> Result<HashMap<String, Vec<Trade>>> {
        // The partition order decides which `per_mint_cap` rows survive the cut;
        // the final result is re-sorted ASC either way, so each mint's run arrives
        // chronological. The two orderings are fixed column lists (no user input),
        // so formatting them into the statement is injection-safe.
        let partition_order = match window {
            TradeWindow::LaunchWindow => {
                "slot ASC, block_time ASC, tx_signature ASC, leg_index ASC"
            }
            TradeWindow::Recent => {
                "slot DESC, block_time DESC, tx_signature DESC, leg_index DESC"
            }
        };
        let sql = format!(
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
                         ORDER BY {partition_order}
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
            "#
        );
        let rows = sqlx::query_as::<_, TradeSlimRow>(&sql)
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
                 WHERE is_mayhem_mode = FALSE \
                   AND ($1::timestamptz IS NULL OR created_at >= $1) \
                   AND ($2::timestamptz IS NULL OR created_at < $2) \
                 ORDER BY created_at DESC LIMIT $3",
            )
            .bind(sel.created_after)
            .bind(sel.created_before)
            .bind(sel.token_cap as i64)
            .fetch_all(&self.pool)
            .await
            .context("selecting candidate tokens")?;
            Ok(rows)
        }
    }

    /// Phase 0 pre-check: estimate the resident corpus bytes **before** loading any
    /// trade rows, so an oversized population is rejected as a clean error instead
    /// of OOM-killing the backend mid-load. Counts `min(trade_count, per_mint_cap)`
    /// per candidate mint via one `GROUP BY` aggregate per chunk (no trade row ever
    /// materialized) and multiplies by [`estimated_per_trade_bytes`].
    pub async fn estimate_corpus_bytes(&self, sel: &Selection) -> Result<u64> {
        let candidates = self.candidates(sel).await?;
        let mints: Vec<String> = candidates.into_iter().map(|(m, _)| m).collect();
        let cap = sel.per_mint_cap.max(1) as u64;
        let mut total_trades: u64 = 0;
        for chunk in mints.chunks(self.chunk) {
            let rows: Vec<(String, i64)> = sqlx::query_as(
                "SELECT mint_address, count(*) FROM trades \
                 WHERE mint_address = ANY($1) GROUP BY mint_address",
            )
            .bind(chunk)
            .fetch_all(&self.pool)
            .await
            .context("estimating corpus size")?;
            for (_mint, n) in rows {
                total_trades = total_trades.saturating_add((n.max(0) as u64).min(cap));
            }
        }
        Ok(total_trades.saturating_mul(estimated_per_trade_bytes() as u64))
    }
}

/// A token's grouping fingerprint columns, projected straight from `tokens`. The
/// JSONB-nested `max_sol_cost` / `spendable_sol_in` are extracted in SQL as
/// bigint lamports (mirroring the live entry path's reader).
#[derive(sqlx::FromRow)]
struct FingerprintRow {
    mint_address: String,
    creator_wallet: String,
    token_program_id: Option<String>,
    initial_buy_sol: Option<f64>,
    cu_limit: Option<i64>,
    cu_price: Option<i64>,
    is_cashback_enabled: bool,
    /// Raw creation-instruction args; lamports are read in Rust via
    /// [`extract_lamports`] (wrapping `u64 as i64`), mirroring the cache-source
    /// `from_token` path. Reading them with a SQL `::bigint` cast instead would
    /// error on the rare malformed `u64 > i64::MAX` value and fail the whole sweep.
    initial_buy_instruction: Option<serde_json::Value>,
    instruction_labels: serde_json::Value,
}

/// Attach the grouping [`TokenFingerprint`] to every token in `corpus` via one
/// chunked `tokens` lookup keyed by mint. Kept independent of the (heavy) trade
/// load so it works whether the trades came fresh from the DB or off the Parquet
/// trade cache — option (a): the trade Parquet stays fingerprint-free, the
/// fingerprint is a cheap separate query. Tokens absent from `tokens` keep the
/// default (all-missing) fingerprint.
pub async fn attach_fingerprints(pool: &PgPool, corpus: &mut Corpus) -> Result<()> {
    const CHUNK: usize = 500;
    let mints: Vec<String> = corpus.tokens.iter().map(|t| t.mint.clone()).collect();
    let mut by_mint: HashMap<String, TokenFingerprint> = HashMap::with_capacity(mints.len());

    for chunk in mints.chunks(CHUNK) {
        let rows = sqlx::query_as::<_, FingerprintRow>(
            r#"
            SELECT mint_address, creator_wallet, token_program_id, initial_buy_sol,
                   cu_limit, cu_price, is_cashback_enabled,
                   initial_buy_instruction,
                   ix_labels AS instruction_labels
            FROM tokens
            WHERE mint_address = ANY($1)
            "#,
        )
        .bind(chunk)
        .fetch_all(pool)
        .await
        .context("loading token fingerprints for grouped sweep")?;

        for r in rows {
            by_mint.insert(
                r.mint_address.clone(),
                TokenFingerprint {
                    creator_wallet: r.creator_wallet,
                    token_program_id: r.token_program_id,
                    initial_buy_sol: r.initial_buy_sol,
                    cu_limit: r.cu_limit,
                    cu_price: r.cu_price,
                    is_cashback_enabled: r.is_cashback_enabled,
                    max_sol_cost: extract_lamports(r.initial_buy_instruction.as_ref(), "max_sol_cost"),
                    spendable_sol_in: extract_lamports(
                        r.initial_buy_instruction.as_ref(),
                        "spendable_sol_in",
                    ),
                    ix_labels: normalize_labels(&r.instruction_labels),
                },
            );
        }
    }

    let mut missing = 0u64;
    for tt in corpus.tokens.iter_mut() {
        match by_mint.get(&tt.mint) {
            Some(fp) => tt.fp = fp.clone(),
            None => missing += 1,
        }
    }
    if missing > 0 {
        tracing::warn!(missing, "attach_fingerprints: some corpus tokens had no tokens row");
    }
    Ok(())
}

#[async_trait]
impl CorpusSource for DbSource {
    async fn load(&self, sel: &Selection) -> Result<Corpus> {
        let candidates = self.candidates(sel).await?;
        let symbols: HashMap<String, String> = candidates.iter().cloned().collect();
        let mints: Vec<String> = candidates.into_iter().map(|(m, _)| m).collect();

        let mut tokens: Vec<TokenTrades> = Vec::with_capacity(mints.len());
        let mut capped = 0u64;
        for chunk in mints.chunks(self.chunk) {
            let grouped = self
                .fetch_chunk(chunk, sel.per_mint_cap, sel.window)
                .await
                .context("batched trade fetch for corpus chunk")?;
            for mint in chunk {
                if let Some(trades) = grouped.get(mint) {
                    if trades.is_empty() {
                        continue;
                    }
                    // A token at exactly the cap was (almost certainly) truncated —
                    // surface it so a too-small `SWEEP_PER_MINT_CAP` doesn't silently
                    // clip a slow exit (Phase 1.1 risk note).
                    if trades.len() as i64 >= sel.per_mint_cap {
                        capped += 1;
                    }
                    let symbol = symbols.get(mint).cloned().unwrap_or_default();
                    tokens.push(finalize_token(mint.clone(), symbol, trades.clone(), sel));
                }
            }
        }
        if capped > 0 {
            tracing::warn!(
                capped,
                per_mint_cap = sel.per_mint_cap,
                "corpus: tokens hit the per-mint trade cap (raise SWEEP_PER_MINT_CAP if exits look truncated)"
            );
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

/// Default on-disk location for cached corpora. The `v2` tag is the slim-schema
/// version: Phase 1.2 dropped the `tx_signature` column, shifting later columns, so
/// a pre-1.2 `corpus_<hash>.parquet` must never be read back with the new layout —
/// bumping the prefix makes stale files simply miss (and rebuild) instead.
pub fn corpus_cache_path(dir: &Path, hash: &str) -> PathBuf {
    dir.join(format!("corpus_v2_{hash}.parquet"))
}

/// Where corpus Parquet caches live: `$SWEEP_CORPUS_CACHE_DIR` if set, else a
/// stable subdir of the OS temp dir. The cache is a pure performance optimisation
/// (a miss just reloads from the DB), so a temp default is fine and needs no config.
pub fn corpus_cache_dir() -> PathBuf {
    std::env::var_os("SWEEP_CORPUS_CACHE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("pumpfun-sweep-corpus"))
}

/// Stable cache key for a corpus *selection* — the inputs that determine the
/// realised population, **not** the population itself (which only exists after a
/// load). Two runs that differ only in grouping / axes / `min_tokens` / `method`
/// over the same window hash equal here, so they share one cached trade Parquet.
pub fn selection_cache_key(sel: &Selection) -> String {
    let mut h = DefaultHasher::new();
    sel.token_cap.hash(&mut h);
    sel.per_mint_cap.hash(&mut h);
    (sel.window as u8).hash(&mut h);
    sel.curve_only.hash(&mut h);
    sel.created_after.map(|t| t.timestamp_millis()).hash(&mut h);
    sel.created_before.map(|t| t.timestamp_millis()).hash(&mut h);
    if let Some(mints) = &sel.mints {
        let mut ms: Vec<&str> = mints.iter().map(String::as_str).collect();
        ms.sort_unstable();
        for m in ms {
            m.hash(&mut h);
        }
    }
    format!("{:016x}", h.finish())
}

/// In-window tokens keep receiving trades for a while after creation, so a
/// selection is only stable enough to cache once its upper bound is at least this
/// many hours in the past. An open-ended or recent window still mutates → reload.
const CORPUS_CACHE_SETTLE_HOURS: i64 = 2;

/// Whether a selection's window is closed and settled enough that its trade set
/// won't keep changing — the precondition for serving it from the Parquet cache.
pub fn selection_is_cacheable(sel: &Selection, now: DateTime<Utc>) -> bool {
    sel.created_before
        .map(|before| before <= now - chrono::Duration::hours(CORPUS_CACHE_SETTLE_HOURS))
        .unwrap_or(false)
}

/// Load a grouped-sweep corpus, reusing a selection-keyed Parquet cache when the
/// window is closed/settled (Rec 3). For the "run many sweeps over the same window
/// for hours" workflow this turns every repeat run's DB load (select candidates +
/// chunked trade fetch + deserialize up to `token_cap` tokens) into a local-disk
/// read. An open/recent window is never cached (its trades still mutate); `fresh`
/// forces a rebuild + cache rewrite even for a cacheable window.
///
/// Fingerprints are **not** cached here — the caller runs the cheap separate
/// [`attach_fingerprints`] lookup after loading (the trade Parquet is
/// fingerprint-free), so this works on both the cache-hit and DB paths.
pub async fn load_grouped_corpus(
    db: PgPool,
    sel: &Selection,
    cache_dir: &Path,
    fresh: bool,
) -> Result<Corpus> {
    let source = DbSource::new(db);
    let cacheable = selection_is_cacheable(sel, Utc::now());
    let key = selection_cache_key(sel);
    // A cache hit reads an already-bounded Parquet off disk (written from a capped
    // corpus) and skips the DB entirely (principle 5), so only pre-check the budget
    // when we're about to actually load trade rows from the DB.
    let cache_hit = cacheable && !fresh && corpus_cache_path(cache_dir, &key).exists();
    if !cache_hit {
        // Holistic admission (Phase 0.2): corpus axis + accumulators + current RSS
        // (live caches) vs one total ceiling — see [`admit_corpus_load`].
        let est = source.estimate_corpus_bytes(sel).await?;
        admit_corpus_load(est)?;
    }

    if !cacheable {
        return source.load(sel).await;
    }
    if fresh {
        // Drop any stale cache so `load_or_build` re-loads from the DB and rewrites.
        std::fs::remove_file(corpus_cache_path(cache_dir, &key)).ok();
    }
    load_or_build(&source, sel, cache_dir, &key).await
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

/// The slim corpus-cache schema — exactly the [`SweepTrade`] fields plus the
/// per-token `mint`/`symbol` and the `wallet` address (re-interned to a `u32` on
/// read). `tx_signature` (Phase 1.2 — no sweep fn reads it), `venue` (a load-time
/// filter), and `vtok`/`rtok` (read by no sweep fn) are intentionally dropped vs.
/// the full `Trade`.
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
        Field::new("vsol", DataType::Float64, true),
        Field::new("rsol", DataType::Float64, true),
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
    let mut vsol = Float64Builder::new();
    let mut rsol = Float64Builder::new();
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
                    Arc::new(vsol.finish()),
                    Arc::new(rsol.finish()),
                ],
            )?;
            writer.write(&batch)?;
        }};
    }

    for token in &corpus.tokens {
        for t in token.trades.iter() {
            mint.append_value(&token.mint);
            symbol.append_value(&token.symbol);
            // Re-materialise the wallet address from the token-local intern table.
            wallet.append_value(&*token.wallets[t.wallet as usize]);
            is_buy.append_value(t.is_buy);
            sol_amount.append_value(t.sol_amount);
            token_amount.append_value(t.token_amount);
            price.append_value(t.price_per_token);
            slot.append_value(t.slot as i64);
            block_time.append_value(t.block_time.timestamp());
            leg_index.append_value(t.leg_index as i32);
            vsol.append_option(t.virtual_sol_reserves);
            rsol.append_option(t.real_sol_reserves);
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

/// Stream a slim corpus Parquet **token by token**, invoking `emit` with each
/// completed [`TokenTrades`] as soon as the mint-contiguous rows for that token end
/// (the file is written one contiguous run per mint, so a mint change closes the
/// previous token). Only one token's `SweepTrade` buffer is resident at a time — the
/// Phase 2a enabler for a streamed grouped fold (peak corpus memory `O(in-flight
/// tokens)` instead of `O(all tokens)`). `emit` returning `Err` aborts the scan
/// (e.g. a cancelled sweep). [`read_corpus_parquet`] is the collect-into-`Vec`
/// wrapper, so the existing round-trip test covers this path.
pub fn stream_corpus_parquet<F>(path: &Path, mut emit: F) -> Result<()>
where
    F: FnMut(TokenTrades) -> Result<()>,
{
    let file = std::fs::File::open(path)?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let reader = builder.build()?;

    let mut cur_mint: Option<String> = None;
    let mut cur_symbol = String::new();
    let mut cur_trades: Vec<SweepTrade> = Vec::new();
    let mut cur_interner = WalletInterner::default();

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
        let vsol = col::<Float64Array>(&batch, 10)?;
        let rsol = col::<Float64Array>(&batch, 11)?;

        for i in 0..batch.num_rows() {
            let m = mint.value(i);
            if cur_mint.as_deref() != Some(m) {
                if let Some(prev) = cur_mint.take() {
                    emit(TokenTrades {
                        mint: prev,
                        symbol: std::mem::take(&mut cur_symbol),
                        fp: TokenFingerprint::default(),
                        trades: Arc::new(std::mem::take(&mut cur_trades)),
                        wallets: Arc::new(std::mem::take(&mut cur_interner).into_table()),
                    })?;
                }
                cur_mint = Some(m.to_string());
                cur_symbol = symbol.value(i).to_string();
            }
            let bt = DateTime::<Utc>::from_timestamp(block_time.value(i), 0).unwrap_or_else(Utc::now);
            cur_trades.push(SweepTrade {
                block_time: bt,
                sol_amount: sol_amount.value(i),
                token_amount: token_amount.value(i),
                price_per_token: price.value(i),
                virtual_sol_reserves: opt_f64(vsol, i),
                real_sol_reserves: opt_f64(rsol, i),
                slot: slot.value(i) as u64,
                wallet: cur_interner.intern(wallet.value(i)),
                leg_index: leg_index.value(i) as u32,
                is_buy: is_buy.value(i),
            });
        }
    }
    if let Some(prev) = cur_mint.take() {
        emit(TokenTrades {
            mint: prev,
            symbol: cur_symbol,
            fp: TokenFingerprint::default(),
            trades: Arc::new(cur_trades),
            wallets: Arc::new(cur_interner.into_table()),
        })?;
    }
    Ok(())
}

/// Read a slim corpus Parquet back into [`Corpus`], re-grouping the
/// mint-contiguous rows into per-token [`SweepTrade`] buffers and re-interning
/// each token's wallet addresses to a token-local `u32`. The dropped `Trade`
/// fields are never reconstructed — no sweep fn reads them. Thin collect-into-`Vec`
/// wrapper over [`stream_corpus_parquet`].
pub fn read_corpus_parquet(path: &Path) -> Result<Corpus> {
    let mut tokens: Vec<TokenTrades> = Vec::new();
    stream_corpus_parquet(path, |tt| {
        tokens.push(tt);
        Ok(())
    })?;
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
    use crate::models::trade::TradeType;
    use chrono::Utc;

    fn trade(mint: &str, wallet: &str, price: f64, buy: bool) -> Trade {
        let mut t = Trade::new(
            mint.into(),
            wallet.into(),
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

    fn token(mint: &str, symbol: &str, trades: &[Trade]) -> TokenTrades {
        TokenTrades::from_trades(mint.into(), symbol.into(), TokenFingerprint::default(), trades)
    }

    #[test]
    fn per_trade_estimate_covers_struct_and_heap() {
        // The Phase 0 pre-check must over- (never under-) count: a SweepTrade owns a
        // heap base58 signature beyond its struct size, so the estimate must exceed
        // size_of alone or it would under-budget and let an OOM through.
        assert!(estimated_per_trade_bytes() > std::mem::size_of::<SweepTrade>());
    }

    #[test]
    fn sweep_per_mint_cap_defaults_below_live_retention() {
        // The sweep cap must sit well under the live MAX_TRADES_RETAINED (50k) —
        // that gap is the Phase 1.1 corpus cut.
        assert!(SWEEP_DEFAULT_PER_MINT_CAP < crate::state::token_cache::MAX_TRADES_RETAINED as i64);
    }

    #[test]
    fn default_window_is_launch_window() {
        // A historical backtest must keep the launch prefix by default (Rec 4).
        assert_eq!(Selection::default().window, TradeWindow::LaunchWindow);
        assert_eq!(TradeWindow::default(), TradeWindow::LaunchWindow);
    }

    #[test]
    fn cache_key_distinguishes_window() {
        // Two runs over the same window but different trade slices must not share a
        // cached corpus.
        let launch = Selection { window: TradeWindow::LaunchWindow, ..Selection::default() };
        let recent = Selection { window: TradeWindow::Recent, ..Selection::default() };
        assert_ne!(selection_cache_key(&launch), selection_cache_key(&recent));
    }

    #[test]
    fn corpus_parquet_round_trips_grouped_by_mint() {
        let corpus = Corpus {
            tokens: vec![
                // Two distinct wallets so the re-interned table round-trips both ids.
                token(
                    "m1",
                    "S1",
                    &[trade("m1", "wA", 1.0, true), trade("m1", "wB", 2.0, false)],
                ),
                token("m2", "S2", &[trade("m2", "wA", 3.0, true)]),
            ],
            hash: "h".into(),
        };
        let path = std::env::temp_dir().join(format!("corpus_rt_{}.parquet", std::process::id()));
        write_corpus_parquet(&corpus, &path).unwrap();
        let back = read_corpus_parquet(&path).unwrap();

        assert_eq!(back.token_count(), 2);
        assert_eq!(back.trade_count(), 3);
        let m1 = back.tokens.iter().find(|t| t.mint == "m1").unwrap();
        assert_eq!(m1.trades.len(), 2);
        assert!((m1.trades[1].price_per_token - 2.0).abs() < 1e-9);
        assert!(!m1.trades[1].is_buy, "second m1 trade is a sell");
        assert_eq!(m1.trades[0].real_sol_reserves, Some(10.0));
        // Wallets re-interned per token: m1 has two distinct addresses, both
        // recoverable through the token-local table.
        assert_eq!(m1.wallets.len(), 2);
        assert_ne!(m1.trades[0].wallet, m1.trades[1].wallet);
        assert_eq!(&*m1.wallets[m1.trades[0].wallet as usize], "wA");
        assert_eq!(&*m1.wallets[m1.trades[1].wallet as usize], "wB");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn streaming_reader_yields_one_token_at_a_time_matching_collect() {
        // The Phase 2a streaming primitive must emit exactly the same per-token
        // buffers (count, order, trade counts) the collect-into-Vec reader builds,
        // and must surface an emit `Err` as an early abort (a cancelled stream).
        let corpus = Corpus {
            tokens: vec![
                token("m1", "S1", &[trade("m1", "wA", 1.0, true), trade("m1", "wB", 2.0, false)]),
                token("m2", "S2", &[trade("m2", "wA", 3.0, true)]),
                token("m3", "S3", &[trade("m3", "wC", 4.0, true)]),
            ],
            hash: "h".into(),
        };
        let path =
            std::env::temp_dir().join(format!("corpus_stream_{}.parquet", std::process::id()));
        write_corpus_parquet(&corpus, &path).unwrap();

        // Streamed tokens equal the collected reader's tokens, in order.
        let collected = read_corpus_parquet(&path).unwrap();
        let mut streamed: Vec<(String, usize)> = Vec::new();
        stream_corpus_parquet(&path, |tt| {
            streamed.push((tt.mint.clone(), tt.trades.len()));
            Ok(())
        })
        .unwrap();
        let expected: Vec<(String, usize)> =
            collected.tokens.iter().map(|t| (t.mint.clone(), t.trades.len())).collect();
        assert_eq!(streamed, expected);
        assert_eq!(streamed.len(), 3, "one emit per mint");

        // An `Err` from `emit` aborts the scan after the first token (cancel path).
        let mut seen = 0usize;
        let err = stream_corpus_parquet(&path, |_tt| {
            seen += 1;
            anyhow::bail!("stop")
        });
        assert!(err.is_err());
        assert_eq!(seen, 1, "emit Err stops the stream immediately");

        std::fs::remove_file(&path).ok();
    }
}
