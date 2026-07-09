// ============================================================
// Create — pump.fun token creation (legacy `create` + `create_v2`).
//
// `create_token_v2` is the current default path (Token-2022 mint). The mint
// keypair is an additional signer alongside the dev wallet. Dev-buy in the same
// tx reuses `build_curve_buy_ixs` with the fresh-curve initial reserves.
// ============================================================

use super::buy::compute_curve_buy_min_out;
use super::PumpFunTrader;
use crate::error::{Context, Result};
use crate::protocol::{self, LAMPORTS_PER_SOL};
use crate::types::{CreateTokenArgs, CreateTokenV2Args, TokenProgram};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_program,
    sysvar,
};
use spl_associated_token_account::{
    get_associated_token_address_with_program_id,
    instruction::create_associated_token_account_idempotent,
};
use std::time::Instant;
use tracing::info;

/// PDAs and derived accounts for a not-yet-created mint.
#[derive(Debug, Clone, Copy)]
pub(super) struct CreateAccounts {
    pub mint_authority: Pubkey,
    pub bonding_curve: Pubkey,
    pub associated_bonding_curve: Pubkey,
    pub global: Pubkey,
    pub metadata: Option<Pubkey>,
    pub mayhem_global_params: Option<Pubkey>,
    pub mayhem_sol_vault: Option<Pubkey>,
    pub mayhem_state: Option<Pubkey>,
    pub mayhem_token_vault: Option<Pubkey>,
}

impl PumpFunTrader {
    /// Legacy SPL-Token `create` (Metaplex metadata CPI). Returns the tx signature.
    pub async fn create_token(
        &self,
        mint: &Keypair,
        args: &CreateTokenArgs,
        confirm: bool,
    ) -> Result<String> {
        self.create_token_inner(mint, args, None, confirm).await
    }

    /// Legacy `create` followed by a dev-wallet buy in the **same** transaction.
    pub async fn create_token_and_dev_buy(
        &self,
        mint: &Keypair,
        args: &CreateTokenArgs,
        dev_buy_sol: f64,
        slippage_bps: Option<u64>,
        confirm: bool,
    ) -> Result<String> {
        let buy_lamports = (dev_buy_sol * LAMPORTS_PER_SOL as f64) as u64;
        self.create_token_inner(mint, args, Some((dev_buy_sol, buy_lamports, slippage_bps)), confirm)
            .await
    }

    /// Token-2022 `create_v2` — the current pump.fun default. Returns the tx signature.
    pub async fn create_token_v2(
        &self,
        mint: &Keypair,
        args: &CreateTokenV2Args,
        confirm: bool,
    ) -> Result<String> {
        self.create_token_v2_inner(mint, args, None, confirm).await
    }

    /// `create_v2` followed by a dev-wallet buy in the **same** transaction.
    pub async fn create_token_v2_and_dev_buy(
        &self,
        mint: &Keypair,
        args: &CreateTokenV2Args,
        dev_buy_sol: f64,
        slippage_bps: Option<u64>,
        confirm: bool,
    ) -> Result<String> {
        let buy_lamports = (dev_buy_sol * LAMPORTS_PER_SOL as f64) as u64;
        self.create_token_v2_inner(mint, args, Some((dev_buy_sol, buy_lamports, slippage_bps)), confirm)
            .await
    }

    async fn create_token_inner(
        &self,
        mint: &Keypair,
        args: &CreateTokenArgs,
        dev_buy: Option<(f64, u64, Option<u64>)>,
        confirm: bool,
    ) -> Result<String> {
        let t0 = Instant::now();
        let wallet = self.config.signer.as_ref();
        let mint_pk = mint.pubkey();
        let token_program = TokenProgram::Legacy;
        let accounts = derive_create_accounts(&mint_pk, token_program, false);
        let mut ixs = self.cu_ixs_for_create(dev_buy.is_some());
        ixs.push(build_create_ix(&mint_pk, wallet.pubkey(), args, &accounts)?);
        self.append_dev_buy_ixs(
            &mut ixs,
            &mint_pk,
            args.creator,
            token_program,
            args.creator == wallet.pubkey(),
            dev_buy,
        )?;
        ixs.push(self.jito_tip_ix(0));
        self.send_create_tx(ixs, wallet, mint, confirm, &mint_pk, args.creator, token_program, args.creator == wallet.pubkey(), false, t0)
            .await
    }

