use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Represents a TPSL (Take Profit Stop Loss) strategy rule.
/// Each rule defines the conditions and parameters for when to buy and sell a token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyTPSLRule {
    pub id: Uuid,
    /// Human-readable name for this rule.
    pub rule_name: String,
    /// Initial buy amount in SOL for filtering token creation events.
    pub p_initial_buy_sol: f64,
    /// Compute-unit limit constraint (optional).
    pub p_cu_limit: Option<u64>,
    /// Compute-unit price constraint (optional), in micro-lamports per CU.
    pub p_cu_price: Option<u64>,
    /// Instruction labels filter (optional JSON array).
    pub p_ix_labels: Value,
    /// Amount of SOL to allocate per buy.
    pub buy_amount: f64,
    /// Take profit percentage (e.g., 50 for 50% gain).
    pub take_profit: f64,
    /// Stop loss percentage (e.g., 20 for 20% loss).
    pub stop_loss: f64,
    /// Whether this rule is currently active.
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl StrategyTPSLRule {
    pub fn new(
        rule_name: String,
        p_initial_buy_sol: f64,
        p_cu_limit: Option<u64>,
        p_cu_price: Option<u64>,
        p_ix_labels: Value,
        buy_amount: f64,
        take_profit: f64,
        stop_loss: f64,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            rule_name,
            p_initial_buy_sol,
            p_cu_limit,
            p_cu_price,
            p_ix_labels,
            buy_amount,
            take_profit,
            stop_loss,
            is_active: true,
            created_at: now,
            updated_at: now,
        }
    }
}
