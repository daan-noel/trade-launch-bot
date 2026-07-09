//! Domain B — token identity + market state.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

/// Static creation facts (write-once), natural key = `mint_address`.
/// Amounts are base units (`initial_supply_base` token, `initial_buy_quote` quote).
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Token {
    pub mint_address: String,
    pub launchpad_id: i16,
    pub quote_asset_id: i16,
    pub creator_wallet: String,
    pub is_own_launch: bool,
    pub name: String,
    pub symbol: String,
    pub decimals: i16,
    pub token_program_id: Option<String>,
    pub initial_supply_base: Option<i64>,
    pub initial_buy_quote: Option<i64>,
    pub creation_slot: Option<i64>,
    pub creation_tx_signature: String,
    pub ix_labels: Json,
    pub meta: Json,
    pub created_at: DateTime<Utc>,
}

/// Insert form of [`Token`] (write-once creation facts). `created_at` defaults to
/// now at the repo boundary when `None`.
#[derive(Debug, Clone, Default)]
pub struct NewToken {
    pub mint_address: String,
    pub launchpad_id: i16,
    pub quote_asset_id: i16,
    pub creator_wallet: String,
    pub is_own_launch: bool,
    pub name: String,
    pub symbol: String,
    pub decimals: i16,
    pub token_program_id: Option<String>,
    pub initial_supply_base: Option<i64>,
    pub initial_buy_quote: Option<i64>,
    pub creation_slot: Option<i64>,
    pub creation_tx_signature: String,
    pub ix_labels: Option<Json>,
    pub meta: Option<Json>,
    pub created_at: Option<DateTime<Utc>>,
}

/// Hot-updated live metrics (was `tokens_info`). Prices are RAW RATIOS
/// (quote base units per base base unit); display/USD is applied in views.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TokenMarketState {
    pub mint_address: String,
    pub current_price_quote: Option<f64>,
    pub ath_price_quote: Option<f64>,
    pub ath_at: Option<DateTime<Utc>>,
    pub volume_quote: i64,
    pub trade_count: i64,
    pub last_trade_at: Option<DateTime<Utc>>,
    pub is_dead: bool,
    pub is_migrated: bool,
    pub updated_at: DateTime<Utc>,
}

/// Per-(mint, market) ingest watermark.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TokenSyncState {
    pub mint_address: String,
    pub market_id: i64,
    pub last_sig: Option<String>,
    pub last_slot: Option<i64>,
    pub last_synced_at: Option<DateTime<Utc>>,
}

/// `token_overview` view row — identity + market state + derived display/USD
/// (age, price, market cap). Derived-never-stored; the view applies decimals +
/// `usd_rate`, so a SOL-quoted and a USDC-quoted token are USD-comparable here.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TokenOverview {
    pub mint_address: String,
    pub launchpad_id: i16,
    pub quote_asset_id: i16,
    pub creator_wallet: String,
    pub is_own_launch: bool,
    pub name: String,
    pub symbol: String,
    pub decimals: i16,
    pub initial_supply_base: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub current_price_quote: Option<f64>,
    pub volume_quote: Option<i64>,
    pub trade_count: Option<i64>,
    pub is_dead: Option<bool>,
    pub is_migrated: Option<bool>,
    pub quote_symbol: String,
    pub quote_decimals: i16,
    pub quote_usd_rate: Option<f64>,
    pub age_secs: Option<i64>,
    pub price_quote_display: Option<f64>,
    pub price_usd: Option<f64>,
    pub market_cap_quote: Option<f64>,
    pub market_cap_usd: Option<f64>,
}
