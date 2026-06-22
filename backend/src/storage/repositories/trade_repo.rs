use std::collections::HashSet;

use chrono::{DateTime, Utc};
use sqlx::{types::Json, PgPool};
use uuid::Uuid;

use crate::models::trade::{Trade, TradeType};

/// Mints per round-trip for the startup cache-seed scans. Bounds each `= ANY($1)`
/// array so Postgres keeps using the per-mint indexes instead of falling back to a
/// full-table seq scan on a huge array (mirrors `sweep::corpus::DbSource` chunking).
const SEED_MINT_CHUNK: usize = 1000;

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
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct TradeDbRow {
    id: Uuid,
    mint_address: String,
    wallet_address: String,
    trade_type: String,
    sol_amount: f64,
    token_amount: f64,
    price_per_token: f64,
    tx_signature: String,
    leg_index: i32,
    slot: i64,
    block_time: DateTime<Utc>,
    received_at: DateTime<Utc>,
    virtual_sol_reserves: Option<f64>,
    virtual_token_reserves: Option<f64>,
    real_sol_reserves: Option<f64>,
    real_token_reserves: Option<f64>,
    ix_type: String,
    ix_labels: Json<serde_json::Value>,
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

        Ok(Self {
            id: r.id,
            mint_address: r.mint_address,
            wallet_address: r.wallet_address,
            trade_type,
            sol_amount: r.sol_amount,
            token_amount: r.token_amount,
            price_per_token: r.price_per_token,
            tx_signature: r.tx_signature,
            leg_index: r.leg_index as u32,
            slot: r.slot as u64,
            block_time: r.block_time,
            received_at: r.received_at,
            virtual_sol_reserves: r.virtual_sol_reserves,
            virtual_token_reserves: r.virtual_token_reserves,
            real_sol_reserves: r.real_sol_reserves,
            real_token_reserves: r.real_token_reserves,
            instruction_type: r.ix_type,
            instruction_labels: r.ix_labels.0,
            venue: r.venue,
        })
    }
}

/// Slim projection for the bulk read paths (swing scan, token-sync seed,
/// backtests). Identical to [`TradeDbRow`] minus the `ix_labels` JSONB column:
/// none of those consumers read per-trade instruction labels (the live ingest
/// ring strips them to `Null` too — see `pipeline.rs`), so omitting the column
/// skips a per-row JSONB decode across whole-mint histories.
#[derive(sqlx::FromRow)]
pub(crate) struct TradeSlimRow {
    id: Uuid,
    mint_address: String,
    wallet_address: String,
    trade_type: String,
    sol_amount: f64,
    token_amount: f64,
    price_per_token: f64,
    tx_signature: String,
    leg_index: i32,
    slot: i64,
    block_time: DateTime<Utc>,
    received_at: DateTime<Utc>,
    virtual_sol_reserves: Option<f64>,
    virtual_token_reserves: Option<f64>,
    real_sol_reserves: Option<f64>,
    real_token_reserves: Option<f64>,
    ix_type: String,
    venue: String,
}

impl TryFrom<TradeSlimRow> for Trade {
    type Error = anyhow::Error;