    async fn create_token_v2_inner(
        &self,
        mint: &Keypair,
        args: &CreateTokenV2Args,
        dev_buy: Option<(f64, u64, Option<u64>)>,
        confirm: bool,
    ) -> Result<String> {
        let t0 = Instant::now();
        let wallet = self.config.signer.as_ref();
        let mint_pk = mint.pubkey();
        let token_program = TokenProgram::Token2022;
        let accounts = derive_create_accounts(&mint_pk, token_program, true);
        let mut ixs = self.cu_ixs_for_create(dev_buy.is_some());
        ixs.push(build_create_v2_ix(&mint_pk, wallet.pubkey(), args, &accounts)?);
        self.append_dev_buy_ixs(
            &mut ixs,
            &mint_pk,
            args.creator,
            token_program,
            args.cashback_enabled,
            dev_buy,
        )?;
        ixs.push(self.jito_tip_ix(0));
        self.send_create_tx(
            ixs,
            wallet,
            mint,
            confirm,
            &mint_pk,
            args.creator,
            token_program,
            args.cashback_enabled,
            args.is_mayhem_mode,
            t0,
        )
        .await
    }

    fn cu_ixs_for_create(&self, with_dev_buy: bool) -> Vec<Instruction> {
        if with_dev_buy {
            let compute = &self.config.compute;
            let price_ix = solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_price(
                compute.price_micro_lamports,
            );
            vec![
                solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(
                    compute.curve_create_buy_cu,
                ),
                price_ix,
            ]
        } else {
            self.engine.cu_ixs_curve_create.clone()
        }
    }

    fn append_dev_buy_ixs(
        &self,
        ixs: &mut Vec<Instruction>,
        mint: &Pubkey,
        creator: Pubkey,
        token_program: TokenProgram,
        cashback_enabled: bool,
        dev_buy: Option<(f64, u64, Option<u64>)>,
    ) -> Result<()> {
        let Some((dev_buy_sol, buy_lamports, slippage_bps)) = dev_buy else {
            return Ok(());
        };
        if buy_lamports == 0 {
            return Ok(());
        }
        let wallet = self.config.signer.pubkey();
        let token_program_pk = token_program.pubkey();
        let user_ata =
            get_associated_token_address_with_program_id(&wallet, mint, &token_program_pk);
        ixs.push(create_associated_token_account_idempotent(
            &wallet,
            &wallet,
            mint,
            &token_program_pk,
        ));
        let pdas = self.derive_token_pdas(mint, &creator, &token_program_pk, cashback_enabled);
        // The curve is created in THIS tx, so it has no on-chain state to read —
        // its live reserves are the protocol-constant fresh-curve reserves. Feed
        // those through the SAME shared `curve_buy_min_out` every other buy uses
        // (no hand-inlined reserve tuple that could drift). `slippage_bps = None`
        // keeps the historical unprotected launch behaviour (min_out = 1).
        let reserves = Some(crate::price::fresh_curve_reserves());
        let min_out = compute_curve_buy_min_out(
            buy_lamports,
            slippage_bps,
            reserves,
            self.config.slippage.curve_fee_buffer_bps,
        );
        let buy_ixs = self.build_curve_buy_ixs(
            mint,
            &pdas,
            &user_ata,
            Vec::new(),
            buy_lamports,
            min_out,
        )?;
        // `build_curve_buy_ixs` includes its own CU budget + tip — strip those;
        // this tx already has a single CU block and one tip at the end.
        let buy_only = buy_ixs
            .into_iter()
            .filter(|ix| ix.program_id != solana_sdk::compute_budget::id())
            .filter(|ix| !self.is_jito_tip_ix(ix))
            .collect::<Vec<_>>();
        ixs.extend(buy_only);
        info!(
            mint = %mint,
            dev_buy_sol,
            buy_lamports,
            min_out,
            "create tx includes dev-buy leg"
        );
        Ok(())
    }

    fn is_jito_tip_ix(&self, ix: &Instruction) -> bool {
        ix.program_id == system_program::id()
            && ix.accounts.len() == 2
            && ix.accounts[0].pubkey == self.config.signer.pubkey()
            && protocol::JITO_TIP_ACCOUNTS.contains(&ix.accounts[1].pubkey)
    }

