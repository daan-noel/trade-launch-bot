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
    /// Spot price from bonding-curve reserves (`virtual_sol / virtual_token`).
    pub fn curve_spot_price(&self) -> Option<f64> {
        match (self.virtual_sol_reserves, self.virtual_token_reserves) {
            (Some(vsol), Some(vtok)) if vtok > 0.0 => Some(vsol / vtok),
            _ => None,
        }
    }

    /// Overwrite `price_per_token` with curve spot price when reserves are present.
    pub fn apply_curve_price(&mut self) {
        if let Some(spot) = self.curve_spot_price() {
            self.price_per_token = spot;
        }
    }

    /// Spot price on the bonding curve immediately before this trade executed.
    ///
    /// TradeEvent reserves are post-trade; this inverts the trade deltas. Returns
    /// `None` when reserves are missing or the inverted state is invalid.
    pub fn price_before_from_reserves(&self) -> Option<f64> {
        let vsol_post = self.virtual_sol_reserves?;
        let vtok_post = self.virtual_token_reserves?;
        let (vsol_pre, vtok_pre) = match self.trade_type {
            TradeType::Buy => (vsol_post - self.sol_amount, vtok_post + self.token_amount),
            TradeType::Sell => (vsol_post + self.sol_amount, vtok_post - self.token_amount),
        };
        if vsol_pre > 0.0 && vtok_pre > 0.0 {
            Some(vsol_pre / vtok_pre)
        } else {
            None
        }
    }

    /// Pre-execution spot: inverted reserves when available, else `fallback_after`.
    pub fn price_before_execution(&self, fallback_after: Option<f64>) -> f64 {
        self.price_before_from_reserves()
            .or(fallback_after)
            .unwrap_or(self.price_per_token)
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn buy_with_reserves(sol: f64, tokens: f64, vsol_post: f64, vtok_post: f64) -> Trade {
        let mut t = Trade::new(
            "mint".into(),
            "user".into(),
            TradeType::Buy,
            sol,
            tokens,
            "sig".into(),
            1,
            Utc::now(),
        );
        t.virtual_sol_reserves = Some(vsol_post);
        t.virtual_token_reserves = Some(vtok_post);
        t.apply_curve_price();
        t
    }

    #[test]
    fn price_before_from_reserves_buy() {
        let t = buy_with_reserves(10.0, 1_000_000.0, 110.0, 900_000.0);
        let pre = t.price_before_from_reserves().unwrap();
        assert!((pre - 100.0 / 1_900_000.0).abs() < 1e-12);
        assert!(pre < t.price_per_token);
    }

    #[test]
    fn price_before_execution_falls_back_to_previous_post() {
        let first = buy_with_reserves(1.0, 100.0, 11.0, 900.0);
        let post_first = first.price_per_token;
        let mut second = buy_with_reserves(10.0, 100.0, 21.0, 800.0);
        second.virtual_sol_reserves = None;
        second.virtual_token_reserves = None;
        second.price_per_token = 0.03;
        assert!((second.price_before_execution(Some(post_first)) - post_first).abs() < 1e-12);
    }
}
