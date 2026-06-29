use std::collections::HashSet;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::trade::{Trade, TradeType};
use crate::storage::repositories::wallet_dict_repo::WalletDictRepo;

/// Mints per round-trip for the startup cache-seed scans. Bounds each `= ANY($1)`
/// array so Postgres keeps using the per-mint indexes instead of falling back to a
/// full-table seq scan on a huge array (mirrors `sweep::corpus::DbSource` chunking).
const SEED_MINT_CHUNK: usize = 1000;

/// Rows per `insert_many` statement. A single Postgres statement is capped at
/// 65535 bind parameters (the wire protocol's int16 count); at 13 binds/row the
/// hard ceiling is 5041 rows, so this stays well under it. sqlx 0.6 silently
/// wraps `len() as i16` past the cap, corrupting the Parse/Bind message into a
/// Postgres parse error — exactly what the token_sync backfill hit on busy mints.
const TRADE_INSERT_CHUNK: usize = 3000;

pub struct TradeRepo {
    pool: PgPool,
}

impl Clone for TradeRepo {
    fn clone(&self) -> Self {
        Self { pool: self.pool.clone() }
    }
}

// ---------------------------------------------------------------------------
// DB row
//
// The NEW `trades` table stores integers (lamports / raw token units) and a raw
// 64-byte signature (BYTEA); the wallet address is resolved in-SQL by joining
// `wallet_dict`. The runtime `Trade` model is unchanged (f64 amounts, base58
// signature string), so every read path reconstructs the model from the integer
// columns via the conversion helpers at the bottom of this file. Columns the new
// table dropped (`id`, `price_per_token`, `received_at`, `ix_type`, `ix_labels`,
// `real_*_reserves`) are synthesized on read.
// ---------------------------------------------------------------------------

/// One row read from the new `trades` table joined to `wallet_dict`. All amounts
/// are integers (lamports / raw token units); `tx_signature` is the raw 64-byte
/// signature; `wallet_address` is the joined-in base58 string.
#[derive(sqlx::FromRow)]
struct TradeDbRow {
    mint_address: String,
    wallet_address: String,
    trade_type: String,
    sol_amount: i64,
    token_amount: i64,
    tx_signature: Vec<u8>,
    // Defaulted so the read paths that don't project `tx_index` (it's not consumed
    // downstream — ordering is resolved in SQL) still map cleanly to 0.
    #[sqlx(default)]
    tx_index: i32,
    leg_index: i16,
    slot: i64,
    block_time: DateTime<Utc>,
    virtual_sol_reserves: Option<i64>,
    virtual_token_reserves: Option<i64>,
    venue: String,
}

impl TryFrom<TradeDbRow> for Trade {
    type Error = anyhow::Error;

    fn try_from(r: TradeDbRow) -> Result<Self, Self::Error> {
        let trade_type = match r.trade_type.as_str() {
            "buy" => TradeType::Buy,
            "sell" => TradeType::Sell,
            other => anyhow::bail!("Unknown trade_type in DB: {other}"),
        };

        // Reconstruct the model amounts from the integer columns. SOL is f64 (human
        // SOL from lamports); token_amount stays an exact integer (no f64 round-trip).
        let sol_amount = lamports_to_sol(r.sol_amount);
        let token_amount = r.token_amount as u64;

        Ok(Self {
            // Synthesized: the new table has no `id` column.
            id: Uuid::new_v4(),
            mint_address: r.mint_address,
            wallet_address: r.wallet_address,
            trade_type,
            sol_amount,
            token_amount,
            // Derived: the new table has no `price_per_token` column. The ratio is
            // computed in f64 (token cast at the divide).
            price_per_token: price_of(sol_amount, token_amount as f64),
            tx_signature: sig_bytes_to_base58(&r.tx_signature),
            tx_index: r.tx_index as u32,
            leg_index: r.leg_index as u32,
            slot: r.slot as u64,
            block_time: r.block_time,
            // Synthesized: the new table has no `received_at`; reuse block_time.
            received_at: r.block_time,
            virtual_sol_reserves: r.virtual_sol_reserves.map(lamports_to_sol),
            virtual_token_reserves: r.virtual_token_reserves.map(|v| v as u64),
            // The new table dropped the real_* reserve columns.
            real_sol_reserves: None,
            real_token_reserves: None,
            // Synthesized "Buy"/"Sell" instruction label from the trade side.
            instruction_type: ix_type_str(trade_type).to_string(),
            instruction_labels: serde_json::Value::Null,
            venue: r.venue,
        })
    }
}

fn trade_type_str(t: TradeType) -> &'static str {
    match t {
        TradeType::Buy => "buy",
        TradeType::Sell => "sell",
    }
}

/// "Buy"/"Sell" instruction-type label synthesized from the trade side (the new
/// table dropped the `ix_type` column).
fn ix_type_str(t: TradeType) -> &'static str {
    match t {
        TradeType::Buy => "Buy",
        TradeType::Sell => "Sell",
    }
}

// ---------------------------------------------------------------------------
// Repo
// ---------------------------------------------------------------------------

