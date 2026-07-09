use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A manually managed Solana wallet belonging to a WalletProfile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Wallet {
    pub id: Uuid,
    pub profile_id: Uuid,
    /// Base58 public key — validated on creation (32–44 chars, base58 alphabet).
    pub address: String,
    pub is_tracked: bool,
    pub comment: Option<String>,
    pub created_at: DateTime<Utc>,
    /// Set by ingest when a trade from this address is observed; null until first seen.
    pub last_seen_at: Option<DateTime<Utc>>,
}

/// Validate a Solana wallet address: base58 alphabet, 32–44 chars.
pub fn validate_solana_address(address: &str) -> Result<(), String> {
    const BASE58_ALPHABET: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    let len = address.len();
    if !(32..=44).contains(&len) {
        return Err(format!(
            "invalid address length {len}: expected 32–44 characters"
        ));
    }
    for ch in address.bytes() {
        if !BASE58_ALPHABET.contains(&ch) {
            return Err(format!(
                "invalid character '{}' in address",
                ch as char
            ));
        }
    }
    Ok(())
}
