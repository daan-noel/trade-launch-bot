use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::wallet::Wallet;

pub struct WalletRepo {
    pool: PgPool,
}

#[derive(sqlx::FromRow)]
struct WalletRow {
    id: Uuid,
    profile_id: Uuid,
    address: String,
    is_tracked: bool,
    comment: Option<String>,
    created_at: DateTime<Utc>,
    last_seen_at: Option<DateTime<Utc>>,
}

impl From<WalletRow> for Wallet {
    fn from(r: WalletRow) -> Self {
        Self {
            id: r.id,
            profile_id: r.profile_id,
            address: r.address,
            is_tracked: r.is_tracked,
            comment: r.comment,
            created_at: r.created_at,
            last_seen_at: r.last_seen_at,
        }
    }
}

impl WalletRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn insert(&self, wallet: &Wallet) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO wallets (id, profile_id, address, is_tracked, comment, created_at, last_seen_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(wallet.id)
        .bind(wallet.profile_id)
        .bind(&wallet.address)
        .bind(wallet.is_tracked)
        .bind(&wallet.comment)
        .bind(wallet.created_at)
        .bind(wallet.last_seen_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Update mutable fields (is_tracked, comment) for an existing wallet.
    pub async fn update(&self, id: Uuid, is_tracked: bool, comment: Option<&str>) -> anyhow::Result<()> {
        sqlx::query("UPDATE wallets SET is_tracked=$1, comment=$2 WHERE id=$3")
            .bind(is_tracked)
            .bind(comment)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete(&self, id: Uuid) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM wallets WHERE id=$1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn find_by_id(&self, id: Uuid) -> anyhow::Result<Option<Wallet>> {
        let row = sqlx::query_as::<_, WalletRow>("SELECT * FROM wallets WHERE id=$1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(Wallet::from))
    }

    pub async fn find_by_address(&self, address: &str) -> anyhow::Result<Option<Wallet>> {
        let row = sqlx::query_as::<_, WalletRow>("SELECT * FROM wallets WHERE address=$1")
            .bind(address)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(Wallet::from))
    }

    pub async fn list_by_profile(&self, profile_id: Uuid) -> anyhow::Result<Vec<Wallet>> {
        let rows = sqlx::query_as::<_, WalletRow>(
            "SELECT * FROM wallets WHERE profile_id=$1 ORDER BY created_at ASC",
        )
        .bind(profile_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(Wallet::from).collect())
    }

    /// Wallets for many profiles in a single query (ordered by profile, then
    /// creation), so a list view can group them in memory instead of issuing one
    /// `list_by_profile` per profile (the previous N+1).
    pub async fn list_by_profiles(&self, profile_ids: &[Uuid]) -> anyhow::Result<Vec<Wallet>> {
        let rows = sqlx::query_as::<_, WalletRow>(
            "SELECT * FROM wallets WHERE profile_id = ANY($1) ORDER BY profile_id, created_at ASC",
        )
        .bind(profile_ids)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(Wallet::from).collect())
    }

    /// Touch last_seen_at for a known address. Silently no-ops if address is not in the table.
    pub async fn touch_last_seen(&self, address: &str, now: DateTime<Utc>) -> anyhow::Result<()> {
        sqlx::query("UPDATE wallets SET last_seen_at=$1 WHERE address=$2")
            .bind(now)
            .bind(address)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Bulk version of [`touch_last_seen`] — stamps `now` on every listed address
    /// in one `UPDATE … WHERE address = ANY($2)`. The live ingest DB-writer uses
    /// this to collapse a flush's wallet touches (a hot wallet can appear dozens
    /// of times) into a single statement.
    pub async fn touch_last_seen_many(
        &self,
        addresses: &[String],
        now: DateTime<Utc>,
    ) -> anyhow::Result<()> {
        if addresses.is_empty() {
            return Ok(());
        }
        sqlx::query("UPDATE wallets SET last_seen_at=$1 WHERE address = ANY($2)")
            .bind(now)
            .bind(addresses)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
