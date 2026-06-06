#![allow(dead_code)]
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
    pub volume: f64,
    pub market_cap: Option<f64>,
    pub trade_count: i64,
    pub last_trade_at: Option<DateTime<Utc>>,
    pub current_price: Option<f64>,
    pub is_rugged: bool,
    pub is_migrated: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Wall-clock time of the last successful manual sync, if any.
    pub last_synced_at: Option<DateTime<Utc>>,
}

impl TokenInfo {
    pub fn new(
        mint_address: String,
        ath_price: Option<f64>,
        ath_timestamp: Option<DateTime<Utc>>,
        age: Option<i64>,
        volume: f64,
        market_cap: Option<f64>,
        trade_count: i64,
        last_trade_at: Option<DateTime<Utc>>,
        current_price: Option<f64>,
        is_rugged: bool,
        is_migrated: bool,
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
            volume,
            market_cap,
            trade_count,
            last_trade_at,
            current_price,
            is_rugged,
            is_migrated,
            created_at,
            updated_at,
            last_synced_at,
        }
    }
}
