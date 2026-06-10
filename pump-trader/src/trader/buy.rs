// ============================================================
// Buy — hot path.
//
// `buy_token` derives the per-token PDAs, assembles the (optional
// account-creation +) buy instructions, sends via the nonce tx path,
// and confirms. On the way out it kicks off background nonce refresh
// and pool replenishment so the next buy starts warm.
// ============================================================

use super::PumpFunTrader;
use crate::constants::{
    CONFIRM_MAX_RETRIES, CURVE_FEE_BUFFER_BPS, LAMPORTS_PER_SOL, TOKEN_PROGRAM_ID,
};
use anyhow::{Context, Result};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::Signer,
};
use spl_associated_token_account::get_associated_token_address_with_program_id;
use std::str::FromStr;
use std::time::Instant;
use tracing::{info, warn};

impl PumpFunTrader {
    pub async fn buy_token(
        &self,
        token_mint: &str,
        creator: &str,
        token_program_id: &str,
        sol_amount: f64,
        slippage_bps: Option<u64>,
    ) -> Result<bool> {
        self.buy_token_inner(token_mint, creator, token_program_id, sol_amount, slippage_bps, false, false)
            .await
            .map(|_sig| true)
    }

    /// Latency-optimized buy for fresh-token snipes. Identical to [`buy_token`]
    /// but skips the ATA-existence RPC round-trip. Safe only when the wallet
    /// provably holds no account for `token_mint` yet — e.g. a token just seen
    /// via the pump.fun create event — because the check would return "missing"
    /// regardless. If that assumption is ever wrong, the only consequence is one
    /// extra create-with-seed token account (a few thousand lamports of rent),
    /// never a failed or misrouted trade.
    ///
    /// Returns the submitted transaction signature *without* blocking on RPC
    /// confirmation — the caller confirms via the WS/DB trade feed and may use
    /// `signature_state` to classify a send that the feed never surfaces.
    pub async fn buy_token_snipe(
        &self,
        token_mint: &str,
        creator: &str,
        token_program_id: &str,
        sol_amount: f64,
        slippage_bps: Option<u64>,
    ) -> Result<String> {
        self.buy_token_inner(token_mint, creator, token_program_id, sol_amount, slippage_bps, true, true)
            .await
    }

