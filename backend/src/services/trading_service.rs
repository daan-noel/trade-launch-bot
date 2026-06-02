use std::sync::Arc;

use anyhow::Result;
use serde::Serialize;

use crate::trader::PumpFunTrader;

/// One token account entry in the wallet — pure on-chain data.
#[derive(Debug, Clone, Serialize)]
pub struct WalletHolding {
    pub mint: String,
    pub amount: u64,
    pub ui_amount: f64,
    pub decimals: u8,
    pub token_account: String,
    pub token_program_id: String,
}

/// On-chain token balance for a wallet + mint pair.
#[derive(Debug, Clone, Serialize)]
pub struct TokenBalance {
    pub mint: String,
    pub wallet: String,
    /// Raw token units (before decimal scaling).
    pub amount: u64,
    /// Human-readable amount (amount / 10^decimals).
    pub ui_amount: f64,
    pub decimals: u8,
    /// Associated token account address, or None if no account exists.
    pub token_account: Option<String>,
    /// Token program ID that owns this account (SPL or Token-2022).
    pub token_program_id: String,
}

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
        token_account_override: Option<&str>,
    ) -> Result<bool> {
        self.trader
            .sell_token(token_mint, token_amount, creator_override, is_cashback, token_account_override)
            .await
    }

    /// Return all non-zero token accounts held by the trader's wallet.
    pub async fn get_all_token_accounts(&self) -> Result<Vec<WalletHolding>> {
        self.trader.get_all_token_accounts().await
    }

    /// Query the on-chain SPL token balance for a wallet + mint pair.
    /// Runs the blocking RPC call on a thread-pool thread.
    pub async fn get_token_balance(&self, wallet: &str, mint: &str) -> Result<TokenBalance> {
        self.trader.get_token_balance(wallet, mint).await
    }
}
