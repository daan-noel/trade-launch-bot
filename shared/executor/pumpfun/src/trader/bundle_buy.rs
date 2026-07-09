// ============================================================
// Bundle buy — signed curve-buy txs for Jito bundle legs.
//
// Each leg may use a different bundler wallet (not `config.signer`). All legs in
// one bundle share the same recent blockhash. Uses idempotent ATA creation and
// per-leg compute-budget / Jito-tip overrides from the launch composer.
// ============================================================

use super::buy::compute_curve_buy_min_out;
use super::PumpFunTrader;
use crate::error::{Context, Result, TradeError};
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

/// Audited pump.fun curve buy discriminator the launch composer may draw per leg.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BundleBuyVariant {
    Buy,
    BuyExactSolIn,
    BuyV2,
    /// Native-SOL curve: aliases to [`BuyExactSolIn`] or v2 exact-quote when cashback.
    BuyExactQuoteIn,
}

impl BundleBuyVariant {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "buy" => Ok(Self::Buy),
            "buy_exact_sol_in" => Ok(Self::BuyExactSolIn),
            "buy_v2" => Ok(Self::BuyV2),
            "buy_exact_quote_in" => Ok(Self::BuyExactQuoteIn),
            other => Err(TradeError::Other(format!("unknown bundle buy variant: {other}"))),
        }
    }

    fn uses_v2_accounts(self, cashback_enabled: bool) -> bool {
        match self {
            Self::BuyV2 => true,
            Self::BuyExactQuoteIn => cashback_enabled,
            _ => false,
        }
    }
}

/// Per-leg overrides from the launch bundle composer (`bundles.legs[].structure`).
#[derive(Debug, Clone, Copy)]
pub struct BundleLegParams {
    pub slippage_bps: u64,
    pub cu_limit: u32,
    pub cu_price: u64,
    pub tip_lamports: u64,
}

