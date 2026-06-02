use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A single buy or sell on a tracked token's bonding curve.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trade {
    pub id: Uuid,
    pub mint_address: String,
    pub wallet_address: String,
    pub trade_type: TradeType,
    /// SOL amount (human-readable, already divided by lamports).
    pub sol_amount: f64,
    pub token_amount: f64,
    /// SOL per token at execution time.
    pub price_per_token: f64,
    pub tx_signature: String,
    /// Index of this trade within the transaction (0 = first pump leg).
    pub leg_index: u32,
    pub slot: u64,
    pub block_time: DateTime<Utc>,

    // ── On-chain state snapshot (from TradeEvent "Program data:" log) ─────────
    /// Virtual SOL reserves on the bonding curve at the time of the trade.
    pub virtual_sol_reserves: Option<f64>,
    pub virtual_token_reserves: Option<f64>,
    /// Real (non-virtual) SOL reserves — used for graduation progress.
    pub real_sol_reserves: Option<f64>,
    pub real_token_reserves: Option<f64>,

    // ── Instruction context ───────────────────────────────────────────────────
    /// Trade-side label for executed trades: "Buy" or "Sell".
    pub instruction_type: String,
    /// Ordered list of human-readable labels for every top-level instruction in
    /// the transaction, e.g. `["Compute Budget: SetComputeUnitLimit", "Pump.Fun: Buy"]`.
    /// Stored as a JSON array.
    pub instruction_labels: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TradeType {
    Buy,
    Sell,
}

impl Trade {
    pub fn new(
        mint_address: String,
        wallet_address: String,
        trade_type: TradeType,
        sol_amount: f64,
        token_amount: f64,
        tx_signature: String,
        slot: u64,
        block_time: DateTime<Utc>,
    ) -> Self {
        let price_per_token = if token_amount > 0.0 {
            sol_amount / token_amount
        } else {
            0.0
        };

        Self {
            id: Uuid::new_v4(),
            mint_address,
            wallet_address,
            trade_type,
            sol_amount,
            token_amount,
            price_per_token,
            tx_signature,
            leg_index: 0,
            slot,
            block_time,
            virtual_sol_reserves: None,
            virtual_token_reserves: None,
            real_sol_reserves: None,
            real_token_reserves: None,
            instruction_type: "Unknown".to_string(),
            instruction_labels: serde_json::Value::Array(vec![]),
        }
    }
}
