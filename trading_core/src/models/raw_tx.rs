use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A single raw, immutable transaction record from the `raw_txs` hypertable.
///
/// `raw_txs` is the **source-of-truth feed**: it stores the original encoded
/// transaction bytes exactly as received from the transport (LaserStream gRPC
/// today), keyed by `(block_time, tx_signature)`. The `trades` table is a *typed
/// projection* derived from this feed — decoded, normalized rows for strategy
/// eval and the frontend. Keeping the raw payload lets us re-decode/backfill the
/// projection without re-fetching from the chain, and the composite primary key
/// doubles as the dedup key (replays and overlapping live/sync windows collapse
/// to a single row).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawTx {
    /// Raw 64-byte transaction signature (stored as `BYTEA`).
    pub tx_signature: Vec<u8>,
    /// Solana slot the transaction landed in.
    pub slot: i64,
    /// Block time; the leading component of the `(block_time, tx_signature)` PK
    /// and the hypertable partitioning column (1-day chunks).
    pub block_time: DateTime<Utc>,
    /// Position of the transaction within its block.
    pub tx_index: i32,
    /// Original encoded transaction bytes (stored as `BYTEA`).
    pub payload: Vec<u8>,
    /// Origin of the record: `0` = live, `1` = sync.
    pub source: i16,
}

impl RawTx {
    /// Construct a `RawTx` from all of its fields.
    pub fn new(
        tx_signature: Vec<u8>,
        slot: i64,
        block_time: DateTime<Utc>,
        tx_index: i32,
        payload: Vec<u8>,
        source: i16,
    ) -> Self {
        Self {
            tx_signature,
            slot,
            block_time,
            tx_index,
            payload,
            source,
        }
    }
}
