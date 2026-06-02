use chrono::{DateTime, Utc};
use sqlx::{types::Json, PgPool};
use uuid::Uuid;

use crate::config::constants::INITIAL_VIRTUAL_TOKEN_RESERVES;
use crate::models::trade::{Trade, TradeType};

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
    virtual_sol_reserves: Option<f64>,
    virtual_token_reserves: Option<f64>,
    real_sol_reserves: Option<f64>,
    real_token_reserves: Option<f64>,
    ix_type: String,
    ix_labels: Json<serde_json::Value>,
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
            virtual_sol_reserves: r.virtual_sol_reserves,
            virtual_token_reserves: r.virtual_token_reserves,
            real_sol_reserves: r.real_sol_reserves,
            real_token_reserves: r.real_token_reserves,
            instruction_type: r.ix_type,
            instruction_labels: r.ix_labels.0,
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

    /// Insert a trade. Ignores duplicates (idempotent on replay).
    pub async fn insert(&self, trade: &Trade) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO trades
                (id, mint_address, wallet_address, trade_type,
                 sol_amount, token_amount, price_per_token,
                 tx_signature, leg_index, slot, block_time,
                 virtual_sol_reserves, virtual_token_reserves,
                 real_sol_reserves, real_token_reserves,
                 ix_type, ix_labels)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)
            ON CONFLICT (tx_signature, leg_index) DO NOTHING
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
        .bind(trade.virtual_sol_reserves)
        .bind(trade.virtual_token_reserves)
        .bind(trade.real_sol_reserves)
        .bind(trade.real_token_reserves)
        .bind(&trade.instruction_type)
        .bind(sqlx::types::Json(&trade.instruction_labels))
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Most recent trades for a token, newest first.
    pub async fn find_by_mint(&self, mint: &str, limit: i64) -> anyhow::Result<Vec<Trade>> {
        let rows = sqlx::query_as::<_, TradeDbRow>(
            r#"
            SELECT id, mint_address, wallet_address, trade_type,
                   sol_amount, token_amount, price_per_token,
                   tx_signature, leg_index, slot, block_time,
                   virtual_sol_reserves, virtual_token_reserves,
                   real_sol_reserves, real_token_reserves,
                   ix_type, ix_labels
            FROM trades
            WHERE mint_address = $1
            ORDER BY block_time DESC
            LIMIT $2
            "#,
        )
        .bind(mint)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(Trade::try_from).collect()
    }

    /// Find all trades for a token in chronological order.
    pub async fn find_by_mint_all(&self, mint: &str) -> anyhow::Result<Vec<Trade>> {
        let rows = sqlx::query_as::<_, TradeDbRow>(
            r#"
            SELECT id, mint_address, wallet_address, trade_type,
                   sol_amount, token_amount, price_per_token,
                   tx_signature, leg_index, slot, block_time,
                   virtual_sol_reserves, virtual_token_reserves,
                   real_sol_reserves, real_token_reserves,
                   ix_type, ix_labels
            FROM trades
            WHERE mint_address = $1
            ORDER BY slot ASC, block_time ASC
            "#,
        )
        .bind(mint)
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

    /// Aggregate stats (count, volume, last_trade_at) for every token in one query.
    /// Returns rows as (mint_address, trade_count, volume_sol_total, last_trade_at, market_cap, current_virtual_token_reserves).
    pub async fn load_all_aggregates(
        &self,
    ) -> anyhow::Result<
        Vec<(
            String,
            u64,
            f64,
            Option<DateTime<Utc>>,
            Option<f64>,
            Option<f64>,
        )>,
    > {
        #[derive(sqlx::FromRow)]
        struct AggRow {
            mint_address: String,
            trade_count: i64,
            volume_sol_total: f64,
            last_trade_at: Option<DateTime<Utc>>,
            last_price: Option<f64>,
            current_virtual_token_reserves: Option<f64>,
        }
        let rows = sqlx::query_as::<_, AggRow>(
            r#"
            WITH agg AS (
                SELECT
                    mint_address,
                    COUNT(*) AS trade_count,
                    COALESCE(SUM(sol_amount), 0.0) AS volume_sol_total,
                    MAX(block_time) AS last_trade_at
                FROM trades
                GROUP BY mint_address
            ),
            last_trade AS (
                SELECT DISTINCT ON (mint_address)
                    mint_address,
                    price_per_token AS last_price,
                    virtual_token_reserves AS current_virtual_token_reserves
                FROM trades
                ORDER BY mint_address, block_time DESC
            )
            SELECT
                a.mint_address,
                a.trade_count,
                a.volume_sol_total,
                a.last_trade_at,
                l.last_price,
                l.current_virtual_token_reserves
            FROM agg a
            LEFT JOIN last_trade l ON l.mint_address = a.mint_address
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| {
                // Use the static initial virtual token reserves as baseline.
                let initial_reserves = INITIAL_VIRTUAL_TOKEN_RESERVES;

                let market_cap = match (r.current_virtual_token_reserves, r.last_price) {
                    (Some(current), Some(price)) => {
                        let circulating_supply = (initial_reserves - current).max(0.0);
                        Some(circulating_supply * price)
                    }
                    _ => None,
                };

                (
                    r.mint_address,
                    r.trade_count as u64,
                    r.volume_sol_total,
                    r.last_trade_at,
                    market_cap,
                    r.current_virtual_token_reserves,
                )
            })
            .collect())
    }

    /// Load all trades for every token in one query, oldest-first per mint.
    pub async fn load_all_chronological(&self) -> anyhow::Result<Vec<Trade>> {
        let rows = sqlx::query_as::<_, TradeDbRow>(
            r#"
            SELECT id, mint_address, wallet_address, trade_type,
                   sol_amount, token_amount, price_per_token,
                   tx_signature, leg_index, slot, block_time,
                   virtual_sol_reserves, virtual_token_reserves,
                   real_sol_reserves, real_token_reserves,
                   ix_type, ix_labels
            FROM trades
            ORDER BY mint_address, block_time ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(Trade::try_from).collect()
    }
}
