use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::wallet::Wallet;

pub struct WalletRepo {
    pool: PgPool,
}

// ---------------------------------------------------------------------------
// DB row
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct WalletDbRow {
    id: Uuid,
    address: String,
    first_seen_at: DateTime<Utc>,
    last_seen_at: DateTime<Utc>,
    is_flagged: bool,
    flag_reason: Option<String>,
}

impl From<WalletDbRow> for Wallet {
    fn from(r: WalletDbRow) -> Self {
        Self {
            id: r.id,
            address: r.address,
            first_seen_at: r.first_seen_at,
            last_seen_at: r.last_seen_at,
            is_flagged: r.is_flagged,
            flag_reason: r.flag_reason,
        }
    }
}

// ---------------------------------------------------------------------------
// Repo
// ---------------------------------------------------------------------------

impl WalletRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Insert or update a wallet. Updates `last_seen_at` on conflict.
    pub async fn upsert(&self, wallet: &Wallet) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO wallets (id, address, first_seen_at, last_seen_at, is_flagged, flag_reason)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (address) DO UPDATE
                SET last_seen_at = EXCLUDED.last_seen_at,
                    is_flagged   = EXCLUDED.is_flagged,
                    flag_reason  = EXCLUDED.flag_reason
            "#,
        )
        .bind(wallet.id)
        .bind(&wallet.address)
        .bind(wallet.first_seen_at)
        .bind(wallet.last_seen_at)
        .bind(wallet.is_flagged)
        .bind(&wallet.flag_reason)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn find_by_address(&self, address: &str) -> anyhow::Result<Option<Wallet>> {
        let row = sqlx::query_as::<_, WalletDbRow>("SELECT * FROM wallets WHERE address = $1")
            .bind(address)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.map(Wallet::from))
    }
}
