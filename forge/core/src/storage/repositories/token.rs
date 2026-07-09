//! Domain B token repos: tokens, market state, sync watermark.

use chrono::Utc;
use serde_json::json;
use sqlx::PgPool;

use crate::models::{NewToken, Token, TokenMarketState, TokenOverview, TokenSyncState};

/// `tokens` — static creation facts (write-once).
pub struct TokenRepo;

impl TokenRepo {
    /// Insert creation facts. Write-once: a re-seen mint is a no-op
    /// (`ON CONFLICT DO NOTHING`). Returns `true` if a new row landed.
    pub async fn insert(pool: &PgPool, t: &NewToken) -> anyhow::Result<bool> {
        let res = sqlx::query(
            "INSERT INTO tokens \
                (mint_address, launchpad_id, quote_asset_id, creator_wallet, is_own_launch, \
                 name, symbol, decimals, token_program_id, initial_supply_base, \
                 initial_buy_quote, creation_slot, creation_tx_signature, ix_labels, meta, created_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16) \
             ON CONFLICT (mint_address) DO NOTHING",
        )
        .bind(&t.mint_address)
        .bind(t.launchpad_id)
        .bind(t.quote_asset_id)
        .bind(&t.creator_wallet)
        .bind(t.is_own_launch)
        .bind(&t.name)
        .bind(&t.symbol)
        .bind(t.decimals)
        .bind(&t.token_program_id)
        .bind(t.initial_supply_base)
        .bind(t.initial_buy_quote)
        .bind(t.creation_slot)
        .bind(&t.creation_tx_signature)
        .bind(t.ix_labels.clone().unwrap_or_else(|| json!([])))
        .bind(t.meta.clone().unwrap_or_else(|| json!({})))
        .bind(t.created_at.unwrap_or_else(Utc::now))
        .execute(pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    pub async fn get(pool: &PgPool, mint_address: &str) -> anyhow::Result<Option<Token>> {
        Ok(sqlx::query_as::<_, Token>("SELECT * FROM tokens WHERE mint_address = $1")
            .bind(mint_address)
            .fetch_optional(pool)
            .await?)
    }

    /// Read the derived overview (age / display price / market cap / USD) — the
    /// same view for every quote, no schema branch.
    pub async fn overview(pool: &PgPool, mint_address: &str) -> anyhow::Result<Option<TokenOverview>> {
        Ok(
            sqlx::query_as::<_, TokenOverview>("SELECT * FROM token_overview WHERE mint_address = $1")
                .bind(mint_address)
                .fetch_optional(pool)
                .await?,
        )
    }
}

/// `token_market_state` — hot-updated live metrics (1:1 with tokens).
pub struct TokenMarketStateRepo;

impl TokenMarketStateRepo {
    /// Upsert the metrics row. `updated_at` is stamped now on every write.
    pub async fn upsert(pool: &PgPool, s: &TokenMarketState) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO token_market_state \
                (mint_address, current_price_quote, ath_price_quote, ath_at, volume_quote, \
                 trade_count, last_trade_at, is_dead, is_migrated, updated_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9, now()) \
             ON CONFLICT (mint_address) DO UPDATE SET \
                 current_price_quote = EXCLUDED.current_price_quote, \
                 ath_price_quote     = EXCLUDED.ath_price_quote, \
                 ath_at              = EXCLUDED.ath_at, \
                 volume_quote        = EXCLUDED.volume_quote, \
                 trade_count         = EXCLUDED.trade_count, \
                 last_trade_at       = EXCLUDED.last_trade_at, \
                 is_dead             = EXCLUDED.is_dead, \
                 is_migrated         = EXCLUDED.is_migrated, \
                 updated_at          = now()",
        )
        .bind(&s.mint_address)
        .bind(s.current_price_quote)
        .bind(s.ath_price_quote)
        .bind(s.ath_at)
        .bind(s.volume_quote)
        .bind(s.trade_count)
        .bind(s.last_trade_at)
        .bind(s.is_dead)
        .bind(s.is_migrated)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn get(pool: &PgPool, mint_address: &str) -> anyhow::Result<Option<TokenMarketState>> {
        Ok(sqlx::query_as::<_, TokenMarketState>(
            "SELECT * FROM token_market_state WHERE mint_address = $1",
        )
        .bind(mint_address)
        .fetch_optional(pool)
        .await?)
    }
}

/// `token_sync_state` — per-(mint, market) ingest watermark.
pub struct TokenSyncStateRepo;

impl TokenSyncStateRepo {
    pub async fn upsert(
        pool: &PgPool,
        mint_address: &str,
        market_id: i64,
        last_sig: Option<&str>,
        last_slot: Option<i64>,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO token_sync_state (mint_address, market_id, last_sig, last_slot, last_synced_at) \
             VALUES ($1,$2,$3,$4, now()) \
             ON CONFLICT (mint_address, market_id) DO UPDATE SET \
                 last_sig = EXCLUDED.last_sig, last_slot = EXCLUDED.last_slot, last_synced_at = now()",
        )
        .bind(mint_address)
        .bind(market_id)
        .bind(last_sig)
        .bind(last_slot)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn get(
        pool: &PgPool,
        mint_address: &str,
        market_id: i64,
    ) -> anyhow::Result<Option<TokenSyncState>> {
        Ok(sqlx::query_as::<_, TokenSyncState>(
            "SELECT * FROM token_sync_state WHERE mint_address = $1 AND market_id = $2",
        )
        .bind(mint_address)
        .bind(market_id)
        .fetch_optional(pool)
        .await?)
    }
}