impl TradeRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Insert a trade. `ON CONFLICT DO NOTHING` on the natural dedup key
    /// `(block_time, tx_signature, leg_index)` — the table's PRIMARY KEY. This was
    /// `DO UPDATE` under the old schema; per the TimescaleDB plan we switch to
    /// DO NOTHING because compressed chunks are update-hostile and the first write
    /// already carries the correct reserves, so a replay has nothing to refresh.
    ///
    /// The single wallet is interned into `wallet_dict` first so the row references
    /// it by a compact `wallet_id` (INTEGER) instead of the base58 string.
    pub async fn insert(&self, trade: &Trade) -> anyhow::Result<()> {
        let wallet_id = WalletDictRepo::new(self.pool.clone())
            .intern(&trade.wallet_address)
            .await?;

        sqlx::query(
            r#"
            INSERT INTO trades
                (mint_address, wallet_id, trade_type, venue,
                 sol_amount, token_amount,
                 virtual_sol_reserves, virtual_token_reserves,
                 slot, tx_index, leg_index, block_time, tx_signature)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
            ON CONFLICT (block_time, tx_signature, leg_index) DO NOTHING
            "#,
        )
        .bind(&trade.mint_address)
        .bind(wallet_id)
        .bind(trade_type_str(trade.trade_type))
        .bind(&trade.venue)
        .bind(sol_to_lamports(trade.sol_amount))
        .bind(trade.token_amount as i64)
        .bind(trade.virtual_sol_reserves.map(sol_to_lamports_opt))
        .bind(trade.virtual_token_reserves.map(|v| v as i64))
        .bind(trade.slot as i64)
        .bind(trade.tx_index as i32)
        .bind(trade.leg_index as i16)
        .bind(trade.block_time)
        .bind(sig_base58_to_bytes(&trade.tx_signature)?)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Bulk version of [`insert`] — one multi-row statement per chunk, same
    /// `ON CONFLICT DO NOTHING` dedup on the `(block_time, tx_signature, leg_index)`
    /// primary key. Callers SHOULD dedup by that key first; DO NOTHING tolerates
    /// duplicates within a flush regardless (the first write wins). Used by the live
    /// ingest DB-writer to collapse a flush into a single round-trip, and by the
    /// token_sync backfill.
    ///
    /// All wallets are interned in one batch first (`intern_many`), then each row
    /// binds its `wallet_id` from the resulting map.
    ///
    /// Chunked at [`TRADE_INSERT_CHUNK`]: a single statement is capped at 65535
    /// bind parameters (the wire protocol's int16 count), and sqlx 0.6 has no
    /// guard — it writes `len() as i16`, so past the ceiling the count silently
    /// wraps and Postgres rejects the malformed Parse/Bind ("DB parse error"). At
    /// 13 binds/row the chunk stays well under the ceiling. Each chunk is safe to
    /// retry (DO NOTHING is idempotent).
    pub async fn insert_many(&self, trades: &[Trade]) -> anyhow::Result<()> {
        if trades.is_empty() {
            return Ok(());
        }

        // Intern every distinct wallet up front, then look each row's id up from
        // the map. One batched round-trip instead of one intern per row.
        let unique: Vec<String> = {
            let mut seen: HashSet<&str> = HashSet::new();
            let mut out: Vec<String> = Vec::new();
            for t in trades {
                if seen.insert(t.wallet_address.as_str()) {
                    out.push(t.wallet_address.clone());
                }
            }
            out
        };
        let wallet_ids = WalletDictRepo::new(self.pool.clone())
            .intern_many(&unique)
            .await?;

        for chunk in trades.chunks(TRADE_INSERT_CHUNK) {
            let mut qb: sqlx::QueryBuilder<sqlx::Postgres> = sqlx::QueryBuilder::new(
                "INSERT INTO trades \
                 (mint_address, wallet_id, trade_type, venue, sol_amount, token_amount, \
                  virtual_sol_reserves, virtual_token_reserves, slot, tx_index, leg_index, \
                  block_time, tx_signature) ",
            );
            // `push_values` cannot bubble a Result, so pre-resolve the fallible
            // signature decode into the loop's error path is not possible here;
            // instead we decode eagerly and bind the bytes (decode errors surface
            // as an empty-vec bind — but the model's signatures come from the chain
            // and are valid, so this is the live path). For robustness the decode
            // falls back to an empty Vec rather than panicking.
            qb.push_values(chunk, |mut b, t| {
                let wallet_id = wallet_ids.get(&t.wallet_address).copied().unwrap_or_default();
                b.push_bind(&t.mint_address)
                    .push_bind(wallet_id)
                    .push_bind(trade_type_str(t.trade_type))
                    .push_bind(&t.venue)
                    .push_bind(sol_to_lamports(t.sol_amount))
                    .push_bind(t.token_amount as i64)
                    .push_bind(t.virtual_sol_reserves.map(sol_to_lamports_opt))
                    .push_bind(t.virtual_token_reserves.map(|v| v as i64))
                    .push_bind(t.slot as i64)
                    .push_bind(t.tx_index as i32)
                    .push_bind(t.leg_index as i16)
                    .push_bind(t.block_time)
                    .push_bind(sig_base58_to_bytes(&t.tx_signature).unwrap_or_default());
            });
            qb.push(" ON CONFLICT (block_time, tx_signature, leg_index) DO NOTHING");
            qb.build().execute(&self.pool).await?;
        }

        Ok(())
    }

    /// Signature of the most recently saved trade for a token on a specific
    /// venue (`"curve"` or `"amm"`), if any. Used as the `until` boundary for
    /// incremental syncs so each venue resumes from its own last saved trade.
    /// Ordered by the execution-order key (slot, tx_index, leg_index) and the raw
    /// signature bytes are decoded back to base58 for the caller.
    pub async fn latest_signature(
        &self,
        mint: &str,
        venue: &str,
    ) -> anyhow::Result<Option<String>> {
        let bytes: Option<Vec<u8>> = sqlx::query_scalar(
            r#"
            SELECT tx_signature
            FROM trades
            WHERE mint_address = $1 AND venue = $2
            ORDER BY slot DESC, tx_index DESC, leg_index DESC
            LIMIT 1
            "#,
        )
        .bind(mint)
        .bind(venue)
        .fetch_optional(&self.pool)
        .await?;

        Ok(bytes.map(|b| sig_bytes_to_base58(&b)))
    }

    /// All transaction signatures already saved for a token on a venue
    /// (`"curve"` or `"amm"`). The incremental sync uses this to skip
    /// `getTransaction` for trades it already has, so it doesn't re-spend Helius
    /// RPC credits re-downloading them. Returned as a set of base58 strings for
    /// O(1) membership tests.
    ///
    /// `candidates` is the list of signatures the sync is about to fetch; the query
    /// intersects against it (`tx_signature = ANY($3)`) so Postgres returns only the
    /// already-saved sigs among that page. The slot floor now reads the venue's
    /// watermark from `token_sync_state.last_slot` (was `tokens_info.last_synced_*`),
    /// COALESCEd to 0 before the first sync stamps a watermark. An empty `candidates`
    /// short-circuits to an empty set.
    pub async fn saved_signatures(
        &self,
        mint: &str,
        venue: &str,
        candidates: &[String],
    ) -> anyhow::Result<HashSet<String>> {
        if candidates.is_empty() {
            return Ok(HashSet::new());
        }
        // Translate candidate base58 signatures to raw bytes for the BYTEA `= ANY`.
        let candidate_bytes: Vec<Vec<u8>> = candidates
            .iter()
            .map(|s| sig_base58_to_bytes(s))
            .collect::<anyhow::Result<_>>()?;

        let rows: Vec<(Vec<u8>,)> = sqlx::query_as(
            r#"
            SELECT DISTINCT t.tx_signature
            FROM trades t
            WHERE t.mint_address = $1
              AND t.venue = $2
              AND t.tx_signature = ANY($3)
              AND t.slot >= COALESCE(
                  (SELECT s.last_slot
                   FROM token_sync_state s
                   WHERE s.mint_address = $1 AND s.venue = $2),
                  0)
            "#,
        )
        .bind(mint)
        .bind(venue)
        .bind(&candidate_bytes)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|(b,)| sig_bytes_to_base58(&b)).collect())
    }

    /// Most-recent trade by `wallet` on `mint` of a given side, or `None`.
    /// Filters in SQL and fetches a single row instead of pulling N rows and
    /// scanning them in Rust. The wallet is first translated to its interned id; if
    /// it has no id it has no trades, so we return `None` without touching `trades`.
    ///
    /// No longer on the entry/exit-confirm path (1C replaced "latest buy/sell for
    /// the pair" with per-signature attribution — see [`Self::find_fill_by_signature`]
    /// / [`Self::sum_legs_by_signatures`]); used for ad-hoc lookups and the
    /// ManualSell external-clear detection path.
    pub async fn find_latest_by_wallet_mint_type(
        &self,
        wallet: &str,
        mint: &str,
        trade_type: TradeType,
    ) -> anyhow::Result<Option<Trade>> {
        let Some(wallet_id) = WalletDictRepo::new(self.pool.clone()).id_for(wallet).await? else {
            return Ok(None);
        };
        let row = sqlx::query_as::<_, TradeDbRow>(
            r#"
            SELECT t.mint_address, w.address AS wallet_address, t.trade_type, t.venue,
                   t.sol_amount, t.token_amount,
                   t.virtual_sol_reserves, t.virtual_token_reserves,
                   t.slot, t.tx_index, t.leg_index, t.block_time, t.tx_signature
            FROM trades t
            JOIN wallet_dict w ON w.id = t.wallet_id
            WHERE t.wallet_id = $1 AND t.mint_address = $2 AND t.trade_type = $3
            ORDER BY t.slot DESC, t.tx_index DESC, t.leg_index DESC
            LIMIT 1
            "#,
        )
        .bind(wallet_id)
        .bind(mint)
        .bind(trade_type_str(trade_type))
        .fetch_optional(&self.pool)
        .await?;

        row.map(Trade::try_from).transpose()
    }

    /// Find all trades for a token in execution order (slot, tx_index, leg_index).
    /// Joins `wallet_dict` to recover each trade's wallet address.
    pub async fn find_by_mint_all(&self, mint: &str) -> anyhow::Result<Vec<Trade>> {
        let rows = sqlx::query_as::<_, TradeDbRow>(
            r#"
            SELECT t.mint_address, w.address AS wallet_address, t.trade_type, t.venue,
                   t.sol_amount, t.token_amount,
                   t.virtual_sol_reserves, t.virtual_token_reserves,
                   t.slot, t.tx_index, t.leg_index, t.block_time, t.tx_signature
            FROM trades t
            JOIN wallet_dict w ON w.id = t.wallet_id
            WHERE t.mint_address = $1
            ORDER BY t.slot ASC, t.tx_index ASC, t.leg_index ASC
            "#,
        )
        .bind(mint)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(Trade::try_from).collect()
    }

    /// Find all trades for a *batch* of tokens in one round-trip, grouped per
    /// mint and each group in the same execution order as [`find_by_mint_all`].
    /// The backtest uses this to fetch a chunk of candidate mints with a single
    /// query instead of one query per token: same total rows, but ~`mints.len()`×
    /// fewer round-trips and PgPool connections held.
    ///
    /// Bounded by the caller's chunk size (never the full `trades` table). Mints
    /// with no trades are simply absent from the returned map.
    pub async fn find_by_mints_all(
        &self,
        mints: &[String],
    ) -> anyhow::Result<std::collections::HashMap<String, Vec<Trade>>> {
        // `mint_address` leads the ORDER BY so each mint's rows arrive as one
        // contiguous run already in execution order; grouping is then a single
        // linear pass with no per-mint sort.
        let rows = sqlx::query_as::<_, TradeDbRow>(
            r#"
            SELECT t.mint_address, w.address AS wallet_address, t.trade_type, t.venue,
                   t.sol_amount, t.token_amount,
                   t.virtual_sol_reserves, t.virtual_token_reserves,
                   t.slot, t.tx_index, t.leg_index, t.block_time, t.tx_signature
            FROM trades t
            JOIN wallet_dict w ON w.id = t.wallet_id
            WHERE t.mint_address = ANY($1)
            ORDER BY t.mint_address ASC, t.slot ASC, t.tx_index ASC, t.leg_index ASC
            "#,
        )
        .bind(mints)
        .fetch_all(&self.pool)
        .await?;

        let mut grouped: std::collections::HashMap<String, Vec<Trade>> =
            std::collections::HashMap::with_capacity(mints.len());
        for row in rows {
            let trade = Trade::try_from(row)?;
            grouped
                .entry(trade.mint_address.clone())
                .or_default()
                .push(trade);
        }
        Ok(grouped)
    }

    /// Find trades for a token in execution order, bounded by `limit`/`offset`.
    /// Same ordering as `find_by_mint_all` but paged so a high-volume token can't
    /// produce an unbounded response (the API never has to materialise every row).
    pub async fn find_by_mint_paged(
        &self,
        mint: &str,
        limit: i64,
        offset: i64,
    ) -> anyhow::Result<Vec<Trade>> {
        let rows = sqlx::query_as::<_, TradeDbRow>(
            r#"
            SELECT t.mint_address, w.address AS wallet_address, t.trade_type, t.venue,
                   t.sol_amount, t.token_amount,
                   t.virtual_sol_reserves, t.virtual_token_reserves,
                   t.slot, t.tx_index, t.leg_index, t.block_time, t.tx_signature
            FROM trades t
            JOIN wallet_dict w ON w.id = t.wallet_id
            WHERE t.mint_address = $1
            ORDER BY t.slot ASC, t.tx_index ASC, t.leg_index ASC
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(mint)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(Trade::try_from).collect()
    }

    /// Sum the legs of one transaction `signature` for `(wallet, mint, side)`,
    /// rolled up into a [`SigLegs`] (Σtokens, Σsol, first/last leg time). `None`
    /// when the signature has no matching trade indexed yet.
    ///
    /// This is the **per-signature entry attribution** primitive: the snipe buy
    /// already returns its own submitted signature, so the entry fill is recovered
    /// by *that* signature instead of `find_latest_by_wallet_mint_type` (the latest
    /// buy for the pair) — which, with two concurrent positions on the same token,
    /// would adopt the same fill twice.
    pub async fn find_fill_by_signature(
        &self,
        wallet: &str,
        mint: &str,
        signature: &str,
    ) -> anyhow::Result<Option<SigLegs>> {
        self.sum_legs_by_signatures(wallet, mint, std::slice::from_ref(&signature.to_string()), TradeType::Buy)
            .await
    }

    /// Sum the legs of a *set* of this position's own transaction `signatures` for
    /// `(wallet, mint, side)` into a single [`SigLegs`]. Used to confirm an exit by
    /// summing the position's OWN sell signatures' token legs against its
    /// `entry_token_amount` — so concurrent same-token positions never confirm
    /// against each other's sells (unlike the shared net `(wallet, mint)` balance).
    /// `None` when none of the signatures are indexed yet (empty `signatures` or an
    /// unknown wallet short-circuits). The integer SOL/token sums are converted back
    /// to f64 (SOL from lamports, tokens from raw units) for [`SigLegs`].
    pub async fn sum_legs_by_signatures(
        &self,
        wallet: &str,
        mint: &str,
        signatures: &[String],
        trade_type: TradeType,
    ) -> anyhow::Result<Option<SigLegs>> {
        if signatures.is_empty() {
            return Ok(None);
        }
        let Some(wallet_id) = WalletDictRepo::new(self.pool.clone()).id_for(wallet).await? else {
            return Ok(None);
        };
        // Translate the position's base58 signatures to raw bytes for the BYTEA filter.
        let sig_bytes: Vec<Vec<u8>> = signatures
            .iter()
            .map(|s| sig_base58_to_bytes(s))
            .collect::<anyhow::Result<_>>()?;

        let row: (i64, i64, i64, Option<DateTime<Utc>>, Option<DateTime<Utc>>) = sqlx::query_as(
            r#"
            SELECT COUNT(*)::bigint,
                   COALESCE(SUM(token_amount), 0)::bigint,
                   COALESCE(SUM(sol_amount), 0)::bigint,
                   MIN(block_time),
                   MAX(block_time)
            FROM trades
            WHERE wallet_id = $1
              AND mint_address = $2
              AND trade_type = $3
              AND tx_signature = ANY($4)
            "#,
        )
        .bind(wallet_id)
        .bind(mint)
        .bind(trade_type_str(trade_type))
        .bind(&sig_bytes)
        .fetch_one(&self.pool)
        .await?;

        let (leg_count, token_sum, lamports_sum, first, last) = row;
        if leg_count == 0 {
            return Ok(None);
        }
        Ok(Some(SigLegs {
            // token_amount stays an exact integer (raw units); SOL → human f64.
            token_amount: token_sum as u64,
            sol_amount: lamports_to_sol(lamports_sum),
            first_block_time: first.unwrap_or_else(Utc::now),
            last_block_time: last.unwrap_or_else(Utc::now),
        }))
    }

    /// Net token balance for `(wallet, mint)` (Σbuys − Σsells), as **signed raw
    /// integer units** (`i64` — a partially-cleared bag can be negative mid-state).
    /// No longer on the sell-confirm hot path (replaced by per-signature
    /// attribution); used for external-clear detection (ManualSell path) and ad-hoc
    /// balance lookups. An unknown wallet has no trades, so returns 0 without
    /// touching `trades`.
    pub async fn net_token_amount_by_wallet_and_mint(
        &self,
        wallet: &str,
        mint: &str,
    ) -> anyhow::Result<i64> {
        let Some(wallet_id) = WalletDictRepo::new(self.pool.clone()).id_for(wallet).await? else {
            return Ok(0);
        };
        // Sum the integer token_amount column with a buy/sell sign — exact raw units.
        let balance: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(CASE WHEN trade_type = 'buy' THEN token_amount WHEN trade_type = 'sell' THEN -token_amount ELSE 0 END), 0)::bigint FROM trades WHERE wallet_id = $1 AND mint_address = $2",
        )
        .bind(wallet_id)
        .bind(mint)
        .fetch_one(&self.pool)
        .await?;

        Ok(balance)
    }

    /// Stream the cache-seed trade history for the given mints, grouped per mint,
    /// invoking `f(mint, trades, agg)` once per mint as its run completes. A single
    /// scan does the work the old two-pass seed needed (full aggregate scan +
    /// full chronological stream):
    ///
    /// - **Capped** — only the newest `per_mint_cap` trades per mint land in
    ///   `trades` (a per-mint `ROW_NUMBER` window), so a high-volume token reads its
    ///   recent window instead of its full unbounded history.
    /// - **Single pass** — lifetime `count`/`volume` and the newest trade's
    ///   `block_time`/`price`/`reserves` ride along as window aggregates computed
    ///   over the *full* partition in the same scan (`SeedAgg`), so the caller never
    ///   needs a second aggregate query.
    ///
    /// `trades` arrives oldest-first per mint (ready for `push_trade_capped`). The
    /// seed path doesn't filter by wallet, so it simply JOINs `wallet_dict` for the
    /// address. Scoped to the seeded set (`mint = ANY($1)`, chunked) and grouped
    /// while streaming so peak memory is one mint's capped run.
    pub async fn for_each_seed_mint<F>(
        &self,
        mints: &[String],
        per_mint_cap: i64,
        mut f: F,
    ) -> anyhow::Result<()>
    where
        F: FnMut(String, Vec<Trade>, SeedAgg),
    {
        use futures_util::TryStreamExt;

        /// One seed row: the trade columns plus the per-mint (partition-constant)
        /// aggregates carried by the window functions. `lifetime_volume` is in
        /// lamports (SUM of the integer column) and converted to f64 SOL on read;
        /// `newest_price` is already a float (sol/token ratio computed in SQL);
        /// `newest_reserves` is the raw integer virtual_token_reserves as f64.
        #[derive(sqlx::FromRow)]
        struct SeedTradeRow {
            #[sqlx(flatten)]
            trade: TradeDbRow,
            lifetime_count: i64,
            lifetime_volume: i64,
            newest_block_time: DateTime<Utc>,
            newest_price: Option<f64>,
            newest_reserves: Option<f64>,
        }

        if mints.is_empty() {
            return Ok(());
        }
        for chunk in mints.chunks(SEED_MINT_CHUNK) {
            let mut stream = sqlx::query_as::<_, SeedTradeRow>(
                r#"
                WITH ranked AS (
                    SELECT t.mint_address, w.address AS wallet_address, t.trade_type, t.venue,
                           t.sol_amount, t.token_amount,
                           t.virtual_sol_reserves, t.virtual_token_reserves,
                           t.slot, t.tx_index, t.leg_index, t.block_time, t.tx_signature,
                           ROW_NUMBER()                          OVER w  AS rn,
                           COUNT(*)                              OVER wp AS lifetime_count,
                           COALESCE(SUM(t.sol_amount) OVER wp, 0)::bigint AS lifetime_volume,
                           FIRST_VALUE(t.block_time)             OVER w  AS newest_block_time,
                           FIRST_VALUE(t.sol_amount::float8 / NULLIF(t.token_amount, 0)) OVER w AS newest_price,
                           FIRST_VALUE(t.virtual_token_reserves::float8) OVER w AS newest_reserves
                    FROM trades t
                    JOIN wallet_dict w ON w.id = t.wallet_id
                    WHERE t.mint_address = ANY($1)
                    WINDOW
                        w  AS (PARTITION BY t.mint_address
                               ORDER BY t.slot DESC, t.tx_index DESC, t.leg_index DESC),
                        wp AS (PARTITION BY t.mint_address)
                )
                SELECT mint_address, wallet_address, trade_type, venue,
                       sol_amount, token_amount,
                       virtual_sol_reserves, virtual_token_reserves,
                       slot, tx_index, leg_index, block_time, tx_signature,
                       lifetime_count, lifetime_volume, newest_block_time, newest_price, newest_reserves
                FROM ranked
                WHERE rn <= $2
                ORDER BY mint_address ASC, slot ASC, tx_index ASC, leg_index ASC
                "#,
            )
            .bind(chunk)
            .bind(per_mint_cap)
            .fetch(&self.pool);

            // Rows are mint-contiguous (ORDER BY mint_address) and each mint lives
            // entirely within this chunk, so group on the mint boundary and flush.
            let mut cur_mint: Option<String> = None;
            let mut buf: Vec<Trade> = Vec::new();
            let mut agg: Option<SeedAgg> = None;
            while let Some(row) = stream.try_next().await? {
                if cur_mint.as_deref() != Some(row.trade.mint_address.as_str()) {
                    if let (Some(m), Some(a)) = (cur_mint.take(), agg.take()) {
                        f(m, std::mem::take(&mut buf), a);
                    }
                    cur_mint = Some(row.trade.mint_address.clone());
                    agg = Some(SeedAgg {
                        lifetime_count: row.lifetime_count.max(0) as u64,
                        // lifetime_volume is a lamports SUM → convert to f64 SOL.
                        lifetime_volume: lamports_to_sol(row.lifetime_volume),
                        last_trade_at: row.newest_block_time,
                        current_reserves: row.newest_reserves,
                        newest_price: row.newest_price,
                    });
                }
                buf.push(Trade::try_from(row.trade)?);
            }
            if let (Some(m), Some(a)) = (cur_mint.take(), agg.take()) {
                f(m, std::mem::take(&mut buf), a);
            }
        }
        Ok(())
    }
}

