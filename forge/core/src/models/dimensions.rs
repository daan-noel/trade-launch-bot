//! Domain A dimensions — small, slow-changing, interned onto hot rows.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

/// A quote asset (SOL, USDC, …). Native SOL is the `is_native` row with 9
/// decimals; its base unit is the lamport. `usd_rate` is the cross-quote
/// numeraire (USD per 1 display unit) that makes tokens on different quotes
/// USD-comparable in views.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct QuoteAsset {
    pub id: i16,
    pub mint: String,
    pub symbol: String,
    pub decimals: i16,
    pub is_native: bool,
    pub usd_rate: Option<f64>,
    pub usd_rate_at: Option<DateTime<Utc>>,
}

impl QuoteAsset {
    /// Base units per one display unit (e.g. 1e9 for SOL, 1e6 for USDC).
    pub fn decimals_u8(&self) -> u8 {
        self.decimals.max(0) as u8
    }
    /// Convert a base-unit amount of this quote to its human display value.
    pub fn to_display(&self, base_units: i64) -> f64 {
        crate::units::to_display(base_units, self.decimals_u8())
    }
    /// USD value of a base-unit amount of this quote (`None` if no rate yet).
    pub fn to_usd(&self, base_units: i64) -> Option<f64> {
        self.usd_rate.map(|r| self.to_display(base_units) * r)
    }
}

/// A launchpad (pump.fun, raydium_launchlab, …). A new launchpad is a ROW.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Launchpad {
    pub id: i16,
    pub key: String,
    pub display_name: String,
    pub default_quote_asset_id: i16,
    pub meta: Json,
    pub created_at: DateTime<Utc>,
}

/// A token's tradeable venue instance (curve first, then AMM after graduation).
/// A token has 1..n markets over its life.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Market {
    pub id: i64,
    pub mint_address: String,
    pub launchpad_id: i16,
    pub market_kind: String,
    pub program_id: String,
    pub quote_asset_id: i16,
    pub pool_address: Option<String>,
    pub created_slot: Option<i64>,
}
