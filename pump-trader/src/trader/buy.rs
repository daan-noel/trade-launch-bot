// ============================================================
// Buy — hot path.
//
// `buy_token` derives the per-token PDAs, assembles the (optional
// account-creation +) buy instructions, sends via the nonce tx path,
// and confirms. On the way out it kicks off background nonce refresh
// and pool replenishment so the next buy starts warm.
// ============================================================

use super::{PumpFunTrader, TokenPDAs};
use crate::constants::{CONFIRM_MAX_RETRIES, LAMPORTS_PER_SOL, TOKEN_PROGRAM_ID};
use anyhow::{Context, Result};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::Signer,
};
use spl_associated_token_account::get_associated_token_address_with_program_id;
use std::str::FromStr;
use std::time::Instant;
use tracing::info;

impl PumpFunTrader {
    pub async fn buy_token(
        &self,
        token_mint: &str,
        creator: &str,
        token_program_id: &str,
        sol_amount: f64,
    ) -> Result<bool> {
        let t0 = Instant::now();
        let buy_lamports = (sol_amount * LAMPORTS_PER_SOL as f64) as u64;
        let keypair = &self.config.keypair;

        let (nonce_pubkey, nonce_hash) = self.acquire_nonce().await?;

        let result: Result<bool> = async {
            let global = self.global_account.as_ref().context("Not initialized")?;

            let mint = Pubkey::from_str(token_mint)?;
            let creator_pubkey = Pubkey::from_str(creator)?;
            let token_program = Pubkey::from_str(token_program_id)?;

            let (bonding_curve, _) = Pubkey::find_program_address(
                &[b"bonding-curve", mint.as_ref()],
                &self.pump_program,
            );
            let (bonding_curve_v2, _) = Pubkey::find_program_address(
                &[b"bonding-curve-v2", mint.as_ref()],
                &self.pump_program,
            );
            let assoc_bonding_curve =
                get_associated_token_address_with_program_id(&bonding_curve, &mint, &token_program);
            let (creator_vault, _) = Pubkey::find_program_address(
                &[b"creator-vault", creator_pubkey.as_ref()],
                &self.pump_program,
            );

            self.token_pdas.lock().await.insert(
                token_mint.to_string(),
                TokenPDAs {
                    token_program,
                    bonding_curve,
                    bonding_curve_v2,
                    associated_bonding_curve: assoc_bonding_curve,
                    creator_vault,
                    cashback_enabled: false,
                },
            );

            // Check if ATA exists
            let ata = get_associated_token_address_with_program_id(
                &keypair.pubkey(),
                &mint,
                &token_program,
            );
            let ata_exists = self.rpc.get_account(&ata).await.is_ok();

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
                .await
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

            let mut buy_data = vec![0x38, 0xfc, 0x74, 0x08, 0x9e, 0xdf, 0xcd, 0x5f];
            buy_data.extend_from_slice(&buy_lamports.to_le_bytes());
            buy_data.extend_from_slice(&1u64.to_le_bytes());
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

            if let Some(tip) = self.jito_tip_ix.lock().await.clone() {
                ixs.push(tip);
            }

            let tx = self.build_nonce_tx(ixs, &nonce_pubkey, nonce_hash, keypair)?;
            let sig = self.send_transaction(&tx).await?;
            info!(
                "📤 Buy sent — sig: {} | SOL: {} | {}ms",
                sig,
                sol_amount,
                t0.elapsed().as_millis()
            );

            self.confirm_transaction(&sig, CONFIRM_MAX_RETRIES)
                .await?;
            info!(
                "✅ Buy confirmed — sig: {} | {}ms",
                sig,
                t0.elapsed().as_millis()
            );
            Ok(true)
        }
        .await;

        self.schedule_nonce_refresh(nonce_pubkey);
        self.replenish_pool_async(token_program_id);
        self.prebuild_one_template_async(token_program_id);

        result
    }
}