    async fn buy_token_inner(
        &self,
        token_mint: &str,
        creator: &str,
        token_program_id: &str,
        sol_amount: f64,
        slippage_bps: Option<u64>,
        skip_ata_check: bool,
        skip_confirm: bool,
    ) -> Result<String> {
        let t0 = Instant::now();
        let buy_lamports = (sol_amount * LAMPORTS_PER_SOL as f64) as u64;
        let keypair = &self.config.keypair;

        let (nonce_pubkey, nonce_hash) = self.acquire_nonce().await?;

        let result: Result<String> = async {
            let global = self.global_account.as_ref().context("Not initialized")?;

            let mint = Pubkey::from_str(token_mint)?;
            let creator_pubkey = Pubkey::from_str(creator)?;
            let token_program = Pubkey::from_str(token_program_id)?;

            // Curve PDAs via the shared derivation (same source of truth as the
            // query path). `Pubkey` is `Copy`, so the locals below are copies and
            // `pdas` is still moved into the cache.
            let pdas = self.derive_token_pdas(&mint, &creator_pubkey, &token_program, false);
            let bonding_curve = pdas.bonding_curve;
            let bonding_curve_v2 = pdas.bonding_curve_v2;
            let assoc_bonding_curve = pdas.associated_bonding_curve;
            let creator_vault = pdas.creator_vault;

            self.token_pdas
                .lock()
                .unwrap()
                .insert(token_mint.to_string(), pdas);

            // Check if ATA exists. On the snipe path the wallet provably holds
            // no account for this just-created mint, so we skip the RPC and go
            // straight to the seed-account (template) path.
            let ata = get_associated_token_address_with_program_id(
                &keypair.pubkey(),
                &mint,
                &token_program,
            );
            let ata_exists = if skip_ata_check {
                false
            } else {
                self.rpc.get_account(&ata).await.is_ok()
            };

            // FIX: acquire template ONCE, use same address for both create ix and buy ix
            let (user_token_account, template_opt) = if ata_exists {
                (ata, None)
            } else {
                let template = self.acquire_buy_template(token_program_id).await?;
                let account = template.user_token_account;
                (account, Some(template))
            };

            // Cache for sell
            self.user_token_accounts
                .lock()
                .unwrap()
                .insert(token_mint.to_string(), user_token_account);

            let mut ixs = Vec::with_capacity(6);
            ixs.extend_from_slice(&self.compute_budget_ixs);

            if let Some(template) = template_opt {
                // FIX: use the same template we already acquired above
                ixs.push(template.create_with_seed_ix);

                let init_ix = if token_program_id == TOKEN_PROGRAM_ID {
                    spl_token::instruction::initialize_account3(
                        &token_program,
                        &user_token_account,
                        &mint,
                        &keypair.pubkey(),
                    )?
                } else {
                    spl_token_2022::instruction::initialize_account3(
                        &token_program,
                        &user_token_account,
                        &mint,
                        &keypair.pubkey(),
                    )?
                };
                ixs.push(init_ix);
            }

            // `buy_exact_sol_in(spendable_quote_in, min_tokens_out)`: slippage
            // floor on tokens received. `None` keeps the legacy min_out=1 (no
            // protection) and skips the reserve read — the latency-critical snipe
            // path. `Some` reads the curve's virtual reserves and sets a
            // conservative lower bound; a failed read falls back to 1 so slippage
            // never blocks a buy.
            let min_tokens_out: u64 = match slippage_bps {
                Some(slip) => match self.curve_reserves(token_mint, &bonding_curve).await {
                    Ok((vt, vq)) => {
                        let net = (buy_lamports as u128)
                            .saturating_mul(10_000 - CURVE_FEE_BUFFER_BPS)
                            / 10_000;
                        let expected = vt.saturating_mul(net) / (vq + net);
                        ((expected * 10_000u128.saturating_sub(slip as u128) / 10_000) as u64).max(1)
                    }
                    Err(e) => {
                        warn!("curve buy slippage: reserve read failed ({e}); using min_out=1");
                        1
                    }
                },
                None => 1,
            };

            let mut buy_data = vec![0x38, 0xfc, 0x74, 0x08, 0x9e, 0xdf, 0xcd, 0x5f];
            buy_data.extend_from_slice(&buy_lamports.to_le_bytes());
            buy_data.extend_from_slice(&min_tokens_out.to_le_bytes());
            ixs.push(Instruction {
                program_id: self.pump_program,
                accounts: vec![
                    AccountMeta::new_readonly(global.global_pda, false),
                    AccountMeta::new(global.fee_recipient, false),
                    AccountMeta::new(mint, false),
                    AccountMeta::new(bonding_curve, false),
                    AccountMeta::new(assoc_bonding_curve, false),
                    AccountMeta::new(user_token_account, false),
                    AccountMeta::new(keypair.pubkey(), true),
                    AccountMeta::new_readonly(self.system_program, false),
                    AccountMeta::new_readonly(token_program, false),
                    AccountMeta::new(creator_vault, false),
                    AccountMeta::new_readonly(self.event_authority, false),
                    AccountMeta::new_readonly(self.pump_program, false),
                    AccountMeta::new(global.global_volume_accumulator, false),
                    AccountMeta::new(global.user_volume_accumulator, false),
                    AccountMeta::new_readonly(global.fee_config, false),
                    AccountMeta::new_readonly(self.fee_program, false),
                    AccountMeta::new_readonly(bonding_curve_v2, false),
                    AccountMeta::new(self.upgrade_fee_recipient, false),
                ],
                data: buy_data,
            });

            if let Some(tip) = &self.jito_tip_ix {
                ixs.push(tip.clone());
            }

            let tx = self.build_nonce_tx(ixs, &nonce_pubkey, nonce_hash, keypair)?;
            let sig = self.send_transaction(&tx).await?;
            info!(
                "📤 Buy sent — sig: {} | SOL: {} | {}ms",
                sig,
                sol_amount,
                t0.elapsed().as_millis()
            );

            if !skip_confirm {
                self.confirm_transaction(&sig, CONFIRM_MAX_RETRIES)
                    .await?;
                info!(
                    "✅ Buy confirmed — sig: {} | {}ms",
                    sig,
                    t0.elapsed().as_millis()
                );
            }
            Ok(sig)
        }
        .await;

        self.schedule_nonce_refresh(nonce_pubkey);
        self.replenish_pool_async(token_program_id);

        result
    }
}
