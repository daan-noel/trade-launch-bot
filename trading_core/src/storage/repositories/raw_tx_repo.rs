//! Repository for the `raw_txs` hypertable — the append-only, immutable
//! source-of-truth transaction feed. Rows are keyed by
//! `(block_time, tx_signature)`, which is also the dedup key, so every insert
//! uses `ON CONFLICT (block_time, tx_signature) DO NOTHING`. See
//! [`crate::models::raw_tx::RawTx`].

use chrono::{DateTime, Utc};

use sqlx::PgPool;

use crate::models::raw_tx::RawTx;

pub struct RawTxRepo {
    pool: PgPool,
}

// ---------------------------------------------------------------------------
// DB row — keeps sqlx derives out of domain models
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct RawTxDbRow {
    tx_signature: Vec<u8>,
    slot: i64,
    block_time: DateTime<Utc>,
    tx_index: i32,
    payload: Vec<u8>,
    source: i16,
}

impl From<RawTxDbRow> for RawTx {
    fn from(r: RawTxDbRow) -> Self {
        Self {
            tx_signature: r.tx_signature,
            slot: r.slot,
            block_time: r.block_time,
            tx_index: r.tx_index,
            payload: r.payload,
            source: r.source,
        }
    }
}

// ---------------------------------------------------------------------------
// Repo
// ---------------------------------------------------------------------------

/// Columns selected for `RawTxDbRow`, in struct order. Explicit (not `SELECT *`)
/// so the read's wire contract is decoupled from the physical table layout.
const RAW_TX_COLS: &str = "tx_signature, slot, block_time, tx_index, payload, source";

/// Max rows per multi-row insert batch. With 6 bind params per row this stays
/// well under sqlx 0.6's 65535 bind-param ceiling (sqlx silently wraps the
/// param count via `len() as i16` past that limit), with headroom to spare.
const INSERT_CHUNK_ROWS: usize = 8000;

impl RawTxRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Insert a single raw transaction. Idempotent on replay — duplicates on the
    /// `(block_time, tx_signature)` primary key are silently ignored.
    pub async fn insert(&self, tx: &RawTx) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO raw_txs
                (tx_signature, slot, block_time, tx_index, payload, source)
              VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (block_time, tx_signature) DO NOTHING
            "#,
        )
        .bind(tx.tx_signature.as_slice())
        .bind(tx.slot)
        .bind(tx.block_time)
        .bind(tx.tx_index)
        .bind(tx.payload.as_slice())
        .bind(tx.source)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Bulk-insert a slice of raw transactions, chunked into multi-row inserts to
    /// stay under the bind-param ceiling (see [`INSERT_CHUNK_ROWS`]). Duplicates
    /// are silently ignored (`ON CONFLICT DO NOTHING`), matching `insert`'s
    /// semantics. Empty slice is a no-op.
    pub async fn insert_many(&self, txs: &[RawTx]) -> anyhow::Result<()> {
        if txs.is_empty() {
            return Ok(());
        }
        for chunk in txs.chunks(INSERT_CHUNK_ROWS) {
            let mut qb = sqlx::QueryBuilder::new(
                "INSERT INTO raw_txs \
                 (tx_signature, slot, block_time, tx_index, payload, source) ",
            );
            qb.push_values(chunk, |mut b, tx| {
                b.push_bind(tx.tx_signature.as_slice())
                    .push_bind(tx.slot)
                    .push_bind(tx.block_time)
                    .push_bind(tx.tx_index)
                    .push_bind(tx.payload.as_slice())
                    .push_bind(tx.source);
            });
            qb.push(" ON CONFLICT (block_time, tx_signature) DO NOTHING");
            qb.build().execute(&self.pool).await?;
        }
        Ok(())
    }

    /// Primary-key lookup by `(block_time, tx_signature)`.
    pub async fn find_by_signature(
        &self,
        block_time: DateTime<Utc>,
        tx_signature: &[u8],
    ) -> anyhow::Result<Option<RawTx>> {
        let row = sqlx::query_as::<_, RawTxDbRow>(&format!(
            "SELECT {RAW_TX_COLS} FROM raw_txs WHERE block_time = $1 AND tx_signature = $2"
        ))
        .bind(block_time)
        .bind(tx_signature)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(RawTx::from))
    }
}
