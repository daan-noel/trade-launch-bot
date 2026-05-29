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
    ) -> Result<bool> {
        self.trader
            .sell_token(token_mint, token_amount, creator_override, is_cashback)
            .await
    }

    /// Return all non-zero token accounts held by the trader's wallet.
    /// Uses raw JSON-RPC via reqwest to avoid extra Solana SDK dependencies.
    pub async fn get_all_token_accounts(&self) -> Result<Vec<WalletHolding>> {
        use crate::config::constants::{TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID};

        let wallet = self.wallet_pubkey();
        let rpc_url = self.trader.rpc_url().to_string();
        let client = reqwest::Client::new();
        let mut holdings: Vec<WalletHolding> = Vec::new();

        for prog in [TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID] {
            let body = serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "getTokenAccountsByOwner",
                "params": [
                    wallet,
                    { "programId": prog },
                    { "encoding": "jsonParsed" }
                ]
            });

            let resp: serde_json::Value = client
                .post(&rpc_url)
                .json(&body)
                .send()
                .await?
                .json()
                .await?;

            let Some(accounts) = resp["result"]["value"].as_array() else { continue };

            for account in accounts {
                let info = &account["account"]["data"]["parsed"]["info"];
                let mint = match info["mint"].as_str() {
                    Some(m) if !m.is_empty() => m.to_string(),
                    _ => continue,
                };
                let ta = &info["tokenAmount"];
                let amount: u64 = ta["amount"].as_str().unwrap_or("0").parse().unwrap_or(0);
                if amount == 0 { continue; }
                let ui_amount = ta["uiAmount"].as_f64().unwrap_or(0.0);
                let decimals = ta["decimals"].as_u64().unwrap_or(0) as u8;
                let token_account = account["pubkey"].as_str().unwrap_or("").to_string();

                holdings.push(WalletHolding {
                    mint,
                    amount,
                    ui_amount,
                    decimals,
                    token_account,
                    token_program_id: prog.to_string(),
                });
            }
        }

        holdings.sort_by(|a, b| {
            b.ui_amount.partial_cmp(&a.ui_amount).unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(holdings)
    }

    /// Query the on-chain SPL token balance for a wallet + mint pair.
    /// Runs the blocking RPC call on a thread-pool thread.
    pub async fn get_token_balance(&self, wallet: &str, mint: &str) -> Result<TokenBalance> {
        let trader = self.trader.clone();
        let wallet = wallet.to_string();
        let mint = mint.to_string();
        tokio::task::spawn_blocking(move || {
            trader.get_token_balance_blocking(&wallet, &mint)
        })
        .await?
    }
}
