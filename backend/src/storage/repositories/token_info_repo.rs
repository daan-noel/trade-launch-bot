use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::token_info::TokenInfo;

#[derive(Clone)]
pub struct TokenInfoRepo {
    pool: PgPool,
}

impl TokenInfoRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Upsert token metrics.
    ///
    /// `ath_price` is written authoritatively from the caller's freshly
    /// recomputed value: the sync path rebuilds state from the full trade
    /// history, so a re-sync must be able to *lower* a previously over-stated
    /// ATH (e.g. one poisoned by a bad trade). A NULL incoming value preserves
    /// the existing stored value. The live ingest path always supplies a
    /// running max seeded from this column on startup, so it never lowers it.
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
        is_dead: bool,
        is_migrated: bool,
        lifetime_secs: Option<i64>,
    ) -> anyhow::Result<()> {
        let now = Utc::now();

        sqlx::query(
            r#"
            INSERT INTO tokens_info
                (mint_address, ath_price, ath_timestamp, age, volume, market_cap, trade_count, last_trade_at, current_price, is_dead, is_migrated, lifetime_secs, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
            ON CONFLICT (mint_address) DO UPDATE
                SET ath_price = COALESCE(EXCLUDED.ath_price, tokens_info.ath_price),
                    ath_timestamp = CASE WHEN EXCLUDED.ath_price IS NOT NULL
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
                    is_dead = EXCLUDED.is_dead,
                    is_migrated = tokens_info.is_migrated OR EXCLUDED.is_migrated,
                    lifetime_secs = COALESCE(EXCLUDED.lifetime_secs, tokens_info.lifetime_secs),
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
        .bind(is_dead)
        .bind(is_migrated)
        .bind(lifetime_secs)
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
        let row = sqlx::query_as::<_, (Uuid, String, Option<f64>, Option<DateTime<Utc>>, Option<i64>, f64, Option<f64>, i64, Option<DateTime<Utc>>, Option<f64>, bool, bool, DateTime<Utc>, DateTime<Utc>, Option<DateTime<Utc>>)>(
            "SELECT id, mint_address, ath_price, ath_timestamp, age, volume, market_cap, trade_count, last_trade_at, current_price, is_dead, is_migrated, created_at, updated_at, last_synced_at FROM tokens_info WHERE mint_address = $1",
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
                is_dead,
                is_migrated,
                created_at,
                updated_at,
                last_synced_at,
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
                is_dead,
                is_migrated,
                created_at,
                updated_at,
                last_synced_at,
            },
        ))
    }

    /// Token metrics rows for the given mints, batched in chunks of `INFO_CHUNK`.
    /// Scoped to the seeded set (`mint = ANY($1)`) so cold start never `SELECT`s the
    /// whole, retention-free `tokens_info` table — it grows forever and the cache
    /// only keeps the recent `SEED_TOKEN_LIMIT` mints. Mints with no row are absent.
    pub async fn find_for(&self, mints: &[String]) -> anyhow::Result<Vec<TokenInfo>> {
        /// Mints per round-trip; keeps each `= ANY($1)` array index-friendly.
        const INFO_CHUNK: usize = 1000;
        if mints.is_empty() {
            return Ok(Vec::new());
        }
        let mut rows = Vec::with_capacity(mints.len());
        for chunk in mints.chunks(INFO_CHUNK) {
            let batch = sqlx::query_as::<_, (Uuid, String, Option<f64>, Option<DateTime<Utc>>, Option<i64>, f64, Option<f64>, i64, Option<DateTime<Utc>>, Option<f64>, bool, bool, DateTime<Utc>, DateTime<Utc>, Option<DateTime<Utc>>)>(
                "SELECT id, mint_address, ath_price, ath_timestamp, age, volume, market_cap, trade_count, last_trade_at, current_price, is_dead, is_migrated, created_at, updated_at, last_synced_at FROM tokens_info WHERE mint_address = ANY($1)",
            )
            .bind(chunk)
            .fetch_all(&self.pool)
            .await?;
            rows.extend(batch);
        }

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
                    is_dead,
                    is_migrated,
                    created_at,
                    updated_at,
                    last_synced_at,
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
                    is_dead,
                    is_migrated,
                    created_at,
                    updated_at,
                    last_synced_at,
                },
            )
            .collect())
    }

    /// Read the per-token sync watermark:
    /// `(last_synced_at, curve_sig, amm_sig, curve_slot, amm_slot)`.
    /// Any field is `None` if the token has never been synced (or predates that
    /// field). Returns all-`None` when the row doesn't exist yet. The `*_slot`
    /// fields are the slots of the matching `*_sig` watermarks, used as the
    /// `from_slot` boundary for the LaserStream replay fast path.
    pub async fn get_sync_watermark(
        &self,
        mint: &str,
    ) -> anyhow::Result<(
        Option<DateTime<Utc>>,
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<i64>,
    )> {
        let row = sqlx::query_as::<
            _,
            (
                Option<DateTime<Utc>>,
                Option<String>,
                Option<String>,
                Option<i64>,
                Option<i64>,
            ),
        >(
            "SELECT last_synced_at, last_synced_curve_sig, last_synced_amm_sig, \
                    last_synced_curve_slot, last_synced_amm_slot \
             FROM tokens_info WHERE mint_address = $1",
        )
        .bind(mint)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.unwrap_or((None, None, None, None, None)))
    }

    /// Record a successful sync: stamp `last_synced_at = at` and store the newest
    /// per-venue signatures (and their slots) seen. A `None` signature/slot
    /// preserves the prior value (e.g. an AMM-less sync leaves any existing AMM
    /// watermark intact).
    pub async fn update_sync_watermark(
        &self,
        mint: &str,
        at: DateTime<Utc>,
        curve_sig: Option<&str>,
        amm_sig: Option<&str>,
        curve_slot: Option<i64>,
        amm_slot: Option<i64>,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO tokens_info
                (mint_address, last_synced_at, last_synced_curve_sig, last_synced_amm_sig,
                 last_synced_curve_slot, last_synced_amm_slot, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $2, $2)
            ON CONFLICT (mint_address) DO UPDATE
                SET last_synced_at = EXCLUDED.last_synced_at,
                    last_synced_curve_sig = COALESCE(EXCLUDED.last_synced_curve_sig, tokens_info.last_synced_curve_sig),
                    last_synced_amm_sig = COALESCE(EXCLUDED.last_synced_amm_sig, tokens_info.last_synced_amm_sig),
                    last_synced_curve_slot = COALESCE(EXCLUDED.last_synced_curve_slot, tokens_info.last_synced_curve_slot),
                    last_synced_amm_slot = COALESCE(EXCLUDED.last_synced_amm_slot, tokens_info.last_synced_amm_slot),
                    updated_at = EXCLUDED.updated_at
            "#,
        )
        .bind(mint)
        .bind(at)
        .bind(curve_sig)
        .bind(amm_sig)
        .bind(curve_slot)
        .bind(amm_slot)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}
