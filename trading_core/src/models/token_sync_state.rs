use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Per-(mint, venue) backfill / sync cursor for the `token_sync_state` table.
///
/// `venue` is one of `'curve'` / `'amm'` (enforced by a DB `CHECK`). The cursor
/// fields are all `Option` because a freshly-tracked (mint, venue) pair has no
/// sync progress yet.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenSyncState {
    /// SPL mint address (base58); FKs to `tokens.mint_address`.
    pub mint_address: String,
    /// Sync venue — `'curve'` or `'amm'`.
    pub venue: String,
    /// Last transaction signature synced for this (mint, venue).
    pub last_sig: Option<String>,
    /// Last slot synced for this (mint, venue).
    pub last_slot: Option<i64>,
    /// Wall-clock time of the last successful sync.
    pub last_synced_at: Option<DateTime<Utc>>,
}
