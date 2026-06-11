use sqlx::PgPool;

use crate::models::transaction::RawTransaction;

/// Persists raw LaserStream (gRPC) transactions into `raw_transactions_grpc`,
/// kept separate from the WS `raw_transactions` table. Write-only (the adapted
/// JSON is an analysis source; nothing in the live path reads it back).
pub struct TransactionGrpcRepo {
    pool: PgPool,
}

impl TransactionGrpcRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Persist the adapted gRPC transaction result. Ignores duplicates (idempotent).
    pub async fn insert(&self, tx: &RawTransaction) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO raw_transactions_grpc
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
}
