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
    /// A paper-test run reached its total-token cap and every position exited;
    /// the rule was auto-deactivated. Not mint-scoped — always delivered.
    PaperTestFinished {
        rule_id: uuid::Uuid,
        rule_name: String,
        run_seq: i64,
        tokens_traded: i64,
        timestamp: DateTime<Utc>,
    },
    /// A tpsl rule list changed for `strategy` ("tpsl1" | "tpsl2") — a rule was
    /// created, updated, deleted, or moved through a lifecycle transition. A bare
    /// signal (no payload beyond the strategy); the client refetches the list.
    /// Not mint-scoped — always delivered.
    TpslRulesChanged { strategy: String },
    /// A tpsl position opened, closed, or changed status. `rule_id` scopes it to
    /// the owning rule so a client refetches only that rule's positions (and the
    /// rule's open-position count). Not mint-scoped — always delivered.
    TpslPositionsChanged { strategy: String, rule_id: uuid::Uuid },
}
