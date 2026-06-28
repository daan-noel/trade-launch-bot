use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::models::token_sync_state::TokenSyncState;

/// Repository for the `token_sync_state` table: a per-(mint, venue) backfill /
/// sync cursor keyed on `(mint_address, venue)`.
pub struct TokenSyncStateRepo {
    pool: PgPool,
}

// ---------------------------------------------------------------------------
// DB row — keeps sqlx derives out of the domain model
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct TokenSyncStateDbRow {
    mint_address: String,
    venue: String,
    last_sig: Option<String>,
    last_slot: Option<i64>,
    last_synced_at: Option<DateTime<Utc>>,
}

impl From<TokenSyncStateDbRow> for TokenSyncState {
    fn from(r: TokenSyncStateDbRow) -> Self {
        Self {
            mint_address: r.mint_address,
            venue: r.venue,
            last_sig: r.last_sig,
            last_slot: r.last_slot,
            last_synced_at: r.last_synced_at,
        }
    }
}

/// Columns selected for `TokenSyncStateDbRow`, in struct order. Explicit (not
/// `SELECT *`) so the query's wire contract stays decoupled from the table layout.
const SYNC_COLS: &str = "mint_address, venue, last_sig, last_slot, last_synced_at";

impl TokenSyncStateRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Insert or refresh the sync cursor for a (mint, venue).
    pub async fn upsert(&self, st: &TokenSyncState) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO token_sync_state
                (mint_address, venue, last_sig, last_slot, last_synced_at)
              VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (mint_address, venue) DO UPDATE SET
                last_sig = EXCLUDED.last_sig,
                last_slot = EXCLUDED.last_slot,
                last_synced_at = EXCLUDED.last_synced_at
            "#,
        )
        .bind(&st.mint_address)
        .bind(&st.venue)
        .bind(&st.last_sig)
        .bind(st.last_slot)
        .bind(st.last_synced_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Fetch the full sync cursor for a (mint, venue), if any.
    pub async fn get(&self, mint: &str, venue: &str) -> anyhow::Result<Option<TokenSyncState>> {
        let row = sqlx::query_as::<_, TokenSyncStateDbRow>(&format!(
            "SELECT {SYNC_COLS} FROM token_sync_state WHERE mint_address = $1 AND venue = $2"
        ))
        .bind(mint)
        .bind(venue)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(TokenSyncState::from))
    }

    /// Fetch just the `last_slot` cursor for a (mint, venue).
    pub async fn last_slot(&self, mint: &str, venue: &str) -> anyhow::Result<Option<i64>> {
        let slot: Option<i64> = sqlx::query_scalar(
            "SELECT last_slot FROM token_sync_state WHERE mint_address = $1 AND venue = $2",
        )
        .bind(mint)
        .bind(venue)
        .fetch_optional(&self.pool)
        .await?
        .flatten();

        Ok(slot)
    }

    /// Fetch just the `last_sig` cursor for a (mint, venue).
    pub async fn last_sig(&self, mint: &str, venue: &str) -> anyhow::Result<Option<String>> {
        let sig: Option<String> = sqlx::query_scalar(
            "SELECT last_sig FROM token_sync_state WHERE mint_address = $1 AND venue = $2",
        )
        .bind(mint)
        .bind(venue)
        .fetch_optional(&self.pool)
        .await?
        .flatten();

        Ok(sig)
    }

    /// Fetch the sync cursors for all venues of a single mint.
    pub async fn for_mint(&self, mint: &str) -> anyhow::Result<Vec<TokenSyncState>> {
        let rows = sqlx::query_as::<_, TokenSyncStateDbRow>(&format!(
            "SELECT {SYNC_COLS} FROM token_sync_state WHERE mint_address = $1"
        ))
        .bind(mint)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(TokenSyncState::from).collect())
    }
}
