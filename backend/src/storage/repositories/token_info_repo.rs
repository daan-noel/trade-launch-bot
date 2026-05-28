use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::token_info::TokenInfo;

pub struct TokenInfoRepo {
    pool: PgPool,
}

impl TokenInfoRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Upsert token metrics.
    /// Keeps `ath_price` as the highest trade price ever seen for this token.
    pub async fn upsert_metrics(
        &self,
        mint: &str,
        ath_price: Option<f64>,
        ath_timestamp: Option<DateTime<Utc>>,
        age: Option<i64>,
        volume: f64,
        market_cap: Option<f64>,
        trade_count: i64,
        last_trade_at: Option<DateTime<Utc>>,
        current_price: Option<f64>,
        is_rugged: bool,
        is_migrated: bool,
    ) -> anyhow::Result<()> {
        let now = Utc::now();

        sqlx::query(
            r#"
            INSERT INTO tokens_info
                (mint_address, ath_price, ath_timestamp, age, volume, market_cap, trade_count, last_trade_at, current_price, is_rugged, is_migrated, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
            ON CONFLICT (mint_address) DO UPDATE
                SET ath_price = GREATEST(COALESCE(tokens_info.ath_price, 0.0), COALESCE(EXCLUDED.ath_price, 0.0)),
                    ath_timestamp = CASE WHEN COALESCE(EXCLUDED.ath_price, 0.0) > COALESCE(tokens_info.ath_price, 0.0)
                                    THEN EXCLUDED.ath_timestamp ELSE tokens_info.ath_timestamp END,
                    age = EXCLUDED.age,
                    volume = EXCLUDED.volume,
                    market_cap = EXCLUDED.market_cap,
                    trade_count = EXCLUDED.trade_count,
                    last_trade_at = CASE
                        WHEN EXCLUDED.last_trade_at IS NULL THEN tokens_info.last_trade_at
                        WHEN tokens_info.last_trade_at IS NULL THEN EXCLUDED.last_trade_at
                        WHEN EXCLUDED.last_trade_at > tokens_info.last_trade_at THEN EXCLUDED.last_trade_at
                        ELSE tokens_info.last_trade_at
                    END,
                    current_price = EXCLUDED.current_price,
                    is_rugged = EXCLUDED.is_rugged,
                    is_migrated = tokens_info.is_migrated OR EXCLUDED.is_migrated,
                    updated_at = EXCLUDED.updated_at
            "#,
        )
        .bind(mint)
        .bind(ath_price)
        .bind(ath_timestamp)
        .bind(age)
        .bind(volume)
        .bind(market_cap)
        .bind(trade_count)
        .bind(last_trade_at)
        .bind(current_price)
        .bind(is_rugged)
        .bind(is_migrated)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn update_migration_status(
        &self,
        mint: &str,
        is_migrated: bool,
    ) -> anyhow::Result<()> {
        let now = Utc::now();
        sqlx::query(
            r#"
            INSERT INTO tokens_info (mint_address, is_migrated, created_at, updated_at)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (mint_address) DO UPDATE
                SET is_migrated = tokens_info.is_migrated OR EXCLUDED.is_migrated,
                    updated_at = EXCLUDED.updated_at
            "#,
        )
        .bind(mint)
        .bind(is_migrated)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    #[allow(dead_code)]
    pub async fn find_by_mint(&self, mint: &str) -> anyhow::Result<Option<TokenInfo>> {
        let row = sqlx::query_as::<_, (Uuid, String, Option<f64>, Option<DateTime<Utc>>, Option<i64>, f64, Option<f64>, i64, Option<DateTime<Utc>>, Option<f64>, bool, bool, DateTime<Utc>, DateTime<Utc>)>(
            "SELECT id, mint_address, ath_price, ath_timestamp, age, volume, market_cap, trade_count, last_trade_at, current_price, is_rugged, is_migrated, created_at, updated_at FROM tokens_info WHERE mint_address = $1",
        )
        .bind(mint)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(
            |(
                id,
                mint_address,
                ath_price,
                ath_timestamp,
                age,
                volume,
                market_cap,
                trade_count,
                last_trade_at,
                current_price,
                is_rugged,
                is_migrated,
                created_at,
                updated_at,
            )| TokenInfo {
                id,
                mint_address,
                ath_price,
                ath_timestamp,
                age,
                volume,
                market_cap,
                trade_count,
                last_trade_at,
                current_price,
                is_rugged,
                is_migrated,
                created_at,
                updated_at,
            },
        ))
    }

    /// List all token metrics rows.
    pub async fn list_all(&self) -> anyhow::Result<Vec<TokenInfo>> {
        let rows = sqlx::query_as::<_, (Uuid, String, Option<f64>, Option<DateTime<Utc>>, Option<i64>, f64, Option<f64>, i64, Option<DateTime<Utc>>, Option<f64>, bool, bool, DateTime<Utc>, DateTime<Utc>)>(
            "SELECT id, mint_address, ath_price, ath_timestamp, age, volume, market_cap, trade_count, last_trade_at, current_price, is_rugged, is_migrated, created_at, updated_at FROM tokens_info",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(
                |(
                    id,
                    mint_address,
                    ath_price,
                    ath_timestamp,
                    age,
                    volume,
                    market_cap,
                    trade_count,
                    last_trade_at,
                    current_price,
                    is_rugged,
                    is_migrated,
                    created_at,
                    updated_at,
                )| TokenInfo {
                    id,
                    mint_address,
                    ath_price,
                    ath_timestamp,
                    age,
                    volume,
                    market_cap,
                    trade_count,
                    last_trade_at,
                    current_price,
                    is_rugged,
                    is_migrated,
                    created_at,
                    updated_at,
                },
            )
            .collect())
    }
}
