use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::transaction::RawTransaction;

pub struct TransactionRepo {
    pool: PgPool,
}

// ---------------------------------------------------------------------------
// DB row — wraps JSONB in sqlx::types::Json for FromRow compatibility
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct RawTransactionDbRow {
    id: Uuid,
    signature: String,
    slot: i64,
    block_time: DateTime<Utc>,
    raw_data: sqlx::types::Json<Value>,
    received_at: DateTime<Utc>,
}

impl From<RawTransactionDbRow> for RawTransaction {
    fn from(r: RawTransactionDbRow) -> Self {
        Self {
            id: r.id,
            signature: r.signature,
            slot: r.slot as u64,
            block_time: r.block_time,
            raw_data: r.raw_data.0,
            received_at: r.received_at,
        }
    }
}

// ---------------------------------------------------------------------------
// Repo
// ---------------------------------------------------------------------------

impl TransactionRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Persist the Helius transaction result. Ignores duplicates (idempotent).
    pub async fn insert(&self, tx: &RawTransaction) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO raw_transactions
                (id, signature, slot, block_time, raw_data, received_at)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (signature) DO NOTHING
            "#,
        )
        .bind(tx.id)
        .bind(&tx.signature)
        .bind(tx.slot as i64)
        .bind(tx.block_time)
        .bind(sqlx::types::Json(&tx.raw_data))
        .bind(tx.received_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    #[allow(dead_code)]
    pub async fn find_by_signature(&self, sig: &str) -> anyhow::Result<Option<RawTransaction>> {
        let row = sqlx::query_as::<_, RawTransactionDbRow>(
            "SELECT * FROM raw_transactions WHERE signature = $1",
        )
        .bind(sig)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(RawTransaction::from))
    }

    #[allow(dead_code)]
    pub async fn exists(&self, sig: &str) -> anyhow::Result<bool> {
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(1) FROM raw_transactions WHERE signature = $1")
                .bind(sig)
                .fetch_one(&self.pool)
                .await?;

        Ok(count > 0)
    }
}
