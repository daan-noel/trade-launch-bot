use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Token metrics stored in `tokens_info` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenInfo {
    pub id: Uuid,
    pub mint_address: String,
    pub ath_price: Option<f64>,
    pub ath_timestamp: Option<DateTime<Utc>>,
    pub age: Option<i64>,
    pub volume_sol: f64,
    pub market_cap: Option<f64>,
    pub trade_count: i64,
    pub last_trade_at: Option<DateTime<Utc>>,
    pub current_price: Option<f64>,
    pub is_dead: bool,
    pub is_migrated: bool,
    /// Total buy SOL across trades in the token's creation slot, if computed.
    pub first_slot_buy_sol: Option<f64>,
    /// Total sell SOL across trades in the token's creation slot, if computed.
    pub first_slot_sell_sol: Option<f64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Wall-clock time of the last successful manual sync, if any.
    pub last_synced_at: Option<DateTime<Utc>>,
}

impl TokenInfo {
    // Only constructed by `seed.rs` tests; production builds rows via the repo.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn new(
        mint_address: String,
        ath_price: Option<f64>,
        ath_timestamp: Option<DateTime<Utc>>,
        age: Option<i64>,
        volume_sol: f64,
        market_cap: Option<f64>,
        trade_count: i64,
        last_trade_at: Option<DateTime<Utc>>,
        current_price: Option<f64>,
        is_dead: bool,
        is_migrated: bool,
        first_slot_buy_sol: Option<f64>,
        first_slot_sell_sol: Option<f64>,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
        last_synced_at: Option<DateTime<Utc>>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            mint_address,
            ath_price,
            ath_timestamp,
            age,
            volume_sol,
            market_cap,
            trade_count,
            last_trade_at,
            current_price,
            is_dead,
            is_migrated,
            first_slot_buy_sol,
            first_slot_sell_sol,
            created_at,
            updated_at,
            last_synced_at,
        }
    }
}
