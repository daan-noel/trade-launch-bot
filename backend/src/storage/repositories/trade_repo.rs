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
    /// scanning them in Rust — used by the buy/sell confirmation hot loops, which
    /// only ever want this one fill. Backed by `idx_trades_wallet_mint`. Unlike a
    /// bounded `find_by_mint(.., N)` + `.find()`, this can't miss the wallet's
    /// fill behind N newer trades on a high-volume mint.
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

    /// Count trades by a specific wallet on a specific token.
    #[allow(dead_code)]
    pub async fn count_by_wallet_and_mint(&self, wallet: &str, mint: &str) -> anyhow::Result<i64> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(1) FROM trades WHERE wallet_address = $1 AND mint_address = $2",
        )
        .bind(wallet)
        .bind(mint)
        .fetch_one(&self.pool)
        .await?;

        Ok(count)
    }

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

    /// Peak and most-recent `real_sol_reserves` (SOL) for a mint across its whole
    /// trade history (curve + post-migration AMM). Returns `None` when no trade
    /// carries a reserve snapshot. A latest value far below the peak means the
    /// SOL backing was drained — the spoof-proof signature of a rug, since wash
    /// trades between many wallets net ~zero SOL and cannot inflate real reserves.
    pub async fn real_sol_reserve_extremes(
        &self,
        mint: &str,
    ) -> anyhow::Result<Option<(f64, f64)>> {
        #[derive(sqlx::FromRow)]
        struct Row {
            peak: Option<f64>,
            latest: Option<f64>,
        }
        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT
              (SELECT MAX(real_sol_reserves) FROM trades
                 WHERE mint_address = $1 AND real_sol_reserves IS NOT NULL) AS peak,
              (SELECT real_sol_reserves FROM trades
                 WHERE mint_address = $1 AND real_sol_reserves IS NOT NULL
                 ORDER BY slot DESC, block_time DESC, leg_index DESC
                 LIMIT 1) AS latest
            "#,
        )
        .bind(mint)
        .fetch_one(&self.pool)
        .await?;

        Ok(match (row.peak, row.latest) {
            (Some(peak), Some(latest)) => Some((peak, latest)),
            _ => None,
        })
    }

    /// Token flow for the *early-buyer cohort* — every wallet that bought within
    /// `slot_window` slots of the token's first trade (launch snipers / bundlers).
    /// Returns `(cohort_bought, cohort_net, total_bought)` in token units:
    /// `cohort_bought` is everything the cohort ever bought, `cohort_net` is its
    /// buys minus sells, and `total_bought` is the mint's whole buy volume (to
    /// judge whether the cohort actually controlled the launch). A dominant
    /// cohort whose net has collapsed to ~zero is the multi-wallet rug signature
    /// that a single-creator check misses.
    pub async fn early_buyer_cohort_net(
        &self,
        mint: &str,
        slot_window: i64,
    ) -> anyhow::Result<(f64, f64, f64)> {
        #[derive(sqlx::FromRow)]
        struct Row {
            cohort_bought: f64,
            cohort_net: f64,
            total_bought: f64,
        }
        // One materialized scan of the mint's trades feeds the first-slot, cohort,
        // and all three aggregates — instead of the previous four separate
        // `WHERE mint_address = $1` scans of the trades table.
        let row = sqlx::query_as::<_, Row>(
            r#"
            WITH mint_trades AS MATERIALIZED (
                SELECT wallet_address, trade_type, token_amount, slot
                FROM trades
                WHERE mint_address = $1
            ),
            first_slot AS (
                SELECT MIN(slot) AS s0 FROM mint_trades
            ),
            cohort AS (
                SELECT DISTINCT mt.wallet_address
                FROM mint_trades mt, first_slot
                WHERE mt.trade_type = 'buy'
                  AND mt.slot <= first_slot.s0 + $2
            )
            SELECT
              COALESCE(SUM(CASE
                WHEN c.wallet_address IS NOT NULL
                     AND mt.trade_type = 'buy' THEN mt.token_amount
                ELSE 0 END), 0.0) AS cohort_bought,
              COALESCE(SUM(CASE
                WHEN c.wallet_address IS NOT NULL THEN
                     CASE WHEN mt.trade_type = 'buy' THEN mt.token_amount
                          WHEN mt.trade_type = 'sell' THEN -mt.token_amount
                          ELSE 0 END
                ELSE 0 END), 0.0) AS cohort_net,
              COALESCE(SUM(CASE
                WHEN mt.trade_type = 'buy' THEN mt.token_amount
                ELSE 0 END), 0.0) AS total_bought
            FROM mint_trades mt
            LEFT JOIN cohort c ON c.wallet_address = mt.wallet_address
            "#,
        )
        .bind(mint)
        .bind(slot_window)
        .fetch_one(&self.pool)
        .await?;

        Ok((row.cohort_bought, row.cohort_net, row.total_bought))
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
