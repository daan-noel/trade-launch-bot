// ============================================================
// Bundle buy — signed curve-buy txs for Jito bundle legs.
//
// Each leg may use a different bundler wallet (not `config.signer`). All legs in
// one bundle share the same recent blockhash. Uses idempotent ATA creation and
// per-leg compute-budget / Jito-tip overrides from the launch composer.
// ============================================================

use super::buy::compute_curve_buy_min_out;
use super::PumpFunTrader;
use crate::error::{Context, Result};
use crate::protocol;
use crate::types::TokenProgram;
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    hash::Hash,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::Signer,
    system_instruction,
    system_program,
    transaction::Transaction,
};
use spl_associated_token_account::{
    get_associated_token_address_with_program_id,
    instruction::create_associated_token_account_idempotent,
};

/// Per-leg overrides from the launch bundle composer (`bundles.legs[].structure`).
#[derive(Debug, Clone, Copy)]
pub struct BundleLegParams {
    pub slippage_bps: u64,
    pub cu_limit: u32,
    pub cu_price: u64,
    pub tip_lamports: u64,
}

impl PumpFunTrader {
    /// Build one signed legacy buy tx for a Jito bundle leg. `signer` is the
    /// bundler wallet (may differ from `TraderConfig.signer`). `blockhash` must
    /// be shared across every tx in the same bundle submission.
    pub async fn build_bundle_leg_tx(
        &self,
        signer: &(dyn Signer + Send + Sync),
        blockhash: Hash,
        mint: &Pubkey,
        creator: &Pubkey,
        token_program: TokenProgram,
        buy_lamports: u64,
        cashback_enabled: bool,
        leg: &BundleLegParams,
    ) -> Result<Transaction> {
        self.global_account.as_ref().context("Not initialized")?;
        let token_program_pk = token_program.pubkey();
        let mint_str = mint.to_string();
        let pdas = self.derive_token_pdas(mint, creator, &token_program_pk, cashback_enabled);
        self.token_pdas.insert(mint_str.clone(), pdas);

        let user_token_account = get_associated_token_address_with_program_id(
            &signer.pubkey(),
            mint,
            &token_program_pk,
        );
        self.user_token_accounts
            .insert(mint_str.clone(), user_token_account);

        let reserves = self
            .curve_reserves(&mint_str, &pdas.bonding_curve)
            .await?;
        let min_tokens_out = compute_curve_buy_min_out(
            buy_lamports,
            Some(leg.slippage_bps),
            Some(reserves),
            self.config.slippage.curve_fee_buffer_bps,
        );

        let account_creation_ixs = vec![create_associated_token_account_idempotent(
            &signer.pubkey(),
            &signer.pubkey(),
            mint,
            &token_program_pk,
        )];

        let ixs = self.build_bundle_curve_buy_ixs(
            signer,
            mint,
            &pdas,
            &user_token_account,
            account_creation_ixs,
            buy_lamports,
            min_tokens_out,
            leg,
        )?;

        self.build_recent_tx_with_blockhash(ixs, signer, blockhash)
            .await
    }

    fn build_bundle_curve_buy_ixs(
        &self,
        signer: &(dyn Signer + Send + Sync),
        mint: &Pubkey,
        pdas: &super::TokenPDAs,
        user_token_account: &Pubkey,
        account_creation_ixs: Vec<Instruction>,
        buy_lamports: u64,
        min_tokens_out: u64,
        leg: &BundleLegParams,
    ) -> Result<Vec<Instruction>> {
        let global = self.global_account.as_ref().context("Not initialized")?;
        let (user_volume_accumulator, _) = Pubkey::find_program_address(
            &[b"user_volume_accumulator", signer.pubkey().as_ref()],
            &protocol::PUMP_FUN,
        );

        let mut ixs = Vec::with_capacity(6);
        ixs.push(ComputeBudgetInstruction::set_compute_unit_limit(leg.cu_limit));
        ixs.push(ComputeBudgetInstruction::set_compute_unit_price(leg.cu_price));
        ixs.extend(account_creation_ixs);

        let mut buy_data = Vec::with_capacity(24);
        buy_data.extend_from_slice(&[0x38, 0xfc, 0x74, 0x08, 0x9e, 0xdf, 0xcd, 0x5f]);
        buy_data.extend_from_slice(&buy_lamports.to_le_bytes());
        buy_data.extend_from_slice(&min_tokens_out.to_le_bytes());
        ixs.push(Instruction {
            program_id: protocol::PUMP_FUN,
            accounts: vec![
                AccountMeta::new_readonly(global.global_pda, false),
                AccountMeta::new(global.fee_recipient, false),
                AccountMeta::new(*mint, false),
                AccountMeta::new(pdas.bonding_curve, false),
                AccountMeta::new(pdas.associated_bonding_curve, false),
                AccountMeta::new(*user_token_account, false),
                AccountMeta::new(signer.pubkey(), true),
                AccountMeta::new_readonly(system_program::id(), false),
                AccountMeta::new_readonly(pdas.token_program, false),
                AccountMeta::new(pdas.creator_vault, false),
                AccountMeta::new_readonly(protocol::EVENT_AUTHORITY, false),
                AccountMeta::new_readonly(protocol::PUMP_FUN, false),
                AccountMeta::new(global.global_volume_accumulator, false),
                AccountMeta::new(user_volume_accumulator, false),
                AccountMeta::new_readonly(global.fee_config, false),
                AccountMeta::new_readonly(protocol::FEE_PROGRAM, false),
                AccountMeta::new_readonly(pdas.bonding_curve_v2, false),
                AccountMeta::new(protocol::PUMP_CURVE_FEE_RECIPIENT, false),
            ],
            data: buy_data,
        });

        ixs.push(system_instruction::transfer(
            &signer.pubkey(),
            &self.jito_tip_account,
            leg.tip_lamports,
        ));

        Ok(ixs)
    }
}
