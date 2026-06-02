use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::trade::TradeType;

/// Lightweight signal for the strategy hot path (reads `TokenCache` after update).
#[derive(Debug, Clone)]
pub struct StrategyPing {
    pub mint: String,
    pub kind: IngestKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngestKind {
    TokenCreated,
    Trade,
    Migrated,
    CreatorActivity,
}

/// Cold-lane SSE notification (enriched from cache in the HTTP handler).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SseEvent {
    TokenCreated {
        mint: String,
        tx_signature: String,
        slot: u64,
        timestamp: DateTime<Utc>,
    },
    TradeExecuted {
        mint: String,
        wallet: String,
        trade_type: TradeType,
        sol_amount: f64,
        token_amount: f64,
        price_per_token: f64,
        tx_signature: String,
        slot: u64,
        timestamp: DateTime<Utc>,
    },
    LiquidityAdded {
        mint: String,
        wallet: String,
        sol_amount: f64,
        token_amount: f64,
        tx_signature: String,
        slot: u64,
        timestamp: DateTime<Utc>,
    },
    LiquidityRemoved {
        mint: String,
        wallet: String,
        sol_amount: f64,
        token_amount: f64,
        tx_signature: String,
        slot: u64,
        timestamp: DateTime<Utc>,
    },
}