/// Look up the on-chain `tx_signature` for a real trade matching a paper fill.
/// Paper execution finds fills in the in-memory cache (which strips `tx_signature`
/// for Phase B), then calls this to recover the real signature so paper positions
/// store the same tx as sim positions and the frontend highlight works identically.
/// Returns `None` for time-driven exits where no real trade occurred.
///
/// Price is derived now (the table has no `price_per_token` column), so the match
/// computes `sol_amount::float8 / NULLIF(token_amount, 0)` and compares it to the
/// requested `price_per_token`. Best-effort lookup; the raw signature bytes are
/// decoded back to base58.
pub async fn find_tx_by_fill(
    pool: &PgPool,
    mint: &str,
    block_time: DateTime<Utc>,
    price_per_token: f64,
) -> sqlx::Result<Option<String>> {
    let bytes: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT tx_signature FROM trades \
         WHERE mint_address = $1 AND block_time = $2 \
           AND (sol_amount::float8 / NULLIF(token_amount, 0)) = $3 \
         LIMIT 1",
    )
    .bind(mint)
    .bind(block_time)
    .bind(price_per_token)
    .fetch_optional(pool)
    .await?;

    Ok(bytes.map(|b| sig_bytes_to_base58(&b)))
}

/// Rolled-up result of one or more trade legs sharing a `(wallet, mint, side)`,
/// summed by transaction signature ([`TradeRepo::find_fill_by_signature`] /
/// [`TradeRepo::sum_legs_by_signatures`]). For an entry the summary is the adopted
/// buy fill (single-leg today); for an exit it's the running total of the
/// position's own sell legs, compared against `entry_token_amount` to confirm the
/// clear.
#[derive(Debug, Clone)]
pub struct SigLegs {
    /// Σ token_amount across the legs — exact raw integer units.
    pub token_amount: u64,
    /// Σ sol_amount across the legs.
    pub sol_amount: f64,
    /// Earliest leg's block time (the fill's entry time).
    pub first_block_time: DateTime<Utc>,
    /// Latest leg's block time (the fill's exit time).
    pub last_block_time: DateTime<Utc>,
}

