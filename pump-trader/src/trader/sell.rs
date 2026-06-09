// ============================================================
// Sell — hot path.
//
// `sell_token` is the public entry: it ensures PDAs and the user
// token account are cached (override → cache → on-chain lookup),
// then retries `execute_sell` up to MAX_SELL_ATTEMPTS with a fresh
// nonce each time. `execute_sell` is the single-attempt inner that
// assembles, signs, sends, and confirms one sell tx.
// ============================================================

use super::PumpFunTrader;
use crate::constants::{CONFIRM_MAX_RETRIES, CURVE_FEE_BUFFER_BPS, MAX_SELL_ATTEMPTS};
use anyhow::{Context, Result};
use solana_sdk::{
    hash::Hash,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::Signer,
};
use std::str::FromStr;
use std::time::{Duration, Instant};
use tracing::{error, info, warn};

impl PumpFunTrader {
    pub async fn sell_token(
        &self,
        token_mint: &str,
        token_amount: u64,
        creator_override: Option<&str>,
        is_cashback: bool,
        token_account_override: Option<&str>,
        slippage_bps: Option<u64>,
    ) -> Result<bool> {
        let mut last_err: Option<anyhow::Error> = None;

        for attempt in 0..MAX_SELL_ATTEMPTS {
            // Ensure PDAs are cached
            if !self.token_pdas.lock().await.contains_key(token_mint) {
                if let Err(e) = self.get_creator_from_mint_pda(token_mint).await {
                    last_err = Some(e);
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                }
            }

            // Ensure user token account is cached
            // FIX: fallback chain — override → cache → on-chain lookup
            {
                let cache = self.user_token_accounts.lock().await;
                if !cache.contains_key(token_mint) {
                    let pk = if let Some(token_account) = token_account_override {
                        // Use provided override
                        let pk = match Pubkey::from_str(token_account) {
                            Ok(pk) => pk,
                            Err(e) => {
                                last_err =
                                    Some(anyhow::anyhow!("Invalid token_account_override: {e}"));
                                continue;
                            }
                        };
                        drop(cache); // release lock before re-acquiring for insert
                        pk
                    } else {
                        // FIX: look up actual on-chain account, don't derive ATA
                        drop(cache); // release lock before async call
                        let holdings = match self.get_all_token_accounts().await {
                            Ok(h) => h,
                            Err(e) => {
                                last_err = Some(e);
                                tokio::time::sleep(Duration::from_millis(50)).await;
                                continue;
                            }
                        };
                        match holdings.iter().find(|h| h.mint == token_mint) {
                            Some(h) => match Pubkey::from_str(&h.token_account) {
                                Ok(pk) => pk,
                                Err(e) => {
                                    last_err =
                                        Some(anyhow::anyhow!("Invalid token account pubkey: {e}"));
                                    continue;
                                }
                            },
                            None => {
                                last_err = Some(anyhow::anyhow!(
                                    "No token account found for mint {}",
                                    token_mint
                                ));
                                tokio::time::sleep(Duration::from_millis(50)).await;
                                continue;
                            }
                        }
                    };
                    self.user_token_accounts
                        .lock()
                        .await
                        .insert(token_mint.to_string(), pk);
                }
            }

            let (nonce_pubkey, nonce_hash) = match self.acquire_nonce().await {
                Ok(v) => v,
                Err(e) => {
                    last_err = Some(e);
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                }
            };

            info!(
                "🔁 Sell attempt {}/{} — token: {} nonce: {}",
                attempt + 1,
                MAX_SELL_ATTEMPTS,
                token_mint,
                nonce_pubkey
            );

            let res = self
                .execute_sell(
                    token_mint,
                    token_amount,
                    creator_override,
                    is_cashback,
                    slippage_bps,
                    &nonce_pubkey,
                    nonce_hash,
                )
                .await;

            self.schedule_nonce_refresh(nonce_pubkey);

            match res {
                Ok(true) => return Ok(true),
                Ok(false) => {
                    let e = anyhow::anyhow!("Sell returned false on attempt {}", attempt + 1);
                    error!("❌ {}", e);
                    last_err = Some(e);
                }
                Err(e) => {
                    error!("❌ Sell attempt {} failed: {}", attempt + 1, e);
                    last_err = Some(e);
                }
            }

            if attempt < MAX_SELL_ATTEMPTS - 1 {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }

        Err(last_err.unwrap_or_else(|| {
            anyhow::anyhow!(
                "Sell failed after {} attempts",
                MAX_SELL_ATTEMPTS
            )
        }))
    }

    // -----------------------------------------------------------------------
    // Sell inner — one attempt
    // -----------------------------------------------------------------------

    async fn execute_sell(
        &self,
        token_mint: &str,
        token_amount: u64,
        creator_override: Option<&str>,
        is_cashback: bool,
        slippage_bps: Option<u64>,
        nonce_pubkey: &Pubkey,
        nonce_hash: Hash,
    ) -> Result<bool> {
        let t0 = Instant::now();
        let keypair = &self.config.keypair;
        let global = self.global_account.as_ref().context("Not initialized")?;

        let mint = Pubkey::from_str(token_mint)?;

        // Read cached PDAs (must exist from buy)
        let mut pdas = self
            .token_pdas
            .lock()
            .await
            .get(token_mint)
            .copied()
            .context("Token PDAs not cached — buy must precede sell")?;

        // Allow caller to override creator vault (e.g. after a creator update)
        if let Some(creator_str) = creator_override {
            let creator = Pubkey::from_str(creator_str)?;
            let (vault, _) = Pubkey::find_program_address(
                &[b"creator-vault", creator.as_ref()],
                &self.pump_program,
            );
            if pdas.creator_vault != vault {
                info!(
                    "🔄 Creator vault updated: {} → {}",
                    pdas.creator_vault, vault
                );
                pdas.creator_vault = vault;
            }
        }

        // Read cached user token account (must exist from buy)
        let user_token_account = self
            .user_token_accounts
            .lock()
            .await
            .get(token_mint)
            .copied()
            .context("User token account not cached — buy must precede sell")?;
        warn!(
            user_token_account = user_token_account.to_string(),
            "=== Using cached user token account for sell"
        );
        // ── Assemble instructions ───────────────────────────────────────────
        let mut ixs = Vec::with_capacity(5);
        ixs.extend_from_slice(&self.compute_budget_ixs);

        // `sell(amount, min_sol_output)`: slippage floor on SOL received. `None`
        // keeps the legacy min_out=1 (snipe path, no extra RPC); `Some` reads the
        // curve's virtual reserves for a conservative lower bound, falling back to
        // 1 if the read fails so slippage never blocks a sell.
        let min_sol_output: u64 = match slippage_bps {
            Some(slip) => match self.curve_virtual_reserves(&pdas.bonding_curve).await {
                Ok((vt, vq)) => {
                    let gross =
                        vq.saturating_mul(token_amount as u128) / (vt + token_amount as u128);
                    let net = gross.saturating_mul(10_000 - CURVE_FEE_BUFFER_BPS) / 10_000;
                    ((net * 10_000u128.saturating_sub(slip as u128) / 10_000) as u64).max(1)
                }
                Err(e) => {
                    warn!("curve sell slippage: reserve read failed ({e}); using min_out=1");
                    1
                }
            },
            None => 1,
        };

        // Sell (Sell exact token in)
        let mut sell_data = vec![0x33, 0xe6, 0x85, 0xa4, 0x01, 0x7f, 0x83, 0xad];
        sell_data.extend_from_slice(&token_amount.to_le_bytes());
        sell_data.extend_from_slice(&min_sol_output.to_le_bytes()); // min_sol_output (slippage floor)

        let mut accounts = vec![
            AccountMeta::new_readonly(global.global_pda, false),
            AccountMeta::new(global.fee_recipient, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(pdas.bonding_curve, false),
            AccountMeta::new(pdas.associated_bonding_curve, false),
            AccountMeta::new(user_token_account, false),
            AccountMeta::new(keypair.pubkey(), true),
            AccountMeta::new_readonly(self.system_program, false),
            AccountMeta::new(pdas.creator_vault, false),
            AccountMeta::new_readonly(pdas.token_program, false),
            AccountMeta::new_readonly(self.event_authority, false),
            AccountMeta::new_readonly(self.pump_program, false),
            AccountMeta::new_readonly(global.fee_config, false),
            AccountMeta::new_readonly(self.fee_program, false),
        ];

        // Include the cashback account if the caller knows the token is
        // cashback-enabled, or if the chain-read PDA flag says so. The caller
        // flag matters because `buy_token` caches `cashback_enabled: false`,
        // so a buy→sell flow would otherwise never include this account.
        if is_cashback || pdas.cashback_enabled {
            accounts.push(AccountMeta::new(global.user_volume_accumulator, false));
        }

        accounts.push(AccountMeta::new_readonly(pdas.bonding_curve_v2, false));
        accounts.push(AccountMeta::new(self.upgrade_fee_recipient, false));

        ixs.push(Instruction {
            program_id: self.pump_program,
            accounts,
            data: sell_data,
        });

        // Jito tip
        if let Some(tip) = self.jito_tip_ix.lock().await.clone() {
            ixs.push(tip);
        }

        // ── Sign & send ─────────────────────────────────────────────────────
        let tx = self.build_nonce_tx(ixs, nonce_pubkey, nonce_hash, keypair)?;
        let sig = self.send_transaction(&tx).await?;

        info!(
            "📤 Sell sent — sig: {} | amount: {} | {}ms",
            sig,
            token_amount,
            t0.elapsed().as_millis()
        );

        self.confirm_transaction(&sig, CONFIRM_MAX_RETRIES)
            .await?;

        info!(
            "✅ Sell confirmed — sig: {} | {}ms",
            sig,
            t0.elapsed().as_millis()
        );
        Ok(true)
    }
}
