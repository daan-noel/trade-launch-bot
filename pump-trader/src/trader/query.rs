// ============================================================
// Read-only queries — NOT on the trade hot path.
//
//  - get_all_token_accounts:   list all non-zero wallet holdings (raw JSON-RPC).
//  - get_token_balance:        on-chain SPL balance for a wallet + mint.
//  - get_creator_from_mint_pda: read the bonding-curve PDA to find the creator,
//                               and cache all sell-side PDAs for the mint.
// ============================================================

use super::{PumpFunTrader, TokenPDAs};
use crate::constants::{PUMP_FUN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID, TOKEN_PROGRAM_ID};
use solana_sdk::pubkey::Pubkey;
use spl_associated_token_account::get_associated_token_address_with_program_id;
use std::str::FromStr;

impl PumpFunTrader {
    /// Return all non-zero token accounts held by the trader's wallet.
    /// Uses raw JSON-RPC via reqwest to avoid extra Solana SDK dependencies.
    pub async fn get_all_token_accounts(
        &self,
    ) -> anyhow::Result<Vec<crate::types::WalletHolding>> {
        let wallet = self.wallet_pubkey();
        let rpc_url = self.rpc_url().to_string();
        let client = reqwest::Client::new();
        let mut holdings: Vec<crate::types::WalletHolding> = Vec::new();

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

            let Some(accounts) = resp["result"]["value"].as_array() else {
                continue;
            };

            for account in accounts {
                let info = &account["account"]["data"]["parsed"]["info"];
                let mint = match info["mint"].as_str() {
                    Some(m) if !m.is_empty() => m.to_string(),
                    _ => continue,
                };
                let ta = &info["tokenAmount"];
                let amount: u64 = ta["amount"].as_str().unwrap_or("0").parse().unwrap_or(0);
                if amount == 0 {
                    continue;
                }
                let ui_amount = ta["uiAmount"].as_f64().unwrap_or(0.0);
                let decimals = ta["decimals"].as_u64().unwrap_or(0) as u8;
                let token_account = account["pubkey"].as_str().unwrap_or("").to_string();

                holdings.push(crate::types::WalletHolding {
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
            b.ui_amount
                .partial_cmp(&a.ui_amount)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(holdings)
    }

    /// Query the on-chain SPL token balance for a given wallet + mint.
    /// Tries both classic Token and Token-2022 ATAs; returns 0 if none found.
    pub async fn get_token_balance(
        &self,
        wallet: &str,
        mint: &str,
    ) -> anyhow::Result<crate::types::TokenBalance> {
        // FIX: don't derive ATA — look up actual on-chain account
        // Check cache first
        let cached = self.user_token_accounts.lock().await.get(mint).copied();

        let token_account_pk = match cached {
            Some(pk) => pk,
            None => {
                let holdings = self.get_all_token_accounts().await?;
                match holdings.iter().find(|h| h.mint == mint) {
                    Some(h) => Pubkey::from_str(&h.token_account)?,
                    None => {
                        // Truly not found
                        return Ok(crate::types::TokenBalance {
                            mint: mint.to_string(),
                            wallet: wallet.to_string(),
                            amount: 0,
                            ui_amount: 0.0,
                            decimals: 0,
                            token_account: None,
                            token_program_id: String::new(),
                        });
                    }
                }
            }
        };

        match self.rpc.get_token_account_balance(&token_account_pk).await {
            Ok(ui_amount) => {
                let amount: u64 = ui_amount.amount.parse().unwrap_or(0);
                // Also cache it for future use
                self.user_token_accounts
                    .lock()
                    .await
                    .insert(mint.to_string(), token_account_pk);
                Ok(crate::types::TokenBalance {
                    mint: mint.to_string(),
                    wallet: wallet.to_string(),
                    amount,
                    ui_amount: ui_amount.ui_amount.unwrap_or(0.0),
                    decimals: ui_amount.decimals,
                    token_account: Some(token_account_pk.to_string()),
                    token_program_id: String::new(),
                })
            }
            Err(e) => anyhow::bail!("Failed to get token balance: {e}"),
        }
    }

    /// Utility to fetch the creator pubkey for a given mint by reading the bonding curve PDA account.
    /// Returns the creator as a String.
    pub async fn get_creator_from_mint_pda(&self, mint_address: &str) -> anyhow::Result<String> {
        let rpc = &self.rpc;

        // Get bonding curve PDA
        let program_id = Pubkey::from_str(PUMP_FUN_PROGRAM_ID)?;
        let mint = Pubkey::from_str(mint_address)?;
        let (bonding_curve, _) =
            Pubkey::find_program_address(&[b"bonding-curve", mint.as_ref()], &program_id);

        // Fetch account data
        let account = rpc.get_account(&bonding_curve).await?;

        // Parse creator from account data
        const CREATOR_OFFSET: usize = 49;
        if account.data.len() < CREATOR_OFFSET + 32 {
            anyhow::bail!("Account data too short");
        }
        let creator_bytes: [u8; 32] =
            account.data[CREATOR_OFFSET..CREATOR_OFFSET + 32].try_into()?;
        let creator = Pubkey::from(creator_bytes);

        // Derive all PDAs needed for sell
        let (bonding_curve_v2, _) =
            Pubkey::find_program_address(&[b"bonding-curve-v2", mint.as_ref()], &program_id);
        let mint_account = rpc.get_account(&mint).await?;
        let token_program = mint_account.owner;

        let assoc_bonding_curve = get_associated_token_address_with_program_id(
            &bonding_curve, // owner = bonding curve PDA
            &mint,
            &token_program,
        );

        let (creator_vault, _) =
            Pubkey::find_program_address(&[b"creator-vault", creator.as_ref()], &program_id);

        let cashback_enabled = account.data.len() > 82 && account.data[82] != 0;

        // Cache PDAs for this mint
        self.token_pdas.lock().await.insert(
            mint_address.to_string(),
            TokenPDAs {
                token_program,
                bonding_curve,
                bonding_curve_v2,
                associated_bonding_curve: assoc_bonding_curve,
                creator_vault,
                cashback_enabled,
            },
        );

        Ok(creator.to_string())
    }
}