impl SigLegs {
    /// Weighted-average execution price (Σsol / Σtokens), or 0 when no tokens.
    pub fn price_per_token(&self) -> f64 {
        if self.token_amount > 0 {
            self.sol_amount / self.token_amount as f64
        } else {
            0.0
        }
    }
}

/// Per-mint, lifetime-scoped aggregates the cache seed needs alongside a mint's
/// (capped) recent trade run — computed in the same single scan as the trades
/// (see [`TradeRepo::for_each_seed_mint`]). `lifetime_count`/`lifetime_volume`
/// cover the *full* history; the `newest_*` fields are the most recent trade's.
pub struct SeedAgg {
    pub lifetime_count: u64,
    pub lifetime_volume: f64,
    pub last_trade_at: DateTime<Utc>,
    pub current_reserves: Option<f64>,
    pub newest_price: Option<f64>,
}

// ---------------------------------------------------------------------------
// Conversion helpers — the I/O boundary between the runtime `Trade` model and the
// integer/BYTEA `trades` schema.
//
// SOL: the model carries `sol_amount` / `virtual_sol_reserves` as human SOL (f64),
// so the SOL side round-trips through `sol_to_lamports`/`lamports_to_sol` (exact
// lamport precision in the BIGINT column). Token amounts and token reserves are
// now exact integers (`u64`) in the model too, so they bind/read as `i64` directly
// — no float helper, no precision loss above 2^53.
// ---------------------------------------------------------------------------

