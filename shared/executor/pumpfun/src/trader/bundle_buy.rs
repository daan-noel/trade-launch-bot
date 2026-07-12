// ============================================================
// Bundle buy — signed curve-buy txs for Jito bundle legs.
//
// Each leg may use a different bundler wallet (not `config.signer`). All legs in
// one bundle share the same recent blockhash. Uses idempotent ATA creation and
// per-leg compute-budget / Jito-tip overrides from the launch composer.
// ============================================================

use super::assemble::{assemble, IxParts};
use super::buy::compute_curve_buy_min_out;
use super::PumpFunTrader;
use crate::error::{Context, Result, TradeError};
use crate::protocol;
use crate::types::TokenProgram;
use executor_core::IxLayout;
use solana_sdk::{
    address_lookup_table::AddressLookupTableAccount,
    hash::Hash,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::Signer,
    system_program,
    transaction::VersionedTransaction,
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
/// Carries the leg's ix **layout** (shape): an authored hand-picked step order, or
/// [`IxLayout::canonical_buy`] when none was authored. The `layout` (a `Vec`) is why
/// this is no longer `Copy`; callers build it by value ([`crate::trader`] leg builder).
#[derive(Debug, Clone)]
pub struct BundleLegParams {
    pub slippage_bps: u64,
    pub cu_limit: u32,
    pub cu_price: u64,
    pub tip_lamports: u64,
    pub layout: IxLayout,
}

// SSOT: the buy-variant discriminators live in `crate::protocol`; aliased here so
// the per-variant dispatch below reads unchanged. `catalog::tests` guards equality.
const DISC_BUY: [u8; 8] = crate::protocol::BUY_DISC;
const DISC_BUY_EXACT_SOL_IN: [u8; 8] = crate::protocol::BUY_EXACT_SOL_IN_DISC;
const DISC_BUY_V2: [u8; 8] = crate::protocol::BUY_V2_DISC;
const DISC_BUY_EXACT_QUOTE_IN_V2: [u8; 8] = crate::protocol::BUY_EXACT_QUOTE_IN_V2_DISC;

impl PumpFunTrader {
    /// Build one signed **v0** buy tx for a Jito bundle leg. `signer` is the
    /// bundler wallet (may differ from `TraderConfig.signer`). `blockhash` must
    /// be shared across every tx in the same bundle submission. When a launch ALT
    /// is configured it compresses the leg's immutable accounts — load-bearing for
    /// the ~27-account v2 buy leg, which would otherwise ride the 1232 B limit; the
    /// v1 leg just gains headroom. No ALT → a plain v0 tx (~legacy size).
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
    ) -> Result<VersionedTransaction> {
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

        // Compile as v0 against the launch ALT (empty slice when none configured),
        // sharing the bundle's blockhash so every leg lands in the same block.
        let alts: &[AddressLookupTableAccount] = self
            .launch_alt
            .as_ref()
            .map(std::slice::from_ref)
            .unwrap_or(&[]);
        self.build_v0_tx_with_blockhash(ixs, signer, blockhash, alts)
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

        let buy_ix = Instruction {
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
        };

        // Layout (shape) is drawn separately from the core ix (bytes above): the
        // authored `leg.layout` (or `canonical_buy` when none) orders the wrappers
        // around the opaque `Core = [buy]` block. The v1 buy carries a single base ATA.
        Ok(assemble(
            &leg.layout,
            IxParts {
                core: vec![buy_ix],
                ata: account_creation_ixs,
                cu_limit: leg.cu_limit,
                cu_price: leg.cu_price,
                tip_lamports: leg.tip_lamports,
                tip_account: self.engine.jito_tip_account,
                payer: signer.pubkey(),
            },
        ))
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

        // v2 buys need the user's WSOL ATA for the quote side in addition to the
        // base ATA — the canonical buy's single `CreateAta` step expands to both.
        let mut ata = account_creation_ixs;
        ata.push(create_associated_token_account_idempotent(
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

        let buy_ix = Instruction {
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
        };

        // Silence unused `creator` — retained for future sharing-config reads.
        let _ = creator;

        // Authored `leg.layout` (or `canonical_buy`) orders the wrappers; the v2 buy's
        // `CreateAta` carries both the base and WSOL ATAs, `Core` is `[buy]`.
        Ok(assemble(
            &leg.layout,
            IxParts {
                core: vec![buy_ix],
                ata,
                cu_limit: leg.cu_limit,
                cu_price: leg.cu_price,
                tip_lamports: leg.tip_lamports,
                tip_account: self.engine.jito_tip_account,
                payer: signer.pubkey(),
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trader::{PumpFunTrader, TraderConfig};
    use solana_sdk::signature::Keypair;
    use std::sync::Arc;

    fn trader_with_global() -> (PumpFunTrader, Keypair) {
        let bundler = Keypair::new();
        let mut t = PumpFunTrader::new(Arc::new(TraderConfig::new(
            "http://localhost".into(),
            vec!["http://localhost".into()],
            Arc::new(Keypair::new()),
            vec![Pubkey::new_unique()],
        )));
        let d = |seeds: &[&[u8]], prog: &Pubkey| Pubkey::find_program_address(seeds, prog).0;
        t.global_account = Some(crate::trader::GlobalAccount {
            global_pda: d(&[b"global"], &protocol::PUMP_FUN),
            fee_recipient: Pubkey::new_unique(),
            global_volume_accumulator: d(&[b"global_volume_accumulator"], &protocol::PUMP_FUN),
            user_volume_accumulator: d(&[b"user_volume_accumulator", bundler.pubkey().as_ref()], &protocol::PUMP_FUN),
            fee_config: d(&[b"fee_config", protocol::PUMP_FUN.as_ref()], &protocol::FEE_PROGRAM),
            stable_quote_mint: None,
        });
        (t, bundler)
    }

    /// The v2 bundle-buy leg is the ~27-account one at risk of the 1232 B ceiling.
    /// Prove the launch ALT compresses it: v0+ALT is both smaller than the legacy
    /// message AND under the limit, and actually references the ALT.
    #[test]
    fn bundle_v2_leg_v0_alt_fits_and_beats_legacy() {
        use solana_sdk::address_lookup_table::AddressLookupTableAccount;
        use solana_sdk::hash::Hash;
        use solana_sdk::message::{v0, Message, VersionedMessage};

        let (t, bundler) = trader_with_global();
        let mint = Pubkey::new_unique();
        let creator = Pubkey::new_unique();
        let token_program = TokenProgram::Token2022.pubkey();
        let pdas = t.derive_token_pdas(&mint, &creator, &token_program, true);
        let user_base_ata =
            get_associated_token_address_with_program_id(&bundler.pubkey(), &mint, &token_program);
        let account_creation_ixs = vec![create_associated_token_account_idempotent(
            &bundler.pubkey(),
            &bundler.pubkey(),
            &mint,
            &token_program,
        )];
        let leg = BundleLegParams { slippage_bps: 500, cu_limit: 250_000, cu_price: 750_000, tip_lamports: 200_000, layout: IxLayout::canonical_buy() };
        let ixs = t
            .build_bundle_v2_buy_ixs(
                &bundler, &mint, &creator, &pdas, &user_base_ata,
                account_creation_ixs, BundleBuyVariant::BuyV2, 10_000_000, 1, &leg,
            )
            .unwrap();

        let legacy = {
            let msg = Message::new(&ixs, Some(&bundler.pubkey()));
            1 + 64 * msg.header.num_required_signatures as usize + msg.serialize().len()
        };
        let alt = AddressLookupTableAccount {
            key: Pubkey::new_unique(),
            addresses: crate::alt::launch_alt_addresses(),
        };
        let vmsg = v0::Message::try_compile(&bundler.pubkey(), &ixs, std::slice::from_ref(&alt), Hash::default())
            .expect("compile v0 bundle leg");
        let v0_size = 1
            + 64 * vmsg.header.num_required_signatures as usize
            + VersionedMessage::V0(vmsg.clone()).serialize().len();
        eprintln!("v2 bundle leg: legacy = {legacy} B, v0+ALT = {v0_size} B (limit 1232)");
        assert!(!vmsg.address_table_lookups.is_empty(), "v0 leg did not reference the ALT");
        assert!(v0_size < legacy, "ALT must shrink the leg: v0 {v0_size} >= legacy {legacy}");
        assert!(v0_size <= 1232, "v2 leg over limit even with ALT: {v0_size} B");
    }

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

    // --- Golden byte-identity: `assemble(canonical_buy, …)` reproduces the old
    // hand-pushed `[cu_limit, cu_price, ata.., core, tip]` order exactly. ---
    use solana_sdk::compute_budget::ComputeBudgetInstruction as CBI;
    use solana_sdk::system_instruction;

    #[test]
    fn golden_bundle_v1_leg_shape() {
        let (t, bundler) = trader_with_global();
        let mint = Pubkey::new_unique();
        let creator = Pubkey::new_unique();
        let token_program = TokenProgram::Legacy.pubkey();
        let pdas = t.derive_token_pdas(&mint, &creator, &token_program, false);
        let user_ata =
            get_associated_token_address_with_program_id(&bundler.pubkey(), &mint, &token_program);
        let ata_ix = create_associated_token_account_idempotent(
            &bundler.pubkey(),
            &bundler.pubkey(),
            &mint,
            &token_program,
        );
        let leg = BundleLegParams { slippage_bps: 500, cu_limit: 250_000, cu_price: 750_000, tip_lamports: 200_000, layout: IxLayout::canonical_buy() };
        let out = t
            .build_bundle_v1_curve_buy_ixs(
                &bundler, &mint, &pdas, &user_ata, vec![ata_ix.clone()],
                BundleBuyVariant::Buy, 10_000_000, 1, &leg,
            )
            .unwrap();
        // [cu_limit, cu_price, base_ata, core buy, tip] — 5 ixs, in this order.
        assert_eq!(out.len(), 5);
        assert_eq!(out[0], CBI::set_compute_unit_limit(leg.cu_limit));
        assert_eq!(out[1], CBI::set_compute_unit_price(leg.cu_price));
        assert_eq!(out[2], ata_ix);
        assert_eq!(out[3].program_id, protocol::PUMP_FUN); // opaque Core buy
        assert_eq!(
            out[4],
            system_instruction::transfer(&bundler.pubkey(), &t.engine.jito_tip_account, leg.tip_lamports)
        );
    }

    #[test]
    fn golden_bundle_v2_leg_shape() {
        let (t, bundler) = trader_with_global();
        let mint = Pubkey::new_unique();
        let creator = Pubkey::new_unique();
        let token_program = TokenProgram::Token2022.pubkey();
        let pdas = t.derive_token_pdas(&mint, &creator, &token_program, true);
        let user_base_ata =
            get_associated_token_address_with_program_id(&bundler.pubkey(), &mint, &token_program);
        let base_ata = create_associated_token_account_idempotent(
            &bundler.pubkey(),
            &bundler.pubkey(),
            &mint,
            &token_program,
        );
        let wsol_ata = create_associated_token_account_idempotent(
            &bundler.pubkey(),
            &bundler.pubkey(),
            &protocol::WSOL_MINT,
            &spl_token::id(),
        );
        let leg = BundleLegParams { slippage_bps: 500, cu_limit: 250_000, cu_price: 750_000, tip_lamports: 200_000, layout: IxLayout::canonical_buy() };
        let out = t
            .build_bundle_v2_buy_ixs(
                &bundler, &mint, &creator, &pdas, &user_base_ata, vec![base_ata.clone()],
                BundleBuyVariant::BuyV2, 10_000_000, 1, &leg,
            )
            .unwrap();
        // [cu_limit, cu_price, base_ata, wsol_ata, core buy, tip] — 6 ixs.
        assert_eq!(out.len(), 6);
        assert_eq!(out[0], CBI::set_compute_unit_limit(leg.cu_limit));
        assert_eq!(out[1], CBI::set_compute_unit_price(leg.cu_price));
        assert_eq!(out[2], base_ata);
        assert_eq!(out[3], wsol_ata); // CreateAta expanded to the WSOL ATA too
        assert_eq!(out[4].program_id, protocol::PUMP_FUN); // opaque Core buy
        assert_eq!(
            out[5],
            system_instruction::transfer(&bundler.pubkey(), &t.engine.jito_tip_account, leg.tip_lamports)
        );
    }
}
