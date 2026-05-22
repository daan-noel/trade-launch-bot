use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// The Helius transaction result stored verbatim for replay support.
/// Stores only the params.result portion from the Helius webhook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawTransaction {
    pub id: Uuid,
    pub signature: String,
    pub slot: u64,
    pub block_time: DateTime<Utc>,
    /// The params.result from Helius webhook — stored as-is, never mutated.
    pub raw_data: Value,
    pub received_at: DateTime<Utc>,
}

impl RawTransaction {
    pub fn new(signature: String, slot: u64, block_time: DateTime<Utc>, raw_data: Value) -> Self {
        Self {
            id: Uuid::new_v4(),
            signature,
            slot,
            block_time,
            raw_data,
            received_at: Utc::now(),
        }
    }
}