/// SOL (human-readable f64) → lamports (i64).
fn sol_to_lamports(sol: f64) -> i64 {
    (sol * 1_000_000_000.0).round() as i64
}

/// SOL reserve (human-readable f64) → lamports (i64); `.map(..)`-friendly.
fn sol_to_lamports_opt(sol: f64) -> i64 {
    sol_to_lamports(sol)
}

/// Lamports (i64) → SOL (human-readable f64).
fn lamports_to_sol(lamports: i64) -> f64 {
    lamports as f64 / 1_000_000_000.0
}

/// Derived execution price (`sol / token`), or 0 when no tokens.
fn price_of(sol: f64, token: f64) -> f64 {
    if token > 0.0 {
        sol / token
    } else {
        0.0
    }
}

/// Base58 signature string → raw 64-byte signature. Parse errors map into anyhow.
fn sig_base58_to_bytes(s: &str) -> anyhow::Result<Vec<u8>> {
    let sig = solana_sdk::signature::Signature::from_str(s)
        .map_err(|e| anyhow::anyhow!("invalid base58 signature {s:?}: {e}"))?;
    Ok(sig.as_ref().to_vec())
}

/// Raw signature bytes → base58 string. Best-effort: a malformed length yields an
/// empty string rather than erroring (read paths shouldn't fail on a stray row).
fn sig_bytes_to_base58(bytes: &[u8]) -> String {
    solana_sdk::signature::Signature::try_from(bytes)
        .map(|s| s.to_string())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Per-signature attribution primitives (`find_fill_by_signature` /
// `sum_legs_by_signatures`) — the heart of decision #2 (two positions on the
// same token must each confirm against their OWN fills, never the shared
// `(wallet, mint)` balance). DB-integration, so `#[ignore]`d like the other
// DB tests; run against a local Postgres:
//   $env:DATABASE_URL = "postgres://postgres:1220@localhost:5432/meme_bot"
//   cargo test -p trading_core trade_repo:: -- --ignored --nocapture
// Each test uses unique mint/wallet ids and deletes the rows it created.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    /// `insert_many` binds 13 params/row (mint_address, wallet_id, trade_type,
    /// venue, sol_amount, token_amount, virtual_sol_reserves, virtual_token_reserves,
    /// slot, tx_index, leg_index, block_time, tx_signature); one Postgres statement
    /// is capped at 65535 (sqlx 0.6 wraps `len() as i16` past it → a Postgres parse
    /// error). Pin the chunk so adding a bound column re-checks the ceiling here
    /// instead of surfacing as a runtime parse error on the backfill path.
    #[test]
    fn trade_insert_chunk_stays_under_param_ceiling() {
        const BINDS_PER_ROW: usize = 13;
        assert!(
            TRADE_INSERT_CHUNK * BINDS_PER_ROW <= 65_535,
            "TRADE_INSERT_CHUNK ({TRADE_INSERT_CHUNK}) × {BINDS_PER_ROW} binds exceeds the 65535 ceiling"
        );
    }

    /// `virtual_sol_reserves` is human SOL in the model but lamports in the
    /// BIGINT column — the SOL↔lamports round-trip must preserve fractional SOL
    /// (the old `f64_opt_to_raw` path rounded 30.5 SOL to the integer 31).
    #[test]
    fn virtual_sol_reserves_round_trips_through_lamports() {
        let sol = 30.123_456_789_f64;
        let stored = sol_to_lamports_opt(sol);
        assert_eq!(stored, 30_123_456_789, "SOL → lamports keeps 9-decimal precision");
        let back = lamports_to_sol(stored);
        assert!((back - sol).abs() < 1e-9, "lamports → SOL recovers the value");
    }

    async fn test_pool() -> Option<PgPool> {
        let url = std::env::var("DATABASE_URL").ok()?;
        PgPoolOptions::new().max_connections(2).connect(&url).await.ok()
    }

    fn unique(prefix: &str) -> String {
        format!("{prefix}{}", Uuid::new_v4().simple())
    }

    /// Insert one trade leg under `sig`/`leg_index` for `(wallet, mint, side)`.
    /// `(block_time, tx_signature, leg_index)` is the conflict target, so distinct
    /// legs of one tx differ only by `leg_index`. The model is unchanged, so this
    /// builds a `Trade` exactly as before; the repo handles the schema conversion.
    async fn insert_leg(
        repo: &TradeRepo,
        wallet: &str,
        mint: &str,
        side: TradeType,
        sig: &str,
        leg_index: u32,
        sol: f64,
        tokens: u64,
    ) {
        let mut trade = Trade::new(
            mint.to_string(),
            wallet.to_string(),
            side,
            sol,
            tokens,
            sig.to_string(),
            100,
            Utc::now(),
        );
        trade.leg_index = leg_index;
        repo.insert(&trade).await.expect("insert leg");
    }

    async fn cleanup(pool: &PgPool, mint: &str) {
        let _ = sqlx::query("DELETE FROM trades WHERE mint_address = $1")
            .bind(mint)
            .execute(pool)
            .await;
    }

    #[tokio::test]
    #[ignore = "requires a local Postgres (DATABASE_URL); run with --ignored"]
    async fn find_fill_by_signature_sums_multi_leg() {
        let Some(pool) = test_pool().await else { return };
        let repo = TradeRepo::new(pool.clone());
        let (wallet, mint, sig) = (unique("W"), unique("M"), unique("buysig-"));

        // One buy that landed as two legs (e.g. a split route) under one signature.
        insert_leg(&repo, &wallet, &mint, TradeType::Buy, &sig, 0, 0.6, 600).await;
        insert_leg(&repo, &wallet, &mint, TradeType::Buy, &sig, 1, 0.4, 400).await;
        // A foreign buy on the SAME (wallet, mint) under a different signature —
        // a concurrent same-token position's fill (decision #2). Must NOT leak in.
        insert_leg(&repo, &wallet, &mint, TradeType::Buy, &unique("foreign-"), 0, 9.9, 9999).await;

        let legs = repo
            .find_fill_by_signature(&wallet, &mint, &sig)
            .await
            .expect("query")
            .expect("the signature's legs are summed, not None");
        assert_eq!(legs.token_amount, 1000, "Σtokens across both legs");
        assert!((legs.sol_amount - 1.0).abs() < 1e-6, "Σsol across both legs");
        // Weighted-average price = Σsol / Σtokens, not a per-leg price.
        assert!((legs.price_per_token() - 0.001).abs() < 1e-9, "weighted-avg fill price");

        cleanup(&pool, &mint).await;
    }

    #[tokio::test]
    #[ignore = "requires a local Postgres (DATABASE_URL); run with --ignored"]
    async fn sum_legs_by_signatures_isolates_own_sells_and_short_circuits_empty() {
        let Some(pool) = test_pool().await else { return };
        let repo = TradeRepo::new(pool.clone());
        let (wallet, mint) = (unique("W"), unique("M"));
        let (mine_a, mine_b, theirs) = (unique("sellA-"), unique("sellB-"), unique("sellX-"));

        // This position's exit landed across two sell signatures…
        insert_leg(&repo, &wallet, &mint, TradeType::Sell, &mine_a, 0, 0.3, 300).await;
        insert_leg(&repo, &wallet, &mint, TradeType::Sell, &mine_b, 0, 0.2, 200).await;
        // …while a concurrent same-token position sold under its own signature.
        insert_leg(&repo, &wallet, &mint, TradeType::Sell, &theirs, 0, 5.0, 5000).await;

        // Empty signature set short-circuits to None (never a full-table scan).
        assert!(
            repo.sum_legs_by_signatures(&wallet, &mint, &[], TradeType::Sell)
                .await
                .expect("query")
                .is_none(),
            "empty signatures ⇒ None"
        );

        let legs = repo
            .sum_legs_by_signatures(
                &wallet,
                &mint,
                &[mine_a.clone(), mine_b.clone()],
                TradeType::Sell,
            )
            .await
            .expect("query")
            .expect("own sell legs summed");
        assert_eq!(legs.token_amount, 500, "only THIS position's sells summed");
        assert!((legs.sol_amount - 0.5).abs() < 1e-6, "concurrent position's sell excluded");

        // Side filter holds: no Buy legs exist, so a Buy query over the sell sigs is None.
        assert!(
            repo.sum_legs_by_signatures(&wallet, &mint, &[mine_a, mine_b], TradeType::Buy)
                .await
                .expect("query")
                .is_none(),
            "trade_type filter excludes the sell legs"
        );

        cleanup(&pool, &mint).await;
    }
}
