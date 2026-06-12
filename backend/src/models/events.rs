use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{token::Token, trade::Trade, transaction::RawTransaction};

/// The central event enum that flows through the internal event bus.
///
/// All layers (services, analyzers, state) communicate exclusively through
/// these variants. No layer holds a reference to raw Helius payloads.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InternalEvent {
    /// A new Pump.fun token was created on-chain.
    TokenCreated(TokenCreatedEvent),

    /// A buy or sell trade was executed on a tracked token.
    TradeExecuted(TradeExecutedEvent),

    /// A token's bonding curve migrated to the PumpSwap AMM.
    TokenMigrated(TokenMigratedEvent),

    /// Liquidity was added to a tracked token's bonding curve.
    LiquidityAdded(LiquidityEvent),

    /// Liquidity was removed from a tracked token's bonding curve
    /// (graduation / rug indicator).
    LiquidityRemoved(LiquidityEvent),

    /// Any transaction by a tracked creator wallet (may overlap with trades).
    CreatorActivityDetected(CreatorActivityEvent),
}

// ---------------------------------------------------------------------------
// Event payloads
// ---------------------------------------------------------------------------

/// Emitted when a new token is detected via the create instruction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenCreatedEvent {
    pub token: Token,
    /// The originating transaction signature.
    pub tx_signature: String,
    pub slot: u64,
    pub timestamp: DateTime<Utc>,
    /// Full raw transaction preserved for replay / debugging.
    pub raw_tx: Arc<RawTransaction>,
}

/// Emitted when a tracked token migrates its bonding curve to PumpSwap.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenMigratedEvent {
    pub mint_address: String,
    pub tx_signature: String,
    pub slot: u64,
    pub timestamp: DateTime<Utc>,
    pub raw_tx: Arc<RawTransaction>,
}

/// Emitted for every buy or sell trade on a tracked token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeExecutedEvent {
    pub trade: Trade,
    pub tx_signature: String,
    pub slot: u64,
    pub timestamp: DateTime<Utc>,
    pub raw_tx: Arc<RawTransaction>,
}

/// Emitted when liquidity changes on a tracked token's bonding curve.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiquidityEvent {
    pub mint_address: String,
    pub wallet_address: String,
    pub sol_amount: f64,
    pub token_amount: f64,
    pub direction: LiquidityDirection,
    pub tx_signature: String,
    pub slot: u64,
    pub timestamp: DateTime<Utc>,
    pub raw_tx: Arc<RawTransaction>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiquidityDirection {
    Add,
    Remove,
}

/// Emitted for any transaction by a known creator wallet on a tracked token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatorActivityEvent {
    pub creator_wallet: String,
    pub mint_address: String,
    pub activity_kind: CreatorActivityKind,
    pub tx_signature: String,
    pub slot: u64,
    pub timestamp: DateTime<Utc>,
    pub raw_tx: Arc<RawTransaction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CreatorActivityKind {
    Create,
    Buy,
    Sell,
    LiquidityAdd,
    LiquidityRemove,
    /// Catch-all for any other creator interaction.
    Other(String),
}