    async fn send_create_tx(
        &self,
        ixs: Vec<Instruction>,
        wallet: &(dyn Signer + Send + Sync),
        mint: &Keypair,
        confirm: bool,
        mint_pk: &Pubkey,
        creator: Pubkey,
        token_program: TokenProgram,
        cashback_enabled: bool,
        is_mayhem_mode: bool,
        t0: Instant,
    ) -> Result<String> {
        let tx = self
            .build_recent_tx_multi(ixs, &[wallet, mint])
            .await
            .context("sign create tx")?;
        let sig = self.send_transaction(&tx).await?;
        info!(
            "🚀 Create submitted — sig: {} | mint: {} | {}ms",
            sig,
            mint_pk,
            t0.elapsed().as_millis()
        );
        if confirm {
            self.confirm_transaction(&sig, self.config.retry.confirm_max_retries)
                .await?;
            info!(
                "✅ Create confirmed — sig: {} | {}ms",
                sig,
                t0.elapsed().as_millis()
            );
        }
        self.warm_post_create_cache(
            mint_pk,
            &creator,
            &token_program.pubkey(),
            cashback_enabled,
        );
        let _ = is_mayhem_mode;
        Ok(sig)
    }
}

pub(super) fn derive_create_accounts(
    mint: &Pubkey,
    token_program: TokenProgram,
    is_v2: bool,
) -> CreateAccounts {
    let token_program_pk = token_program.pubkey();
    let bonding_curve =
        Pubkey::find_program_address(&[b"bonding-curve", mint.as_ref()], &protocol::PUMP_FUN).0;
    let associated_bonding_curve =
        get_associated_token_address_with_program_id(&bonding_curve, mint, &token_program_pk);
    let mint_authority =
        Pubkey::find_program_address(&[b"mint-authority"], &protocol::PUMP_FUN).0;
    let global = Pubkey::find_program_address(&[b"global"], &protocol::PUMP_FUN).0;
    let metadata = if is_v2 {
        None
    } else {
        Some(Pubkey::find_program_address(
            &[
                b"metadata",
                protocol::MPL_TOKEN_METADATA.as_ref(),
                mint.as_ref(),
            ],
            &protocol::MPL_TOKEN_METADATA,
        )
        .0)
    };
    let (mayhem_global_params, mayhem_sol_vault, mayhem_state, mayhem_token_vault) = if is_v2 {
        let global_params =
            Pubkey::find_program_address(&[b"global-params"], &protocol::MAYHEM_PROGRAM).0;
        let sol_vault =
            Pubkey::find_program_address(&[b"sol-vault"], &protocol::MAYHEM_PROGRAM).0;
        let mayhem_state =
            Pubkey::find_program_address(&[b"mayhem-state", mint.as_ref()], &protocol::MAYHEM_PROGRAM)
                .0;
        let mayhem_token_vault =
            get_associated_token_address_with_program_id(&sol_vault, mint, &token_program_pk);
        (
            Some(global_params),
            Some(sol_vault),
            Some(mayhem_state),
            Some(mayhem_token_vault),
        )
    } else {
        (None, None, None, None)
    };
    CreateAccounts {
        mint_authority,
        bonding_curve,
        associated_bonding_curve,
        global,
        metadata,
        mayhem_global_params,
        mayhem_sol_vault,
        mayhem_state,
        mayhem_token_vault,
    }
}

fn build_create_ix(
    mint: &Pubkey,
    user: Pubkey,
    args: &CreateTokenArgs,
    accounts: &CreateAccounts,
) -> Result<Instruction> {
    let metadata = accounts
        .metadata
        .context("legacy create requires Metaplex metadata PDA")?;
    let mut data = Vec::with_capacity(8 + 128);
    data.extend_from_slice(&protocol::CREATE_DISC);
    append_anchor_string(&mut data, &args.name);
    append_anchor_string(&mut data, &args.symbol);
    append_anchor_string(&mut data, &args.uri);
    data.extend_from_slice(args.creator.as_ref());
    Ok(Instruction {
        program_id: protocol::PUMP_FUN,
        accounts: vec![
            AccountMeta::new(*mint, true),
            AccountMeta::new_readonly(accounts.mint_authority, false),
            AccountMeta::new(accounts.bonding_curve, false),
            AccountMeta::new(accounts.associated_bonding_curve, false),
            AccountMeta::new_readonly(accounts.global, false),
            AccountMeta::new_readonly(protocol::MPL_TOKEN_METADATA, false),
            AccountMeta::new(metadata, false),
            AccountMeta::new(user, true),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new_readonly(protocol::TOKEN, false),
            AccountMeta::new_readonly(protocol::ASSOCIATED_TOKEN_PROGRAM, false),
            AccountMeta::new_readonly(sysvar::rent::id(), false),
            AccountMeta::new_readonly(protocol::EVENT_AUTHORITY, false),
            AccountMeta::new_readonly(protocol::PUMP_FUN, false),
        ],
        data,
    })
}

