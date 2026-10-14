//! Live trading wallets. The pubkey is the id. `key_ref` is a filename, never a key.

use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

/// A row the API may return. No filename, no secret.
#[derive(Debug, Clone, Serialize)]
pub struct TraderWallet {
    pub pubkey: String,
    pub name: String,
    pub status: String,
    pub is_primary: bool,
    pub created_at: DateTime<Utc>,
    pub archived_at: Option<DateTime<Utc>>,
    pub nonce_count: i64,
    pub rule_count: i64,
    pub open_positions: i64,
}

/// The row plus the keystore filename. Stays inside the live process.
#[derive(Debug, Clone)]
pub struct TraderWalletRecord {
    pub pubkey: String,
    pub name: String,
    pub status: String,
    pub key_ref: String,
    pub is_primary: bool,
}

#[derive(Debug, Clone)]
pub struct TraderNonce {
    pub pubkey: String,
    pub authority: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WalletMove {
    pub id: Uuid,
    pub from_wallet: String,
    pub to_wallet: String,
    pub step: String,
    pub error: Option<String>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

/// One open real position the move has to carry.
#[derive(Debug, Clone)]
pub struct OpenWalletPosition {
    pub id: Uuid,
    pub mint_address: String,
    pub token_account: Option<String>,
    pub token_program_id: Option<String>,
    pub status: String,
    pub entry_token_amount: Option<i64>,
}