    fn try_from(r: TradeSlimRow) -> Result<Self, Self::Error> {
        let trade_type = match r.trade_type.as_str() {
            "buy" => TradeType::Buy,
            "sell" => TradeType::Sell,
            other => anyhow::bail!("Unknown trade_type in DB: {other}"),
        };

        Ok(Self {
            id: r.id,
            mint_address: r.mint_address,
            wallet_address: r.wallet_address,
            trade_type,
            sol_amount: r.sol_amount,
            token_amount: r.token_amount,
            price_per_token: r.price_per_token,
            tx_signature: r.tx_signature,
            leg_index: r.leg_index as u32,
            slot: r.slot as u64,
            block_time: r.block_time,
            received_at: r.received_at,
            virtual_sol_reserves: r.virtual_sol_reserves,
            virtual_token_reserves: r.virtual_token_reserves,
            real_sol_reserves: r.real_sol_reserves,
            real_token_reserves: r.real_token_reserves,
            instruction_type: r.ix_type,
            // Not selected on the bulk paths; matches the ingest ring's stripped value.
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

// ---------------------------------------------------------------------------
// Repo
// ---------------------------------------------------------------------------

impl TradeRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Insert a trade. On replay, refresh the decoded price/reserve columns so
    /// decoder fixes (e.g. AMM pre- vs post-swap reserves) propagate on re-sync,
    /// while preserving identity/time columns (`id`, `received_at`).
    pub async fn insert(&self, trade: &Trade) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO trades
                (id, mint_address, wallet_address, trade_type,
                 sol_amount, token_amount, price_per_token,
                 tx_signature, leg_index, slot, block_time, received_at,
                 virtual_sol_reserves, virtual_token_reserves,
                 real_sol_reserves, real_token_reserves,
                 ix_type, ix_labels, venue)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19)
            ON CONFLICT (tx_signature, leg_index, block_time) DO UPDATE SET
                price_per_token        = EXCLUDED.price_per_token,
                virtual_sol_reserves   = EXCLUDED.virtual_sol_reserves,
                virtual_token_reserves = EXCLUDED.virtual_token_reserves,
                real_sol_reserves      = EXCLUDED.real_sol_reserves,
                real_token_reserves    = EXCLUDED.real_token_reserves
            "#,
        )
        .bind(trade.id)
        .bind(&trade.mint_address)
        .bind(&trade.wallet_address)
        .bind(trade_type_str(trade.trade_type))
        .bind(trade.sol_amount)
        .bind(trade.token_amount)
        .bind(trade.price_per_token)
        .bind(&trade.tx_signature)
        .bind(trade.leg_index as i32)
        .bind(trade.slot as i64)
        .bind(trade.block_time)
        .bind(trade.received_at)
        .bind(trade.virtual_sol_reserves)
        .bind(trade.virtual_token_reserves)
        .bind(trade.real_sol_reserves)
        .bind(trade.real_token_reserves)
        .bind(&trade.instruction_type)
        .bind(sqlx::types::Json(&trade.instruction_labels))
        .bind(&trade.venue)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Bulk version of [`insert`] — one multi-row statement for the whole slice,
    /// with the identical upsert. Callers MUST dedup by `(tx_signature,
    /// leg_index)` first: Postgres rejects an `ON CONFLICT DO UPDATE` that hits
    /// the same conflict target twice within one statement. (`block_time` is
    /// deterministic per tx, so it's part of the conflict target — see migration
    /// 0002 — but adds nothing to the dedup key.) Used by the live
    /// ingest DB-writer to collapse a flush of trades into a single round-trip.
    pub async fn insert_many(&self, trades: &[Trade]) -> anyhow::Result<()> {
        if trades.is_empty() {
            return Ok(());
        }
        let mut qb: sqlx::QueryBuilder<sqlx::Postgres> = sqlx::QueryBuilder::new(
            "INSERT INTO trades \
             (id, mint_address, wallet_address, trade_type, sol_amount, token_amount, \
              price_per_token, tx_signature, leg_index, slot, block_time, received_at, \
              virtual_sol_reserves, virtual_token_reserves, real_sol_reserves, \
              real_token_reserves, ix_type, ix_labels, venue) ",
        );
        qb.push_values(trades, |mut b, t| {
            b.push_bind(t.id)
                .push_bind(&t.mint_address)
                .push_bind(&t.wallet_address)
                .push_bind(trade_type_str(t.trade_type))
                .push_bind(t.sol_amount)
                .push_bind(t.token_amount)
                .push_bind(t.price_per_token)
                .push_bind(&t.tx_signature)
                .push_bind(t.leg_index as i32)
                .push_bind(t.slot as i64)
                .push_bind(t.block_time)
                .push_bind(t.received_at)
                .push_bind(t.virtual_sol_reserves)
                .push_bind(t.virtual_token_reserves)
                .push_bind(t.real_sol_reserves)
                .push_bind(t.real_token_reserves)
                .push_bind(&t.instruction_type)
                .push_bind(sqlx::types::Json(&t.instruction_labels))
                .push_bind(&t.venue);
        });
        qb.push(
            " ON CONFLICT (tx_signature, leg_index, block_time) DO UPDATE SET \
             price_per_token        = EXCLUDED.price_per_token, \
             virtual_sol_reserves   = EXCLUDED.virtual_sol_reserves, \
             virtual_token_reserves = EXCLUDED.virtual_token_reserves, \
             real_sol_reserves      = EXCLUDED.real_sol_reserves, \
             real_token_reserves    = EXCLUDED.real_token_reserves",
        );
        qb.build().execute(&self.pool).await?;

        Ok(())
    }

    /// Signature of the most recently saved trade for a token on a specific
    /// venue (`"curve"` or `"amm"`), if any. Used as the `until` boundary for
    /// incremental syncs so each venue resumes from its own last saved trade.
    pub async fn latest_signature(
        &self,
        mint: &str,
        venue: &str,
    ) -> anyhow::Result<Option<String>> {
        let sig: Option<String> = sqlx::query_scalar(
            r#"
            SELECT tx_signature
            FROM trades
            WHERE mint_address = $1 AND venue = $2
            ORDER BY slot DESC, block_time DESC
            LIMIT 1
            "#,
        )
        .bind(mint)
        .bind(venue)
        .fetch_optional(&self.pool)
        .await?;

        Ok(sig)
    }

    /// All transaction signatures already saved for a token on a venue
    /// (`"curve"` or `"amm"`). The incremental sync uses this to skip
    /// `getTransaction` for trades it already has (e.g. ones live ingest
    /// persisted ahead of the sync), so it doesn't re-spend Helius RPC credits
    /// re-downloading them. Returned as a set for O(1) membership tests.
    ///
    /// `candidates` is the list of signatures the sync is about to fetch; the
    /// query intersects against it (`tx_signature = ANY($3)`) so Postgres returns
    /// only the already-saved sigs among that page instead of streaming every
    /// saved signature for the mint into the process. An empty `candidates`
    /// short-circuits to an empty set (nothing to skip).
    pub async fn saved_signatures(
        &self,
        mint: &str,
        venue: &str,
        candidates: &[String],
    ) -> anyhow::Result<HashSet<String>> {
        if candidates.is_empty() {
            return Ok(HashSet::new());
        }
        // Bound the scan to the venue's sync watermark: an incremental sync only
        // ever lists signatures newer than that slot (`getSignaturesForAddress`
        // `until` the watermark sig), so a saved signature below it can never be
        // re-encountered and doesn't belong in the skip-set. COALESCE to 0 (all
        // rows, the old behaviour) before the first sync stamps a watermark. The
        // `= ANY($3)` further narrows the result to the caller's candidate page.
        let rows: Vec<(String,)> = sqlx::query_as(
            r#"
            SELECT DISTINCT t.tx_signature
            FROM trades t
            WHERE t.mint_address = $1
              AND t.venue = $2
              AND t.tx_signature = ANY($3)
              AND t.slot >= COALESCE(
                  (SELECT CASE WHEN $2 = 'amm'
                               THEN ti.last_synced_amm_slot
                               ELSE ti.last_synced_curve_slot END
                   FROM tokens_info ti
                   WHERE ti.mint_address = $1),
                  0)
            "#,
        )
        .bind(mint)
        .bind(venue)
        .bind(candidates)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|(sig,)| sig).collect())
    }

    /// Most-recent trade by `wallet` on `mint` of a given side, or `None`.
    /// Filters in SQL and fetches a single row instead of pulling N rows and
    /// scanning them in Rust. Backed by `idx_trades_wallet_mint`. Unlike a
    /// bounded `find_by_mint(.., N)` + `.find()`, this can't miss the wallet's
    /// fill behind N newer trades on a high-volume mint.
    ///
    /// No longer on the entry/exit-confirm path (1C replaced "latest buy/sell for
    /// the pair" with per-signature attribution — see [`Self::find_fill_by_signature`]
    /// / [`Self::sum_legs_by_signatures`]); retained for ad-hoc single-fill lookups.
    #[allow(dead_code)]
    pub async fn find_latest_by_wallet_mint_type(
        &self,
        wallet: &str,
        mint: &str,
        trade_type: TradeType,
    ) -> anyhow::Result<Option<Trade>> {
        let type_str = match trade_type {
            TradeType::Buy => "buy",
            TradeType::Sell => "sell",
        };
        let row = sqlx::query_as::<_, TradeDbRow>(
            r#"
            SELECT id, mint_address, wallet_address, trade_type,
                   sol_amount, token_amount, price_per_token,
                   tx_signature, leg_index, slot, block_time, received_at,
                   virtual_sol_reserves, virtual_token_reserves,
                   real_sol_reserves, real_token_reserves,
                   ix_type, ix_labels, venue
            FROM trades
            WHERE wallet_address = $1 AND mint_address = $2 AND trade_type = $3
            ORDER BY block_time DESC
            LIMIT 1
            "#,
        )
        .bind(wallet)
        .bind(mint)
        .bind(type_str)
        .fetch_optional(&self.pool)
        .await?;

        row.map(Trade::try_from).transpose()
    }

    /// Find all trades for a token in chronological order. Slim projection
    /// (no `ix_labels` JSONB): the seed/metrics/backtest consumers never read
    /// per-trade instruction labels.
    pub async fn find_by_mint_all(&self, mint: &str) -> anyhow::Result<Vec<Trade>> {
        let rows = sqlx::query_as::<_, TradeSlimRow>(
            r#"
            SELECT id, mint_address, wallet_address, trade_type,
                   sol_amount, token_amount, price_per_token,
                   tx_signature, leg_index, slot, block_time, received_at,
                   virtual_sol_reserves, virtual_token_reserves,
                   real_sol_reserves, real_token_reserves,
                   ix_type, venue
            FROM trades
            WHERE mint_address = $1
            ORDER BY slot ASC, block_time ASC, tx_signature ASC, leg_index ASC
            "#,
        )
        .bind(mint)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(Trade::try_from).collect()
    }

    /// Find all trades for a *batch* of tokens in one round-trip, grouped per
    /// mint and each group in the same chronological order as
    /// [`find_by_mint_all`]. The backtest uses this to fetch a chunk of candidate
    /// mints with a single query instead of one query per token: same total rows,
    /// but ~`mints.len()`× fewer round-trips and PgPool connections held — so a
    /// running simulation stops starving the live ingest pipeline's shared pool.
    ///
    /// Bounded by the caller's chunk size (never the full `trades` table). Mints
    /// with no trades are simply absent from the returned map.
    pub async fn find_by_mints_all(
        &self,
        mints: &[String],
    ) -> anyhow::Result<std::collections::HashMap<String, Vec<Trade>>> {
        // `mint_address` leads the ORDER BY so each mint's rows arrive as one
        // contiguous run already in chronological order; grouping is then a single
        // linear pass with no per-mint sort.
        let rows = sqlx::query_as::<_, TradeSlimRow>(
            r#"
            SELECT id, mint_address, wallet_address, trade_type,
                   sol_amount, token_amount, price_per_token,
                   tx_signature, leg_index, slot, block_time, received_at,
                   virtual_sol_reserves, virtual_token_reserves,
                   real_sol_reserves, real_token_reserves,
                   ix_type, venue
            FROM trades
            WHERE mint_address = ANY($1)
            ORDER BY mint_address ASC, slot ASC, block_time ASC, tx_signature ASC, leg_index ASC
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

    /// Find trades for a token in chronological order, bounded by `limit`/`offset`.
    /// Same ordering as `find_by_mint_all` but paged so a high-volume token can't
    /// produce an unbounded response (the API never has to materialise every row).
    /// Slim projection (no `ix_labels` JSONB): the swing scan ignores per-trade
    /// labels, and the trades-API DB fallback now matches its cache-served branch,
    /// which already returns `Null` labels (the ingest ring strips them).
    pub async fn find_by_mint_paged(
        &self,
        mint: &str,
        limit: i64,
        offset: i64,
    ) -> anyhow::Result<Vec<Trade>> {
        let rows = sqlx::query_as::<_, TradeSlimRow>(
            r#"
            SELECT id, mint_address, wallet_address, trade_type,
                   sol_amount, token_amount, price_per_token,
                   tx_signature, leg_index, slot, block_time, received_at,
                   virtual_sol_reserves, virtual_token_reserves,
                   real_sol_reserves, real_token_reserves,
                   ix_type, venue
            FROM trades
            WHERE mint_address = $1
            ORDER BY slot ASC, block_time ASC, tx_signature ASC, leg_index ASC
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
    /// rolled up into a [`SigLegs`] (Σtokens, Σsol, weighted price, first/last leg
    /// time). `None` when the signature has no matching trade indexed yet.
    ///
    /// This is the **per-signature entry attribution** primitive: the snipe buy
    /// already returns its own submitted signature, so the entry fill is recovered
    /// by *that* signature instead of `find_latest_by_wallet_mint_type` (the latest
    /// buy for the pair) — which, with two concurrent positions on the same token,
    /// would adopt the same fill twice. Backed by `idx_trades_wallet_mint_sig`.
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
    /// `None` when none of the signatures are indexed yet (empty `signatures`
    /// short-circuits). Backed by `idx_trades_wallet_mint_sig` (one index probe per
    /// signature via `= ANY`).
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
        let row: (i64, f64, f64, Option<DateTime<Utc>>, Option<DateTime<Utc>>) = sqlx::query_as(
            r#"
            SELECT COUNT(*)::bigint,
                   COALESCE(SUM(token_amount), 0.0),
                   COALESCE(SUM(sol_amount), 0.0),
                   MIN(block_time),
                   MAX(block_time)
            FROM trades
            WHERE wallet_address = $1
              AND mint_address = $2
              AND trade_type = $3
              AND tx_signature = ANY($4)
            "#,
        )
        .bind(wallet)
        .bind(mint)
        .bind(trade_type_str(trade_type))
        .bind(signatures)
        .fetch_one(&self.pool)
        .await?;

        let (leg_count, token_amount, sol_amount, first, last) = row;
        if leg_count == 0 {
            return Ok(None);
        }
        Ok(Some(SigLegs {
            token_amount,
            sol_amount,
            first_block_time: first.unwrap_or_else(Utc::now),
            last_block_time: last.unwrap_or_else(Utc::now),
        }))
    }

    /// Net token balance for `(wallet, mint)` (Σbuys − Σsells). No longer on the
    /// sell-confirm hot path (replaced by per-signature attribution); retained for
    /// the deferred SOL balance-floor / committed-SOL guards and ad-hoc balance
    /// lookups.
    #[allow(dead_code)]
    pub async fn net_token_amount_by_wallet_and_mint(
        &self,
        wallet: &str,
        mint: &str,
    ) -> anyhow::Result<f64> {
        let balance: f64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(CASE WHEN trade_type = 'buy' THEN token_amount WHEN trade_type = 'sell' THEN -token_amount ELSE 0 END), 0.0) FROM trades WHERE wallet_address = $1 AND mint_address = $2",
        )
        .bind(wallet)
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
    /// `trades` arrives oldest-first per mint (ready for `push_trade_capped`). Scoped
    /// to the seeded set (`mint = ANY($1)`, chunked) so the scan rides the per-mint
    /// index, and grouped while streaming so peak memory is one mint's capped run.
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
        /// aggregates carried by the window functions.
        #[derive(sqlx::FromRow)]
        struct SeedTradeRow {
            #[sqlx(flatten)]
            trade: TradeDbRow,
            lifetime_count: i64,
            lifetime_volume: f64,
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
                    SELECT id, mint_address, wallet_address, trade_type,
                           sol_amount, token_amount, price_per_token,
                           tx_signature, leg_index, slot, block_time, received_at,
                           virtual_sol_reserves, virtual_token_reserves,
                           real_sol_reserves, real_token_reserves,
                           ix_type, ix_labels, venue,
                           ROW_NUMBER()                       OVER w  AS rn,
                           COUNT(*)                           OVER wp AS lifetime_count,
                           COALESCE(SUM(sol_amount) OVER wp, 0.0)     AS lifetime_volume,
                           FIRST_VALUE(block_time)            OVER w  AS newest_block_time,
                           FIRST_VALUE(price_per_token)       OVER w  AS newest_price,
                           FIRST_VALUE(virtual_token_reserves) OVER w AS newest_reserves
                    FROM trades
                    WHERE mint_address = ANY($1)
                    WINDOW
                        w  AS (PARTITION BY mint_address
                               ORDER BY slot DESC, block_time DESC, tx_signature DESC, leg_index DESC),
                        wp AS (PARTITION BY mint_address)
                )
                SELECT id, mint_address, wallet_address, trade_type,
                       sol_amount, token_amount, price_per_token,
                       tx_signature, leg_index, slot, block_time, received_at,
                       virtual_sol_reserves, virtual_token_reserves,
                       real_sol_reserves, real_token_reserves,
                       ix_type, ix_labels, venue,
                       lifetime_count, lifetime_volume, newest_block_time, newest_price, newest_reserves
                FROM ranked
                WHERE rn <= $2
                ORDER BY mint_address ASC, slot ASC, block_time ASC, tx_signature ASC, leg_index ASC
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
                        lifetime_volume: row.lifetime_volume,
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
pub(crate) async fn find_tx_by_fill(
    pool: &PgPool,
    mint: &str,
    block_time: DateTime<Utc>,
    price_per_token: f64,
) -> sqlx::Result<Option<String>> {
    sqlx::query_scalar(
        "SELECT tx_signature FROM trades \
         WHERE mint_address = $1 AND block_time = $2 AND price_per_token = $3 \
         LIMIT 1",
    )
    .bind(mint)
    .bind(block_time)
    .bind(price_per_token)
    .fetch_optional(pool)
    .await
}

/// Rolled-up result of one or more trade legs sharing a `(wallet, mint, side)`,
/// summed by transaction signature ([`TradeRepo::find_fill_by_signature`] /
/// [`TradeRepo::sum_legs_by_signatures`]). For an entry the summary is the adopted
/// buy fill (single-leg today); for an exit it's the running total of the
/// position's own sell legs, compared against `entry_token_amount` to confirm the
/// clear.
#[derive(Debug, Clone)]
pub struct SigLegs {
    /// Σ token_amount across the legs.
    pub token_amount: f64,
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
        if self.token_amount > 0.0 {
            self.sol_amount / self.token_amount
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
// Per-signature attribution primitives (`find_fill_by_signature` /
// `sum_legs_by_signatures`) — the heart of decision #2 (two positions on the
// same token must each confirm against their OWN fills, never the shared
// `(wallet, mint)` balance). DB-integration, so `#[ignore]`d like the other
// DB tests; run against a local Postgres:
//   $env:DATABASE_URL = "postgres://postgres:1220@localhost:5432/meme_bot"
//   cargo test --bin backend trade_repo:: -- --ignored --nocapture
// Each test uses unique mint/wallet ids and deletes the rows it created.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    async fn test_pool() -> Option<PgPool> {
        let url = std::env::var("DATABASE_URL").ok()?;
        PgPoolOptions::new().max_connections(2).connect(&url).await.ok()
    }

    fn unique(prefix: &str) -> String {
        format!("{prefix}{}", Uuid::new_v4().simple())
    }

    /// Insert one trade leg under `sig`/`leg_index` for `(wallet, mint, side)`.
    /// `(tx_signature, leg_index, block_time)` is the conflict target, so distinct
    /// legs of one tx differ only by `leg_index`.
    async fn insert_leg(
        repo: &TradeRepo,
        wallet: &str,
        mint: &str,
        side: TradeType,
        sig: &str,
        leg_index: u32,
        sol: f64,
        tokens: f64,
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
        insert_leg(&repo, &wallet, &mint, TradeType::Buy, &sig, 0, 0.6, 600.0).await;
        insert_leg(&repo, &wallet, &mint, TradeType::Buy, &sig, 1, 0.4, 400.0).await;
        // A foreign buy on the SAME (wallet, mint) under a different signature —
        // a concurrent same-token position's fill (decision #2). Must NOT leak in.
        insert_leg(&repo, &wallet, &mint, TradeType::Buy, &unique("foreign-"), 0, 9.9, 9999.0).await;

        let legs = repo
            .find_fill_by_signature(&wallet, &mint, &sig)
            .await
            .expect("query")
            .expect("the signature's legs are summed, not None");
        assert!((legs.token_amount - 1000.0).abs() < 1e-6, "Σtokens across both legs");
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
        insert_leg(&repo, &wallet, &mint, TradeType::Sell, &mine_a, 0, 0.3, 300.0).await;
        insert_leg(&repo, &wallet, &mint, TradeType::Sell, &mine_b, 0, 0.2, 200.0).await;
        // …while a concurrent same-token position sold under its own signature.
        insert_leg(&repo, &wallet, &mint, TradeType::Sell, &theirs, 0, 5.0, 5000.0).await;

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
        assert!((legs.token_amount - 500.0).abs() < 1e-6, "only THIS position's sells summed");
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
