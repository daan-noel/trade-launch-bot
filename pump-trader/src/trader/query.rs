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
use std::collections::HashMap;
use std::str::FromStr;

impl PumpFunTrader {
    /// Return all non-zero token accounts held by the trader's wallet.
    /// Uses raw JSON-RPC via reqwest to avoid extra Solana SDK dependencies.
    pub async fn get_all_token_accounts(
        &self,
    ) -> anyhow::Result<Vec<crate::types::WalletHolding>> {
        let wallet = self.wallet_pubkey();
        let rpc_url = self.rpc_url();
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

            // Reuse the trader's shared HTTP client (connection pool / TLS reuse)
            // instead of constructing a fresh `reqwest::Client` per wallet scan.
            let resp: serde_json::Value = self
                .http
                .post(rpc_url)
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

    /// Resolve the wallet's token account for `mint` with at most one on-chain
    /// lookup. Serves from the in-memory cache when the account is already known
    /// (populated by a prior buy/sell on this trader), otherwise performs a
    /// single `get_all_token_accounts` scan and caches the result. Returns
    /// `None` when the wallet holds no account for the mint.
    ///
    /// Hot-path callers (e.g. the TPSL sell retry loop) use this so repeated
    /// attempts don't each re-scan the entire wallet — the lookup happens once
    /// and every later attempt hits the cache (zero RPC).
    pub async fn resolve_cached_token_account(
        &self,
        mint: &str,
    ) -> anyhow::Result<Option<Pubkey>> {
        if let Some(pk) = self.user_token_accounts.lock().unwrap().get(mint).copied() {
            return Ok(Some(pk));
        }
        // The scan already returns every holding in the wallet, so cache all of
        // them in one pass instead of keeping only `mint`. A later lookup for
        // any other held mint then hits the cache (zero RPC) rather than
        // re-scanning the whole wallet. On-chain is the source of truth, so we
        // overwrite any existing entries with the freshly-read accounts.
        let holdings = self.get_all_token_accounts().await?;
        let mut cache = self.user_token_accounts.lock().unwrap();
        let mut resolved = None;
        for h in &holdings {
            // A malformed token_account from a valid RPC response shouldn't
            // happen; skip rather than fail the whole resolve if it does.
            let Ok(pk) = Pubkey::from_str(&h.token_account) else {
                continue;
            };
            if h.mint == mint {
                resolved = Some(pk);
            }
            cache.insert(h.mint.clone(), pk);
        }
        Ok(resolved)
    }

    /// Read the bonding curve's virtual reserves — `(virtual_token, virtual_quote)`
    /// in raw units (quote = lamports) — for slippage quoting on the curve path.
    /// Layout after the 8-byte Anchor discriminator: `virtual_token_reserves` @8,
    /// `virtual_quote_reserves` @16 (both u64 LE).
    pub(crate) async fn curve_virtual_reserves(
        &self,
        bonding_curve: &Pubkey,
    ) -> anyhow::Result<(u128, u128)> {
        let acct = self
            .rpc
            .get_account(bonding_curve)
            .await
            .map_err(|e| anyhow::anyhow!("read bonding curve {bonding_curve}: {e}"))?;
        let d = &acct.data;
        if d.len() < 24 {
            anyhow::bail!("bonding curve account too short: {} bytes", d.len());
        }
        let vt = u64::from_le_bytes(d[8..16].try_into().unwrap()) as u128;
        let vq = u64::from_le_bytes(d[16..24].try_into().unwrap()) as u128;
        if vt == 0 || vq == 0 {
            anyhow::bail!("bonding curve has zero virtual reserves");
        }
        Ok((vt, vq))
    }

    /// Curve virtual reserves with a WS-cache fast path: serve a fresh cached
    /// snapshot for `mint` (same `(virtual_token, virtual_quote=lamports)` units
    /// as [`curve_virtual_reserves`]) when one is available, otherwise read the
    /// bonding-curve account on-chain. Used for curve buy/sell slippage quoting.
    pub(crate) async fn curve_reserves(
        &self,
        mint: &str,
        bonding_curve: &Pubkey,
    ) -> anyhow::Result<(u128, u128)> {
        if let Some(r) = self.reserve_cache.get_fresh(
            mint,
            std::time::Duration::from_millis(crate::constants::RESERVE_CACHE_MAX_AGE_MS),
            false,
        ) {
            return Ok(r);
        }
        self.curve_virtual_reserves(bonding_curve).await
    }

    /// Query the on-chain SPL token balance for a given wallet + mint.
    /// Tries both classic Token and Token-2022 ATAs; returns 0 if none found.
    pub async fn get_token_balance(
        &self,
        wallet: &str,
        mint: &str,
    ) -> anyhow::Result<crate::types::TokenBalance> {
        // FIX: don't derive ATA — look up actual on-chain account. Serve from
        // the cache, or do a single wallet scan (which also warms the cache for
        // every other held mint).
        let token_account_pk = match self.resolve_cached_token_account(mint).await? {
            Some(pk) => pk,
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
        };

        match self.rpc.get_token_account_balance(&token_account_pk).await {
            Ok(ui_amount) => {
                let amount: u64 = ui_amount.amount.parse().unwrap_or(0);
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

    /// Resolve everything a manual buy needs straight from chain (source of
    /// truth, so it handles freshly-migrated and Token-2022 tokens the local
    /// cache may not know about yet). Reads the bonding-curve PDA for the
    /// creator and the `complete` flag, and the mint account owner for the
    /// token program.
    ///
    /// BondingCurve layout: 8-byte discriminator, 5×u64 reserves/supply, then
    /// `complete: bool` at offset 48 and `creator: Pubkey` at offset 49 — the
    /// same account [`get_creator_from_mint_pda`] reads.
    pub async fn resolve_buy_routing(
        &self,
        mint_address: &str,
    ) -> anyhow::Result<crate::types::BuyRouting> {
        let rpc = &self.rpc;
        let program_id = Pubkey::from_str(PUMP_FUN_PROGRAM_ID)?;
        let mint = Pubkey::from_str(mint_address)?;
        let (bonding_curve, _) =
            Pubkey::find_program_address(&[b"bonding-curve", mint.as_ref()], &program_id);

        // Both independent accounts in one request (getMultipleAccounts) — this
        // gates every manual buy/sell.
        let accounts = rpc.get_multiple_accounts(&[bonding_curve, mint]).await?;
        let [bonding_acct, mint_acct]: [Option<_>; 2] = accounts
            .try_into()
            .map_err(|_| anyhow::anyhow!("getMultipleAccounts returned an unexpected count"))?;
        let account =
            bonding_acct.ok_or_else(|| anyhow::anyhow!("bonding curve account not found"))?;
        let mint_account =
            mint_acct.ok_or_else(|| anyhow::anyhow!("mint account not found"))?;
        const COMPLETE_OFFSET: usize = 48;
        const CREATOR_OFFSET: usize = 49;
        if account.data.len() < CREATOR_OFFSET + 32 {
            anyhow::bail!("Bonding curve account data too short");
        }
        let is_migrated = account.data[COMPLETE_OFFSET] != 0;
        let creator_bytes: [u8; 32] =
            account.data[CREATOR_OFFSET..CREATOR_OFFSET + 32].try_into()?;
        let creator = Pubkey::from(creator_bytes).to_string();
        let token_program_id = mint_account.owner.to_string();

        Ok(crate::types::BuyRouting {
            creator,
            token_program_id,
            is_migrated,
        })
    }

    /// Batch-resolve on-chain migration status for many mints in as few RPC
    /// round-trips as possible. Reads each mint's bonding-curve PDA and inspects
    /// the `complete` byte at offset 48 — the same source of truth the trade
    /// path uses in [`resolve_buy_routing`], but via `getMultipleAccounts` so a
    /// whole wallet costs one request per 100 mints instead of one per mint.
    ///
    /// Returns a map of mint -> is_migrated holding only the mints whose bonding
    /// curve account exists and is long enough to read. Mints absent from the
    /// map could not be resolved (not a pump.fun bonding-curve token, account
    /// missing, or the RPC chunk errored); the caller decides how to treat that
    /// "unknown" — e.g. fall back to a cached value.
    pub async fn resolve_migrated_batch(&self, mints: &[String]) -> HashMap<String, bool> {
        const COMPLETE_OFFSET: usize = 48;
        let program_id = match Pubkey::from_str(PUMP_FUN_PROGRAM_ID) {
            Ok(p) => p,
            Err(_) => return HashMap::new(),
        };

        // Derive the bonding-curve PDA for every mint we can parse, keeping the
        // mint string alongside it so results map back to the caller's keys.
        let derived: Vec<(String, Pubkey)> = mints
            .iter()
            .filter_map(|m| {
                let mint = Pubkey::from_str(m).ok()?;
                let (bonding_curve, _) =
                    Pubkey::find_program_address(&[b"bonding-curve", mint.as_ref()], &program_id);
                Some((m.clone(), bonding_curve))
            })
            .collect();

        let mut out = HashMap::new();
        // getMultipleAccounts is capped at 100 accounts per request.
        for chunk in derived.chunks(100) {
            let pubkeys: Vec<Pubkey> = chunk.iter().map(|(_, pda)| *pda).collect();
            let accounts = match self.rpc.get_multiple_accounts(&pubkeys).await {
                Ok(a) => a,
                Err(e) => {
                    tracing::warn!("resolve_migrated_batch: get_multiple_accounts failed: {e}");
                    continue;
                }
            };
            for ((mint, _), account) in chunk.iter().zip(accounts) {
                if let Some(acct) = account {
                    if acct.data.len() > COMPLETE_OFFSET {
                        out.insert(mint.clone(), acct.data[COMPLETE_OFFSET] != 0);
                    }
                }
            }
        }
        out
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

        // Fetch the bonding-curve and mint accounts in one request
        // (getMultipleAccounts) — independent reads, so a single round-trip.
        let accounts = rpc.get_multiple_accounts(&[bonding_curve, mint]).await?;
        let [bonding_acct, mint_acct]: [Option<_>; 2] = accounts
            .try_into()
            .map_err(|_| anyhow::anyhow!("getMultipleAccounts returned an unexpected count"))?;
        let account =
            bonding_acct.ok_or_else(|| anyhow::anyhow!("bonding curve account not found"))?;
        let mint_account =
            mint_acct.ok_or_else(|| anyhow::anyhow!("mint account not found"))?;

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
        self.token_pdas.lock().unwrap().insert(
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
