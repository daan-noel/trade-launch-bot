use std::sync::Arc;

use anyhow::Result;

use crate::trader::PumpFunTrader;

/// TradingService provides a lightweight bridge to the on-chain PumpFun trader.
#[derive(Clone)]
pub struct TradingService {
    trader: Arc<PumpFunTrader>,
}

impl TradingService {
    pub fn new(trader: Arc<PumpFunTrader>) -> Self {
        Self { trader }
    }

    pub async fn buy_token(
        &self,
        token_mint: &str,
        creator: &str,
        token_program_id: &str,
        sol_amount: f64,
    ) -> Result<bool> {
        self.trader.buy_token(token_mint, creator, token_program_id, sol_amount).await
    }

    /// Return the trader wallet public key as a base58 string.
    pub fn wallet_pubkey(&self) -> String {
        // Access via PumpFunTrader public method (added)
        self.trader.wallet_pubkey()
    }

    pub async fn sell_token(
        &self,
        token_mint: &str,
        token_amount: u64,
        creator_override: Option<&str>,
        is_cashback: bool,
    ) -> Result<bool> {
        self.trader
            .sell_token(token_mint, token_amount, creator_override, is_cashback)
            .await
    }
}
