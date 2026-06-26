use std::sync::Arc;

use sqlx::PgPool;

use crate::models::transaction::RawTransaction;

/// Rows per `insert_many` statement. One Postgres statement is capped at 65535
/// bind parameters (the wire int16 count, which sqlx 0.6 wraps without a guard);
/// at 7 binds/row the ceiling is 9362 rows. Kept lower than that anyway — each
/// row carries a heavy `raw_data` JSONB blob, so a smaller chunk bounds the
/// per-statement message size, not just the parameter count.
const RAW_INSERT_CHUNK: usize = 5000;

pub struct TransactionRepo {
    pool: PgPool,
}

impl TransactionRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Persist a raw transaction result, tagged by `source` ('live' = real-time
    /// LaserStream ingest pipeline, 'sync' = token_sync backfill — both fetch
    /// methods). Plain insert —
    /// `raw_transactions` is weekly range-partitioned on `received_at`, and a
    /// unique constraint on a partitioned table must include the partition key
    /// (which isn't stable across reconnect re-delivery), so there's no
    /// `ON CONFLICT (signature)` dedup. Each path writes a signature once per
    /// run; rare duplicate rows are tolerated in this write-only replay/analysis
    /// table.
    pub async fn insert(&self, tx: &RawTransaction, source: &str) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO raw_transactions
                (id, signature, slot, block_time, raw_data, received_at, source)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(tx.id)
        .bind(&tx.signature)
        .bind(tx.slot as i64)
        .bind(tx.block_time)
        .bind(sqlx::types::Json(&tx.raw_data))
        .bind(tx.received_at)
        .bind(source)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Bulk version of [`insert`] — one multi-row statement per [`RAW_INSERT_CHUNK`]
    /// rows, tagging every row with the same `source`. Like [`insert`] it's a plain
    /// insert (no conflict target on the partitioned table). Used by the live ingest
    /// DB-writer (≤256 rows = one chunk) and the token_sync backfill, whose larger
    /// flushes are split across statements to stay under the 65535 bind-param ceiling.
    pub async fn insert_many(&self, txs: &[Arc<RawTransaction>], source: &str) -> anyhow::Result<()> {
        for chunk in txs.chunks(RAW_INSERT_CHUNK) {
            let mut qb: sqlx::QueryBuilder<sqlx::Postgres> = sqlx::QueryBuilder::new(
                "INSERT INTO raw_transactions \
                 (id, signature, slot, block_time, raw_data, received_at, source) ",
            );
            qb.push_values(chunk, |mut b, tx| {
                b.push_bind(tx.id)
                    .push_bind(&tx.signature)
                    .push_bind(tx.slot as i64)
                    .push_bind(tx.block_time)
                    .push_bind(sqlx::types::Json(&tx.raw_data))
                    .push_bind(tx.received_at)
                    .push_bind(source);
            });
            qb.build().execute(&self.pool).await?;
        }

        Ok(())
    }
}