// SSOT: the buy-variant discriminators live in `crate::protocol`; aliased here so
// the per-variant dispatch below reads unchanged. `catalog::tests` guards equality.
const DISC_BUY: [u8; 8] = crate::protocol::BUY_DISC;
const DISC_BUY_EXACT_SOL_IN: [u8; 8] = crate::protocol::BUY_EXACT_SOL_IN_DISC;
const DISC_BUY_V2: [u8; 8] = crate::protocol::BUY_V2_DISC;
const DISC_BUY_EXACT_QUOTE_IN_V2: [u8; 8] = crate::protocol::BUY_EXACT_QUOTE_IN_V2_DISC;

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
        variant: BundleBuyVariant,
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

        let ixs = if variant.uses_v2_accounts(cashback_enabled) {
            self.build_bundle_v2_buy_ixs(
                signer,
                mint,
                creator,
                &pdas,
                &user_token_account,
                account_creation_ixs,
                variant,
                buy_lamports,
                min_tokens_out,
                leg,
            )?
        } else {
            let v1_variant = match variant {
                BundleBuyVariant::BuyExactQuoteIn => BundleBuyVariant::BuyExactSolIn,
                other => other,
            };
            self.build_bundle_v1_curve_buy_ixs(
                signer,
                mint,
                &pdas,
                &user_token_account,
                account_creation_ixs,
                v1_variant,
                buy_lamports,
                min_tokens_out,
                leg,
            )?
        };

        self.build_recent_tx_with_blockhash(ixs, signer, blockhash)
            .await
    }

    /// Recent blockhash for a Jito bundle — reads the warmed cache or fetches once.
    pub async fn fresh_blockhash(&self) -> Result<Hash> {
        use std::time::Duration;
        if let Some(hash) = self.engine.blockhash_cache.get_fresh(Duration::from_millis(
            self.config.cache.blockhash_max_age_ms,
        )) {
            return Ok(hash);
        }
        self.rpc
            .get_latest_blockhash()
            .await
            .map_err(|e| crate::error::TradeError::Other(format!("fetch blockhash: {e}")))
    }

    fn build_bundle_v1_curve_buy_ixs(
        &self,
        signer: &(dyn Signer + Send + Sync),
        mint: &Pubkey,
        pdas: &super::TokenPDAs,
        user_token_account: &Pubkey,
        account_creation_ixs: Vec<Instruction>,
        variant: BundleBuyVariant,
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

        let mut buy_data = Vec::with_capacity(25);
        match variant {
            BundleBuyVariant::Buy => {
                buy_data.extend_from_slice(&DISC_BUY);
                buy_data.extend_from_slice(&min_tokens_out.to_le_bytes());
                buy_data.extend_from_slice(&buy_lamports.to_le_bytes());
            }
            BundleBuyVariant::BuyExactSolIn => {
                buy_data.extend_from_slice(&DISC_BUY_EXACT_SOL_IN);
                buy_data.extend_from_slice(&buy_lamports.to_le_bytes());
                buy_data.extend_from_slice(&min_tokens_out.to_le_bytes());
            }
            _ => {
                return Err(TradeError::Other(
                    "v1 builder called with a v2-only variant".into(),
                ))
            }
        }
        // `OptionBool` track_volume = None
        buy_data.push(0);

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
            &self.engine.jito_tip_account,
            leg.tip_lamports,
        ));

        Ok(ixs)
    }

    fn build_bundle_v2_buy_ixs(
        &self,
        signer: &(dyn Signer + Send + Sync),
        mint: &Pubkey,
        creator: &Pubkey,
        pdas: &super::TokenPDAs,
        user_base_ata: &Pubkey,
        account_creation_ixs: Vec<Instruction>,
        variant: BundleBuyVariant,
        buy_lamports: u64,
        min_tokens_out: u64,
        leg: &BundleLegParams,
    ) -> Result<Vec<Instruction>> {
        let global = self.global_account.as_ref().context("Not initialized")?;
        let quote_mint = protocol::WSOL_MINT;
        let quote_token_program = spl_token::id();
        let ata_program = spl_associated_token_account::id();

        let associated_quote_fee_recipient = get_associated_token_address_with_program_id(
            &global.fee_recipient,
            &quote_mint,
            &quote_token_program,
        );
        let buyback_fee_recipient = protocol::PUMP_AMM_BUYBACK_FEE_RECIPIENT;
        let associated_quote_buyback = get_associated_token_address_with_program_id(
            &buyback_fee_recipient,
            &quote_mint,
            &quote_token_program,
        );
        let associated_base_bonding_curve = get_associated_token_address_with_program_id(
            &pdas.bonding_curve,
            mint,
            &pdas.token_program,
        );
        let associated_quote_bonding_curve = get_associated_token_address_with_program_id(
            &pdas.bonding_curve,
            &quote_mint,
            &quote_token_program,
        );
        let associated_quote_user = get_associated_token_address_with_program_id(
            &signer.pubkey(),
            &quote_mint,
            &quote_token_program,
        );
        let associated_creator_vault = get_associated_token_address_with_program_id(
            &pdas.creator_vault,
            &quote_mint,
            &quote_token_program,
        );
        let (sharing_config, _) =
            Pubkey::find_program_address(&[b"sharing-config", mint.as_ref()], &protocol::PUMP_FUN);
        let (user_volume_accumulator, _) = Pubkey::find_program_address(
            &[b"user_volume_accumulator", signer.pubkey().as_ref()],
            &protocol::PUMP_FUN,
        );
        let associated_user_volume = get_associated_token_address_with_program_id(
            &user_volume_accumulator,
            &quote_mint,
            &quote_token_program,
        );

        let mut ixs = Vec::with_capacity(8);
        ixs.push(ComputeBudgetInstruction::set_compute_unit_limit(leg.cu_limit));
        ixs.push(ComputeBudgetInstruction::set_compute_unit_price(leg.cu_price));
        ixs.extend(account_creation_ixs);
        // v2 buys need the user's WSOL ATA for the quote side.
        ixs.push(create_associated_token_account_idempotent(
            &signer.pubkey(),
            &signer.pubkey(),
            &quote_mint,
            &quote_token_program,
        ));

        let mut buy_data = Vec::with_capacity(24);
        match variant {
            BundleBuyVariant::BuyV2 => {
                buy_data.extend_from_slice(&DISC_BUY_V2);
                buy_data.extend_from_slice(&min_tokens_out.to_le_bytes());
                buy_data.extend_from_slice(&buy_lamports.to_le_bytes());
            }
            BundleBuyVariant::BuyExactQuoteIn => {
                buy_data.extend_from_slice(&DISC_BUY_EXACT_QUOTE_IN_V2);
                buy_data.extend_from_slice(&buy_lamports.to_le_bytes());
                buy_data.extend_from_slice(&min_tokens_out.to_le_bytes());
            }
            _ => {
                return Err(TradeError::Other(
                    "v2 builder called with a v1-only variant".into(),
                ))
            }
        }

        ixs.push(Instruction {
            program_id: protocol::PUMP_FUN,
            accounts: vec![
                AccountMeta::new_readonly(global.global_pda, false),
                AccountMeta::new_readonly(*mint, false),
                AccountMeta::new_readonly(quote_mint, false),
                AccountMeta::new_readonly(pdas.token_program, false),
                AccountMeta::new_readonly(quote_token_program, false),
                AccountMeta::new_readonly(ata_program, false),
                AccountMeta::new(global.fee_recipient, false),
                AccountMeta::new(associated_quote_fee_recipient, false),
                AccountMeta::new(buyback_fee_recipient, false),
                AccountMeta::new(associated_quote_buyback, false),
                AccountMeta::new(pdas.bonding_curve, false),
                AccountMeta::new(associated_base_bonding_curve, false),
                AccountMeta::new(associated_quote_bonding_curve, false),
                AccountMeta::new(signer.pubkey(), true),
                AccountMeta::new(*user_base_ata, false),
                AccountMeta::new(associated_quote_user, false),
                AccountMeta::new(pdas.creator_vault, false),
                AccountMeta::new(associated_creator_vault, false),
                AccountMeta::new_readonly(sharing_config, false),
                AccountMeta::new(global.global_volume_accumulator, false),
                AccountMeta::new(user_volume_accumulator, false),
                AccountMeta::new(associated_user_volume, false),
                AccountMeta::new_readonly(global.fee_config, false),
                AccountMeta::new_readonly(protocol::FEE_PROGRAM, false),
                AccountMeta::new_readonly(system_program::id(), false),
                AccountMeta::new_readonly(protocol::EVENT_AUTHORITY, false),
                AccountMeta::new_readonly(protocol::PUMP_FUN, false),
            ],
            data: buy_data,
        });

        ixs.push(system_instruction::transfer(
            &signer.pubkey(),
            &self.engine.jito_tip_account,
            leg.tip_lamports,
        ));

        // Silence unused `creator` — retained for future sharing-config reads.
        let _ = creator;

        Ok(ixs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variant_parse_covers_composer_surface() {
        assert_eq!(
            BundleBuyVariant::parse("buy").unwrap(),
            BundleBuyVariant::Buy
        );
        assert_eq!(
            BundleBuyVariant::parse("buy_exact_sol_in").unwrap(),
            BundleBuyVariant::BuyExactSolIn
        );
        assert_eq!(
            BundleBuyVariant::parse("buy_v2").unwrap(),
            BundleBuyVariant::BuyV2
        );
        assert_eq!(
            BundleBuyVariant::parse("buy_exact_quote_in").unwrap(),
            BundleBuyVariant::BuyExactQuoteIn
        );
        assert!(BundleBuyVariant::parse("unknown").is_err());
    }

    #[test]
    fn v2_routing_requires_cashback_for_exact_quote() {
        assert!(!BundleBuyVariant::BuyExactQuoteIn.uses_v2_accounts(false));
        assert!(BundleBuyVariant::BuyExactQuoteIn.uses_v2_accounts(true));
        assert!(BundleBuyVariant::BuyV2.uses_v2_accounts(false));
    }
}
