// ============================================================
// Create — pump.fun token creation (legacy `create` + `create_v2`).
//
// `create_token_v2` is the current default path (Token-2022 mint). The mint
// keypair is an additional signer alongside the dev wallet. Dev-buy in the same
// tx reuses `build_curve_buy_ixs` with the fresh-curve initial reserves.
// ============================================================

use super::assemble::{assemble, IxParts};
use super::buy::compute_curve_buy_min_out;
use super::PumpFunTrader;
use crate::error::{Context, Result};
use crate::protocol::{self, LAMPORTS_PER_SOL};
use crate::types::{CreateTokenArgs, CreateTokenV2Args, TokenProgram};
use executor_core::IxLayout;
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
        let create_ix = build_create_ix(&mint_pk, wallet.pubkey(), args, &accounts)?;
        let ixs = self.assemble_create_ixs(
            create_ix,
            &mint_pk,
            args.creator,
            token_program,
            args.creator == wallet.pubkey(),
            dev_buy,
        )?;
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
        let create_ix = build_create_v2_ix(&mint_pk, wallet.pubkey(), args, &accounts)?;
        let ixs = self.assemble_create_ixs(
            create_ix,
            &mint_pk,
            args.creator,
            token_program,
            args.cashback_enabled,
            dev_buy,
        )?;
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

    /// Assemble the full create tx instruction list via the canonical create
    /// layout `[cu_limit, cu_price, core, tip]`. `Core` is `[create_ix]`, or the
    /// fused `[create_ix, ata, buy]` when a dev buy is present. The CU limit is
    /// the create-only budget unless a dev buy is requested (`dev_buy.is_some()`),
    /// matching the pre-refactor `cu_ixs_for_create` selection.
    fn assemble_create_ixs(
        &self,
        create_ix: Instruction,
        mint: &Pubkey,
        creator: Pubkey,
        token_program: TokenProgram,
        cashback_enabled: bool,
        dev_buy: Option<(f64, u64, Option<u64>)>,
    ) -> Result<Vec<Instruction>> {
        let mut core = Vec::with_capacity(3);
        core.push(create_ix);
        core.extend(self.dev_buy_core_ixs(mint, creator, token_program, cashback_enabled, dev_buy)?);

        let compute = &self.config.compute;
        let cu_limit = if dev_buy.is_some() {
            compute.curve_create_buy_cu
        } else {
            compute.curve_create_cu
        };
        // Authored `create_layout` (set per launch from the template) orders the
        // CU/tip wrappers around the fused `Core`; absent ⇒ the canonical shape.
        let layout = self
            .create_layout
            .clone()
            .unwrap_or_else(IxLayout::canonical_create);
        Ok(assemble(
            &layout,
            IxParts {
                core,
                ata: Vec::new(), // buyer ATA is fused into `core` for create
                cu_limit,
                cu_price: compute.price_micro_lamports,
                tip_lamports: self.jito_tip_cache.tip_lamports_for_level(0),
                tip_account: self.engine.jito_tip_account,
                payer: self.config.signer.pubkey(),
            },
        ))
    }

    /// The dev-buy block that fuses into create's `Core`: `[ata, buy]`, or empty
    /// when there is no dev buy (or a zero-lamport one). Reuses `curve_buy_ix` —
    /// the SSOT buy ix — so there is no build-then-strip of CU/tip decorations.
    fn dev_buy_core_ixs(
        &self,
        mint: &Pubkey,
        creator: Pubkey,
        token_program: TokenProgram,
        cashback_enabled: bool,
        dev_buy: Option<(f64, u64, Option<u64>)>,
    ) -> Result<Vec<Instruction>> {
        let Some((dev_buy_sol, buy_lamports, slippage_bps)) = dev_buy else {
            return Ok(Vec::new());
        };
        if buy_lamports == 0 {
            return Ok(Vec::new());
        }
        let wallet = self.config.signer.pubkey();
        let token_program_pk = token_program.pubkey();
        let user_ata =
            get_associated_token_address_with_program_id(&wallet, mint, &token_program_pk);
        let ata_ix = create_associated_token_account_idempotent(
            &wallet,
            &wallet,
            mint,
            &token_program_pk,
        );
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
        let buy_ix = self.curve_buy_ix(mint, &pdas, &user_ata, buy_lamports, min_out)?;
        info!(
            mint = %mint,
            dev_buy_sol,
            buy_lamports,
            min_out,
            "create tx includes dev-buy leg"
        );
        Ok(vec![ata_ix, buy_ix])
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
        // With a launch ALT configured, compile a v0 tx that references it: a
        // create_v2 + dev-buy names ~27 accounts and overflows the 1232 B
        // legacy-message limit, but the ~15 immutable ones collapse to 1-byte ALT
        // indexes. Without an ALT, the legacy single-message path is unchanged
        // (correct for create-only and create_v1, which already fit).
        let sig = match &self.launch_alt {
            Some(alt) => {
                let tx = self
                    .build_recent_v0_tx_multi(ixs, &[wallet, mint], std::slice::from_ref(alt))
                    .await
                    .context("build v0 create tx")?;
                self.send_versioned_transaction(&tx).await?
            }
            None => {
                let tx = self
                    .build_recent_tx_multi(ixs, &[wallet, mint])
                    .await
                    .context("sign create tx")?;
                self.send_transaction(&tx).await?
            }
        };
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

    /// Populate the trader's `global_account` with correctly-derived PDAs so the
    /// dev-buy leg builds offline (no RPC), letting the size tests exercise the
    /// REAL create_v2 + dev-buy instruction set.
    fn with_global(mut t: PumpFunTrader) -> PumpFunTrader {
        let wallet = t.config.signer.pubkey();
        let d = |seeds: &[&[u8]], prog: &Pubkey| Pubkey::find_program_address(seeds, prog).0;
        t.global_account = Some(crate::trader::GlobalAccount {
            global_pda: d(&[b"global"], &protocol::PUMP_FUN),
            fee_recipient: Pubkey::new_unique(),
            global_volume_accumulator: d(&[b"global_volume_accumulator"], &protocol::PUMP_FUN),
            user_volume_accumulator: d(&[b"user_volume_accumulator", wallet.as_ref()], &protocol::PUMP_FUN),
            fee_config: d(&[b"fee_config", protocol::PUMP_FUN.as_ref()], &protocol::FEE_PROGRAM),
            stable_quote_mint: None,
        });
        t
    }

    /// The real create_v2 + dev-buy instruction set (2 CU + create_v2 + ATA + curve
    /// buy + tip), built with production-representative identity string lengths.
    fn create_v2_dev_buy_ixs(t: &PumpFunTrader, mint: &Pubkey, wallet: Pubkey) -> Vec<Instruction> {
        let accounts = derive_create_accounts(mint, TokenProgram::Token2022, true);
        let args = CreateTokenV2Args {
            name: "France World Cup".into(), // 16 chars — a real launch name
            symbol: "FWC".into(),
            uri: "ipfs://QmZadt6hjfsXjUSNpi2pm1Ky9Zx8i6r5k39gnZY1C1m5dQ".into(), // 53 B ipfs pointer
            creator: wallet,
            is_mayhem_mode: false,
            cashback_enabled: false,
        };
        let create_ix = build_create_v2_ix(mint, wallet, &args, &accounts).unwrap();
        t.assemble_create_ixs(
            create_ix,
            mint,
            wallet,
            TokenProgram::Token2022,
            false,
            Some((0.02, 20_000_000, None)),
        )
        .unwrap()
    }

    fn wire_size(msg: &solana_sdk::message::Message) -> usize {
        1 + 64 * msg.header.num_required_signatures as usize + msg.serialize().len()
    }

    /// The bug: a legacy single-message create_v2 + dev-buy overflows the 1232 B
    /// tx limit (Helius `/fast` → "base64 encoded too large"). Pins the regression.
    #[test]
    fn create_v2_dev_buy_legacy_overflows_1232() {
        let t = with_global(trader());
        let wallet = t.config.signer.pubkey();
        let mint = Keypair::new();
        let ixs = create_v2_dev_buy_ixs(&t, &mint.pubkey(), wallet);
        let msg = solana_sdk::message::Message::new(&ixs, Some(&wallet));
        let size = wire_size(&msg);
        eprintln!("create_v2 + dev-buy legacy = {size} B (limit 1232)");
        assert!(size > 1232, "expected legacy create_v2+dev-buy > 1232, got {size} B");
    }

    /// The fix: compiling the SAME instruction set as a v0 tx against the launch
    /// ALT (the immutable-account set) brings it comfortably under 1232 B.
    #[test]
    fn create_v2_dev_buy_v0_with_alt_fits_1232() {
        use solana_sdk::address_lookup_table::AddressLookupTableAccount;
        use solana_sdk::hash::Hash;
        use solana_sdk::message::{v0, VersionedMessage};

        let t = with_global(trader());
        let wallet = t.config.signer.pubkey();
        let mint = Keypair::new();
        let ixs = create_v2_dev_buy_ixs(&t, &mint.pubkey(), wallet);

        let alt = AddressLookupTableAccount {
            key: Pubkey::new_unique(),
            addresses: crate::alt::launch_alt_addresses(),
        };
        let msg = v0::Message::try_compile(&wallet, &ixs, std::slice::from_ref(&alt), Hash::default())
            .expect("compile v0 create tx");
        let size = 1
            + 64 * msg.header.num_required_signatures as usize
            + VersionedMessage::V0(msg.clone()).serialize().len();
        eprintln!("create_v2 + dev-buy v0+ALT  = {size} B (limit 1232)");
        assert!(size <= 1232, "expected v0+ALT create_v2+dev-buy <= 1232, got {size} B");
        // The ALT must actually be doing the work — at least the immutable programs
        // + constant PDAs (>= 12) should resolve through it, not inline.
        assert!(
            !msg.address_table_lookups.is_empty(),
            "v0 message did not reference the ALT"
        );
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

    // --- Golden byte-identity: the `assemble()` refactor is a provable no-op. ---
    // Each test reconstructs the pre-refactor hand-pushed sequence and asserts the
    // new builder yields an identical `Vec<Instruction>` (same program ids,
    // accounts, and data, in the same order).

    use solana_sdk::compute_budget::ComputeBudgetInstruction as CBI;

    /// Create-**only** must reproduce exactly `[cu_limit, cu_price, create, tip]`.
    #[test]
    fn golden_create_only_shape() {
        let t = with_global(trader());
        let wallet = t.config.signer.pubkey();
        let mint = Keypair::new();
        let args = CreateTokenV2Args {
            name: "Golden".into(),
            symbol: "GLD".into(),
            uri: "ipfs://x".into(),
            creator: wallet,
            is_mayhem_mode: false,
            cashback_enabled: false,
        };
        let accounts = derive_create_accounts(&mint.pubkey(), TokenProgram::Token2022, true);
        let create_ix = build_create_v2_ix(&mint.pubkey(), wallet, &args, &accounts).unwrap();

        let got = t
            .assemble_create_ixs(create_ix.clone(), &mint.pubkey(), wallet, TokenProgram::Token2022, false, None)
            .unwrap();

        let compute = &t.config.compute;
        let expected = vec![
            CBI::set_compute_unit_limit(compute.curve_create_cu),
            CBI::set_compute_unit_price(compute.price_micro_lamports),
            create_ix,
            t.jito_tip_ix(0),
        ];
        assert_eq!(got, expected);
    }

    /// Create **with** dev buy must reproduce `[cu_limit, cu_price, create, ata, buy, tip]`
    /// — the fused `Core` = `[create, ata, buy]` — with the create-buy CU budget.
    #[test]
    fn golden_create_dev_buy_shape() {
        let t = with_global(trader());
        let wallet = t.config.signer.pubkey();
        let mint = Keypair::new();
        let args = CreateTokenV2Args {
            name: "Golden".into(),
            symbol: "GLD".into(),
            uri: "ipfs://x".into(),
            creator: wallet,
            is_mayhem_mode: false,
            cashback_enabled: false,
        };
        let accounts = derive_create_accounts(&mint.pubkey(), TokenProgram::Token2022, true);
        let create_ix = build_create_v2_ix(&mint.pubkey(), wallet, &args, &accounts).unwrap();

        let got = t
            .assemble_create_ixs(
                create_ix.clone(),
                &mint.pubkey(),
                wallet,
                TokenProgram::Token2022,
                false,
                Some((0.02, 20_000_000, None)),
            )
            .unwrap();

        // Hand-build the expected dev-buy fused block the way the old builder did.
        let token_program_pk = TokenProgram::Token2022.pubkey();
        let user_ata = get_associated_token_address_with_program_id(&wallet, &mint.pubkey(), &token_program_pk);
        let ata_ix = create_associated_token_account_idempotent(&wallet, &wallet, &mint.pubkey(), &token_program_pk);
        let pdas = t.derive_token_pdas(&mint.pubkey(), &wallet, &token_program_pk, false);
        let min_out = compute_curve_buy_min_out(
            20_000_000,
            None,
            Some(crate::price::fresh_curve_reserves()),
            t.config.slippage.curve_fee_buffer_bps,
        );
        let buy_ix = t.curve_buy_ix(&mint.pubkey(), &pdas, &user_ata, 20_000_000, min_out).unwrap();

        let compute = &t.config.compute;
        let expected = vec![
            CBI::set_compute_unit_limit(compute.curve_create_buy_cu),
            CBI::set_compute_unit_price(compute.price_micro_lamports),
            create_ix,
            ata_ix,
            buy_ix,
            t.jito_tip_ix(0),
        ];
        assert_eq!(got, expected);
    }

    /// Phase C: an authored `create_layout` reshapes the create tx — a lean `[Core]`
    /// layout drops CU + tip, leaving just the create ix (`Core` placed opaque).
    #[test]
    fn create_layout_override_reshapes_the_tx() {
        use executor_core::{DecoStep, IxLayout};
        let mut t = with_global(trader());
        let wallet = t.config.signer.pubkey();
        let mint = Keypair::new();
        let args = CreateTokenV2Args {
            name: "Lean".into(),
            symbol: "LN".into(),
            uri: "ipfs://x".into(),
            creator: wallet,
            is_mayhem_mode: false,
            cashback_enabled: false,
        };
        let accounts = derive_create_accounts(&mint.pubkey(), TokenProgram::Token2022, true);
        let create_ix = build_create_v2_ix(&mint.pubkey(), wallet, &args, &accounts).unwrap();

        // Default: the canonical 4-ix create-only shape.
        let canonical = t
            .assemble_create_ixs(create_ix.clone(), &mint.pubkey(), wallet, TokenProgram::Token2022, false, None)
            .unwrap();
        assert_eq!(canonical.len(), 4);

        // Authored lean `[Core]` → just the create ix.
        t.set_create_layout(Some(IxLayout { steps: vec![DecoStep::Core] }));
        let lean = t
            .assemble_create_ixs(create_ix.clone(), &mint.pubkey(), wallet, TokenProgram::Token2022, false, None)
            .unwrap();
        assert_eq!(lean.len(), 1);
        assert_eq!(lean[0], create_ix, "Core placed opaque, byte-identical");
    }
}