fn build_create_v2_ix(
    mint: &Pubkey,
    user: Pubkey,
    args: &CreateTokenV2Args,
    accounts: &CreateAccounts,
) -> Result<Instruction> {
    let mut data = Vec::with_capacity(8 + 128);
    data.extend_from_slice(&protocol::CREATE_V2_DISC);
    append_anchor_string(&mut data, &args.name);
    append_anchor_string(&mut data, &args.symbol);
    append_anchor_string(&mut data, &args.uri);
    data.extend_from_slice(args.creator.as_ref());
    data.push(u8::from(args.is_mayhem_mode));
    data.push(u8::from(args.cashback_enabled));
    Ok(Instruction {
        program_id: protocol::PUMP_FUN,
        accounts: vec![
            AccountMeta::new(*mint, true),
            AccountMeta::new_readonly(accounts.mint_authority, false),
            AccountMeta::new(accounts.bonding_curve, false),
            AccountMeta::new(accounts.associated_bonding_curve, false),
            AccountMeta::new_readonly(accounts.global, false),
            AccountMeta::new(user, true),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new_readonly(protocol::TOKEN_2022, false),
            AccountMeta::new_readonly(protocol::ASSOCIATED_TOKEN_PROGRAM, false),
            AccountMeta::new(protocol::MAYHEM_PROGRAM, false),
            AccountMeta::new_readonly(
                accounts
                    .mayhem_global_params
                    .context("create_v2 missing mayhem global_params")?,
                false,
            ),
            AccountMeta::new(
                accounts
                    .mayhem_sol_vault
                    .context("create_v2 missing mayhem sol_vault")?,
                false,
            ),
            AccountMeta::new(
                accounts
                    .mayhem_state
                    .context("create_v2 missing mayhem_state")?,
                false,
            ),
            AccountMeta::new(
                accounts
                    .mayhem_token_vault
                    .context("create_v2 missing mayhem_token_vault")?,
                false,
            ),
            AccountMeta::new_readonly(protocol::EVENT_AUTHORITY, false),
            AccountMeta::new_readonly(protocol::PUMP_FUN, false),
        ],
        data,
    })
}

fn append_anchor_string(buf: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    buf.extend_from_slice(bytes);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trader::{PumpFunTrader, TraderConfig};
    use solana_sdk::signature::Keypair;
    use std::sync::Arc;

    fn trader() -> PumpFunTrader {
        PumpFunTrader::new(Arc::new(TraderConfig::new(
            "http://localhost".into(),
            vec!["http://localhost".into()],
            Arc::new(Keypair::new()),
            vec![Pubkey::new_unique()],
        )))
    }

    #[test]
    fn create_v2_ix_has_expected_discriminator_and_account_count() {
        let mint = Keypair::new();
        let user = Pubkey::new_unique();
        let accounts = derive_create_accounts(&mint.pubkey(), TokenProgram::Token2022, true);
        let ix = build_create_v2_ix(
            &mint.pubkey(),
            user,
            &CreateTokenV2Args {
                name: "Test".into(),
                symbol: "TST".into(),
                uri: "https://example.com/meta.json".into(),
                creator: user,
                is_mayhem_mode: false,
                cashback_enabled: true,
            },
            &accounts,
        )
        .unwrap();
        assert_eq!(&ix.data[..8], &protocol::CREATE_V2_DISC);
        assert_eq!(ix.accounts.len(), 16);
        assert_eq!(ix.program_id, protocol::PUMP_FUN);
        assert!(ix.accounts[0].is_signer);
        assert_eq!(ix.accounts[0].pubkey, mint.pubkey());
    }

    #[test]
    fn legacy_create_ix_has_metaplex_accounts() {
        let mint = Keypair::new();
        let user = Pubkey::new_unique();
        let accounts = derive_create_accounts(&mint.pubkey(), TokenProgram::Legacy, false);
        let ix = build_create_ix(
            &mint.pubkey(),
            user,
            &CreateTokenArgs {
                name: "Legacy".into(),
                symbol: "LEG".into(),
                uri: "ipfs://x".into(),
                creator: user,
            },
            &accounts,
        )
        .unwrap();
        assert_eq!(&ix.data[..8], &protocol::CREATE_DISC);
        assert_eq!(ix.accounts.len(), 14);
        assert_eq!(ix.accounts[5].pubkey, protocol::MPL_TOKEN_METADATA);
    }

    #[test]
    fn create_cu_budget_differs_for_dev_buy_combo() {
        let cfg = crate::config::ComputeBudgetCfg::default();
        assert!(
            cfg.curve_create_buy_cu > cfg.curve_create_cu,
            "create+buy CU limit must exceed create-only"
        );
    }
}
