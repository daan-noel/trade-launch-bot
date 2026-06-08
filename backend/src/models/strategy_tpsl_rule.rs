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
    pub p_initial_buy_sol: Option<f64>,
    /// Compute-unit limit constraint (optional).
    pub p_cu_limit: Option<u64>,
    /// Compute-unit price constraint (optional), in micro-lamports per CU.
    pub p_cu_price: Option<u64>,
    /// Filter: match the token's creation-instruction max_sol_cost (optional).
    pub p_max_sol_cost: Option<f64>,
    /// Filter: match the token's creation-instruction spendable_sol_in (optional).
    pub p_spendable_sol_in: Option<f64>,
    /// Concurrency cap: max tokens held open at the same time.
    pub p_max_concurrent_tokens: Option<u64>,
    /// Total cap: max tokens this rule may trade over the whole run.
    pub p_max_total_tokens: Option<u64>,
    /// Instruction labels filter (optional JSON array).
    pub p_ix_labels: Value,
    /// Trade mode: "paper" (paper test) or "real" (real trading)
    pub trade_mode: String,
    /// Amount of SOL to allocate per buy.
    pub buy_amount: f64,
    /// Take profit percentage (e.g., 50 for 50% gain).
    pub take_profit: f64,
    /// Stop loss percentage (e.g., 20 for 20% loss).
    pub stop_loss: f64,
    /// E1 · Trailing stop percentage: exit when price falls this far below the
    /// peak-since-entry. `None`/`0` disables (per `ignore_zero_f64`).
    pub p_trailing_stop_pct: Option<f64>,
    /// Price tolerance percent when matching p_initial_buy_sol.
    pub tolerance_pct: f64,
    /// Whether this rule is currently active.
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl StrategyTPSLRule {
    pub fn new(
        rule_name: String,
        p_initial_buy_sol: Option<f64>,
        p_cu_limit: Option<u64>,
        p_cu_price: Option<u64>,
        p_ix_labels: Value,
        trade_mode: String,
        buy_amount: f64,
        take_profit: f64,
        stop_loss: f64,
        p_max_sol_cost: Option<f64>,
        p_spendable_sol_in: Option<f64>,
        p_max_concurrent_tokens: Option<u64>,
        p_max_total_tokens: Option<u64>,
        tolerance_pct: Option<f64>,
        p_trailing_stop_pct: Option<f64>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            rule_name,
            p_initial_buy_sol,
            p_cu_limit,
            p_cu_price,
            p_ix_labels,
            trade_mode,
            buy_amount,
            take_profit,
            stop_loss,
            p_trailing_stop_pct,
            p_max_sol_cost,
            p_spendable_sol_in,
            p_max_concurrent_tokens: p_max_concurrent_tokens,
            p_max_total_tokens: p_max_total_tokens,
            tolerance_pct: tolerance_pct.unwrap_or(0.0),
            is_active: false,
            created_at: now,
            updated_at: now,
        }
    }
}
