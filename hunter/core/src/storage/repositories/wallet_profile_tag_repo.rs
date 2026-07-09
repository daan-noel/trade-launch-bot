use sqlx::PgPool;
use uuid::Uuid;

use crate::models::wallet_profile_tag::WalletProfileTag;

pub struct WalletProfileTagRepo {
    pool: PgPool,
}

impl WalletProfileTagRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn list(&self) -> anyhow::Result<Vec<WalletProfileTag>> {
        let rows = sqlx::query_as::<_, WalletProfileTag>(
            "SELECT id, name, color, comment, created_at FROM wallet_profile_tags ORDER BY created_at ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn find_by_ids(&self, ids: &[Uuid]) -> anyhow::Result<Vec<WalletProfileTag>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let rows = sqlx::query_as::<_, WalletProfileTag>(
            "SELECT id, name, color, comment, created_at FROM wallet_profile_tags WHERE id = ANY($1)",
        )
        .bind(ids)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn insert(&self, name: &str, color: &str, comment: Option<&str>) -> anyhow::Result<WalletProfileTag> {
        let row = sqlx::query_as::<_, WalletProfileTag>(
            "INSERT INTO wallet_profile_tags (name, color, comment) VALUES ($1, $2, $3) RETURNING id, name, color, comment, created_at",
        )
        .bind(name)
        .bind(color)
        .bind(comment)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn update(&self, id: Uuid, name: &str, color: &str, comment: Option<&str>) -> anyhow::Result<bool> {
        let res = sqlx::query(
            "UPDATE wallet_profile_tags SET name=$1, color=$2, comment=$3 WHERE id=$4",
        )
        .bind(name)
        .bind(color)
        .bind(comment)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    pub async fn delete(&self, id: Uuid) -> anyhow::Result<bool> {
        let res = sqlx::query("DELETE FROM wallet_profile_tags WHERE id=$1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(res.rows_affected() > 0)
    }
}
