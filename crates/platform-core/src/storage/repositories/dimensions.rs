//! Domain A dimension repos: quote assets, launchpads, markets.

use chrono::Utc;
use sqlx::PgPool;

use crate::models::{Launchpad, Market, QuoteAsset};

/// `quote_assets` — the interned quote dimension (SOL, USDC, …).
pub struct QuoteAssetRepo;

impl QuoteAssetRepo {
    pub async fn all(pool: &PgPool) -> anyhow::Result<Vec<QuoteAsset>> {
        Ok(sqlx::query_as::<_, QuoteAsset>("SELECT * FROM quote_assets ORDER BY id")
            .fetch_all(pool)
            .await?)
    }

    pub async fn get(pool: &PgPool, id: i16) -> anyhow::Result<Option<QuoteAsset>> {
        Ok(sqlx::query_as::<_, QuoteAsset>("SELECT * FROM quote_assets WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?)
    }

    /// Poller entry point: set the USD numeraire for a quote (USDC pinned ≈ 1,
    /// SOL updated from the price feed). Stamps `usd_rate_at = now()`.
    pub async fn set_usd_rate(pool: &PgPool, id: i16, usd_rate: f64) -> anyhow::Result<()> {
        sqlx::query("UPDATE quote_assets SET usd_rate = $2, usd_rate_at = $3 WHERE id = $1")
            .bind(id)
            .bind(usd_rate)
            .bind(Utc::now())
            .execute(pool)
            .await?;
        Ok(())
    }
}

/// `launchpads` — which platform a token launched on.
pub struct LaunchpadRepo;

impl LaunchpadRepo {
    pub async fn all(pool: &PgPool) -> anyhow::Result<Vec<Launchpad>> {
        Ok(sqlx::query_as::<_, Launchpad>("SELECT * FROM launchpads ORDER BY id")
            .fetch_all(pool)
            .await?)
    }

    pub async fn get(pool: &PgPool, id: i16) -> anyhow::Result<Option<Launchpad>> {
        Ok(sqlx::query_as::<_, Launchpad>("SELECT * FROM launchpads WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?)
    }
}

/// A market to upsert (a token's curve/amm instance). `id` is DB-generated.
#[derive(Debug, Clone)]
pub struct NewMarket {
    pub mint_address: String,
    pub launchpad_id: i16,
    pub market_kind: String,
    pub program_id: String,
    pub quote_asset_id: i16,
    pub pool_address: Option<String>,
    pub created_slot: Option<i64>,
}

/// `markets` — a token's 1..n tradeable venue instances.
pub struct MarketRepo;

impl MarketRepo {
    /// Idempotent by the `(mint, launchpad, market_kind)` unique key. Returns the
    /// (possibly pre-existing) row so callers get its generated `id`.
    pub async fn upsert(pool: &PgPool, m: &NewMarket) -> anyhow::Result<Market> {
        let row = sqlx::query_as::<_, Market>(
            "INSERT INTO markets (mint_address, launchpad_id, market_kind, program_id, \
                                  quote_asset_id, pool_address, created_slot) \
             VALUES ($1,$2,$3,$4,$5,$6,$7) \
             ON CONFLICT (mint_address, launchpad_id, market_kind) DO UPDATE SET \
                 program_id   = EXCLUDED.program_id, \
                 pool_address = COALESCE(EXCLUDED.pool_address, markets.pool_address) \
             RETURNING *",
        )
        .bind(&m.mint_address)
        .bind(m.launchpad_id)
        .bind(&m.market_kind)
        .bind(&m.program_id)
        .bind(m.quote_asset_id)
        .bind(&m.pool_address)
        .bind(m.created_slot)
        .fetch_one(pool)
        .await?;
        Ok(row)
    }

    pub async fn by_mint(pool: &PgPool, mint_address: &str) -> anyhow::Result<Vec<Market>> {
        Ok(
            sqlx::query_as::<_, Market>("SELECT * FROM markets WHERE mint_address = $1 ORDER BY id")
                .bind(mint_address)
                .fetch_all(pool)
                .await?,
        )
    }
}
