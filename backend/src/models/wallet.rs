use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A Solana wallet observed interacting with tracked tokens.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Wallet {
    pub id: Uuid,
    /// Base58 public key.
    pub address: String,
    pub first_seen_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub is_flagged: bool,
    pub flag_reason: Option<String>,
}

impl Wallet {
    pub fn new(address: String, now: DateTime<Utc>) -> Self {
        Self {
            id: Uuid::new_v4(),
            address,
            first_seen_at: now,
            last_seen_at: now,
            is_flagged: false,
            flag_reason: None,
        }
    }

    #[allow(dead_code)]
    pub fn touch(&mut self, now: DateTime<Utc>) {
        self.last_seen_at = now;
    }

    pub fn flag(&mut self, reason: String) {
        self.is_flagged = true;
        self.flag_reason = Some(reason);
    }
}
