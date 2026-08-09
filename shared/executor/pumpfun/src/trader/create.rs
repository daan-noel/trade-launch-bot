// ============================================================
// Create — pump.fun token creation (legacy `create` + `create_v2`).
//
// `create_token_v2` is the current default path (Token-2022 mint). The mint
// keypair is an additional signer alongside the dev wallet. Dev-buy in the same
// tx reuses `build_curve_buy_ixs` with the fresh-curve initial reserves.
// ============================================================

use super::assemble::{assemble, IxParts};
use super::bundle_buy::BundleBuyVariant;
use super::buy::compute_curve_buy_min_out;
use super::PumpFunTrader;
use crate::error::{Context, Result, TradeError};
use crate::protocol::{self, LAMPORTS_PER_SOL};
use crate::types::{CreateTokenArgs, CreateTokenV2Args, TokenProgram};
use executor_core::IxLayout;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_program,
    sysvar,
    transaction::VersionedTransaction,
};
use spl_associated_token_account::{
    get_associated_token_address_with_program_id,
    instruction::create_associated_token_account_idempotent,
};
use std::time::Instant;
use tracing::info;

/// The dev-wallet buy fused into a create tx: the spend, its slippage floor, and
/// the curve-buy **encoding**. `variant` is any of the four [`BundleBuyVariant`]s —
/// the same catalog the bundler co-buys draw from — so the dev buy is not pinned
/// to `buy_exact_sol_in`. `sol` is the human-SOL mirror of `lamports` (kept
/// for logging). A tokens-out variant (`buy` / `buy_v2`) needs a `slippage_bps` so
/// `min_out` is a real reserve-derived token floor (else it would buy ~1 token); the
/// launcher validates that before this is built.
#[derive(Debug, Clone, Copy)]
pub struct DevBuy {
    pub sol: f64,
    pub lamports: u64,
    pub slippage_bps: Option<u64>,
    pub variant: BundleBuyVariant,
}

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
    /// `variant` selects the dev-buy curve encoding (see [`DevBuy`]).
    pub async fn create_token_and_dev_buy(
        &self,
        mint: &Keypair,
        args: &CreateTokenArgs,
        dev_buy_sol: f64,
        slippage_bps: Option<u64>,
        variant: BundleBuyVariant,
        confirm: bool,
    ) -> Result<String> {
        let buy_lamports = (dev_buy_sol * LAMPORTS_PER_SOL as f64) as u64;
        let dev_buy = DevBuy { sol: dev_buy_sol, lamports: buy_lamports, slippage_bps, variant };
        self.create_token_inner(mint, args, Some(dev_buy), confirm).await
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
    /// `variant` selects the dev-buy curve encoding (see [`DevBuy`]).
    pub async fn create_token_v2_and_dev_buy(
        &self,
        mint: &Keypair,
        args: &CreateTokenV2Args,
        dev_buy_sol: f64,
        slippage_bps: Option<u64>,
        variant: BundleBuyVariant,
        confirm: bool,
    ) -> Result<String> {
        let buy_lamports = (dev_buy_sol * LAMPORTS_PER_SOL as f64) as u64;
        let dev_buy = DevBuy { sol: dev_buy_sol, lamports: buy_lamports, slippage_bps, variant };
        self.create_token_v2_inner(mint, args, Some(dev_buy), confirm).await
    }

    async fn create_token_inner(
        &self,
        mint: &Keypair,
        args: &CreateTokenArgs,
        dev_buy: Option<DevBuy>,
        confirm: bool,
    ) -> Result<String> {
        let t0 = Instant::now();
        let wallet = self.config.signer.as_ref();
        let mint_pk = mint.pubkey();
        let token_program = TokenProgram::Legacy;
        let accounts = derive_create_accounts(&mint_pk, token_program, false);
        let create_ix = build_create_ix(&mint_pk, wallet.pubkey(), args, &accounts)?;
        // Legacy `create` builds a v1 (SPL-Token) curve, which has NO cashback / v2 buy
        // accounts — so the fused dev-buy must route through the v1 layout. Pass
        // `cashback_enabled = false` (a v1 curve can never be cashback); passing
        // `creator == signer` here mis-fed the dev-buy variant router, steering a
        // `buy_exact_quote_in` dev-buy onto the v2 account layout → ConstraintSeeds.
        let ixs = self.assemble_create_ixs(
            create_ix,
            &mint_pk,
            args.creator,
            token_program,
            false,
            dev_buy,
            0,
        )?;
        self.send_create_tx(ixs, wallet, mint, confirm, &mint_pk, args.creator, token_program, false, false, t0)
            .await
    }

    async fn create_token_v2_inner(
        &self,
        mint: &Keypair,
        args: &CreateTokenV2Args,
        dev_buy: Option<DevBuy>,
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
            0,
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
        dev_buy: Option<DevBuy>,
        tip_level: u8,
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
                // Level 0 for a standalone create send (unchanged); the atomic
                // launch bundle passes the escalation level so the whole-bundle tip
                // rides the create leg and climbs on a re-bid.
                tip_lamports: self.jito_tip_cache.tip_lamports_for_level(tip_level),
                tip_account: self.engine.jito_tip_account,
                payer: self.config.signer.pubkey(),
            },
        ))
    }

    /// Build the `create` (+ fused dev-buy) as an **unsent signed v0 tx** on a
    /// caller-supplied blockhash — the first leg (`tx0`) of an atomic launch Jito
    /// bundle. Unlike [`create_token`], this does NOT submit or confirm: the caller
    /// prepends it to the co-buy legs and `sendBundle`s all of them atomically, so
    /// the create and every sniper buy land in one slot (no front-run gap, no
    /// separate-submission drop). Signed by the dev wallet (`config.signer`, fee
    /// payer) + the `mint` keypair. The whole-bundle Jito tip rides this leg at
    /// `tip_level` (0 first attempt, higher on a re-bid). Compiled v0 over the
    /// launch ALT (empty when none) — required for the ~27-account create_v2+dev-buy.
    pub async fn build_create_leg_tx(
        &self,
        mint_signer: &(dyn Signer + Send + Sync),
        args: &CreateTokenArgs,
        dev_buy: Option<DevBuy>,
        blockhash: solana_sdk::hash::Hash,
        tip_level: u8,
    ) -> Result<VersionedTransaction> {
        let mint_pk = mint_signer.pubkey();
        let token_program = TokenProgram::Legacy;
        let accounts = derive_create_accounts(&mint_pk, token_program, false);
        let create_ix = build_create_ix(&mint_pk, self.config.signer.pubkey(), args, &accounts)?;
        // v1 (legacy) curve — no cashback / v2 buy accounts exist, so the fused dev-buy
        // must use the v1 layout. `false`, not `creator == signer` (see `create_token_inner`).
        let ixs = self.assemble_create_ixs(
            create_ix,
            &mint_pk,
            args.creator,
            token_program,
            false,
            dev_buy,
            tip_level,
        )?;
        self.sign_create_leg(ixs, mint_signer, blockhash).await
    }

    /// Token-2022 [`build_create_leg_tx`] — the current default create_v2 path.
    pub async fn build_create_v2_leg_tx(
        &self,
        mint_signer: &(dyn Signer + Send + Sync),
        args: &CreateTokenV2Args,
        dev_buy: Option<DevBuy>,
        blockhash: solana_sdk::hash::Hash,
        tip_level: u8,
    ) -> Result<VersionedTransaction> {
        let mint_pk = mint_signer.pubkey();
        let token_program = TokenProgram::Token2022;
        let accounts = derive_create_accounts(&mint_pk, token_program, true);
        let create_ix = build_create_v2_ix(&mint_pk, self.config.signer.pubkey(), args, &accounts)?;
        let ixs = self.assemble_create_ixs(
            create_ix,
            &mint_pk,
            args.creator,
            token_program,
            args.cashback_enabled,
            dev_buy,
            tip_level,
        )?;
        self.sign_create_leg(ixs, mint_signer, blockhash).await
    }

    /// Compile + sign the create leg's ixs as a v0 tx over the launch ALT, signed by
    /// the dev wallet (fee payer) + the mint keypair, on the shared bundle blockhash.
    async fn sign_create_leg(
        &self,
        ixs: Vec<Instruction>,
        mint_signer: &(dyn Signer + Send + Sync),
        blockhash: solana_sdk::hash::Hash,
    ) -> Result<VersionedTransaction> {
        let alts = self.launch_alt.as_slice();
        let dev = self.config.signer.clone();
        let signers: [&(dyn Signer + Send + Sync); 2] = [dev.as_ref(), mint_signer];
        self.build_v0_tx_with_blockhash_multi(ixs, &signers, blockhash, alts)
            .await
    }

    /// The dev-buy block that fuses into create's `Core`: `[base_ata, ..extra_atas,
    /// buy]`, or empty when there is no dev buy (or a zero-lamport one). Draws the
    /// variant's raw buy (+ any WSOL quote ATA a v2 encoding needs) from the SAME
    /// `build_curve_buy_core` SSOT the bundler co-buys use — so the dev buy supports
    /// all four encodings, byte-for-byte identical to a co-buy of the same variant,
    /// with no build-then-strip of CU/tip decorations.
    fn dev_buy_core_ixs(
        &self,
        mint: &Pubkey,
        creator: Pubkey,
        token_program: TokenProgram,
        cashback_enabled: bool,
        dev_buy: Option<DevBuy>,
    ) -> Result<Vec<Instruction>> {
        let Some(DevBuy { sol: dev_buy_sol, lamports: buy_lamports, slippage_bps, variant }) =
            dev_buy
        else {
            return Ok(Vec::new());
        };
        // Same spend guard as curve/AMM buys — rejects NaN/inf/≤0/over-max, and
        // catches sol↔lamports drift from direct `DevBuy { .. }` constructors
        // (forge bundle path). A zero-lamport "no buy" is caught by
        // `buy_lamports_checked` (rounds-to-0 → Err), not short-circuited here.
        let checked = self.buy_lamports_checked(dev_buy_sol)?;
        if checked != buy_lamports {
            return Err(TradeError::InvalidBuyAmount(format!(
                "dev-buy sol={dev_buy_sol} ⇒ {checked} lamports, but DevBuy.lamports={buy_lamports}"
            )));
        }
        let buy_lamports = checked;
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
        // The curve is created in THIS tx, so it has no on-chain state to read — its
        // live reserves are the protocol-constant fresh-curve reserves. Feed those
        // through the SAME shared `curve_buy_min_out` every other buy uses (no
        // hand-inlined reserve tuple that could drift). For a SOL-in variant
        // `slippage_bps = None` keeps the historical unprotected launch behaviour
        // (min_out = 1); a tokens-out variant carries a real slippage (launcher-
        // validated) so `min_out` is a genuine token floor.
        let reserves = Some(crate::price::fresh_curve_reserves());
        let min_out = compute_curve_buy_min_out(
            buy_lamports,
            slippage_bps,
            reserves,
            self.config.slippage.curve_fee_buffer_bps,
        );
        // Buyer is the dev/creator wallet (`config.signer`). `build_curve_buy_core`
        // dispatches v1/v2 purely by `variant` and returns the raw buy plus any
        // extra ATA (the v2 WSOL quote ATA) to fuse right after the base ATA.
        let core = self.build_curve_buy_core(
            variant,
            &wallet,
            mint,
            &pdas,
            &user_ata,
            buy_lamports,
            min_out,
        )?;
        info!(
            mint = %mint,
            dev_buy_sol,
            buy_lamports,
            min_out,
            ?variant,
            "create tx includes dev-buy leg"
        );
        let mut ixs = Vec::with_capacity(2 + core.extra_atas.len());
        ixs.push(ata_ix);
        ixs.extend(core.extra_atas);
        ixs.push(core.buy_ix);
        Ok(ixs)
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
            Some(DevBuy { sol: 0.02, lamports: 20_000_000, slippage_bps: None, variant: BundleBuyVariant::BuyExactSolIn }),
            0,
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

    #[test]
    fn dev_buy_rejects_non_finite_sol() {
        let t = with_global(trader());
        let mint = Pubkey::new_unique();
        let err = t
            .dev_buy_core_ixs(
                &mint,
                t.config.signer.pubkey(),
                TokenProgram::Token2022,
                false,
                Some(DevBuy {
                    sol: f64::NAN,
                    lamports: 0,
                    slippage_bps: None,
                    variant: BundleBuyVariant::BuyExactSolIn,
                }),
            )
            .expect_err("NaN must fail the spend guard");
        assert!(matches!(err, TradeError::InvalidBuyAmount(_)), "{err:?}");
    }

    #[test]
    fn dev_buy_rejects_sol_lamports_mismatch() {
        let t = with_global(trader());
        let mint = Pubkey::new_unique();
        let err = t
            .dev_buy_core_ixs(
                &mint,
                t.config.signer.pubkey(),
                TokenProgram::Token2022,
                false,
                Some(DevBuy {
                    sol: 0.02,
                    lamports: 1, // should be 20_000_000
                    slippage_bps: None,
                    variant: BundleBuyVariant::BuyExactSolIn,
                }),
            )
            .expect_err("mismatched lamports must fail");
        assert!(matches!(err, TradeError::InvalidBuyAmount(_)), "{err:?}");
    }

    // --- Golden byte-identity: the `assemble()` refactor is a provable no-op. ---
    // Each test reconstructs the pre-refactor hand-pushed sequence and asserts the
    // new builder yields an identical `Vec<Instruction>` (same program ids,
    // accounts, and data, in the same order).

    use solana_sdk::compute_budget::ComputeBudgetInstruction as CBI;

    /// The whole-bundle Jito tip the create/launch path appends (`DecoStep::Tip`):
    /// a transfer to a JITO tip account, since launches submit via the Jito block
    /// engine. This is deliberately NOT `jito_tip_ix`, which targets a Helius
    /// Sender wallet for the single-tx buy/sell path (`sender_tip_account`).
    fn expected_bundle_tip_ix(
        t: &PumpFunTrader,
        level: u8,
    ) -> solana_sdk::instruction::Instruction {
        solana_sdk::system_instruction::transfer(
            &t.config.signer.pubkey(),
            &t.jito_tip_account,
            t.jito_tip_cache.tip_lamports_for_level(level),
        )
    }

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
            .assemble_create_ixs(create_ix.clone(), &mint.pubkey(), wallet, TokenProgram::Token2022, false, None, 0)
            .unwrap();

        let compute = &t.config.compute;
        let expected = vec![
            CBI::set_compute_unit_limit(compute.curve_create_cu),
            CBI::set_compute_unit_price(compute.price_micro_lamports),
            create_ix,
            expected_bundle_tip_ix(&t, 0),
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
                Some(DevBuy { sol: 0.02, lamports: 20_000_000, slippage_bps: None, variant: BundleBuyVariant::BuyExactSolIn }),
                0,
            )
            .unwrap();

        // Hand-build the expected dev-buy fused block: base ATA + the variant's raw
        // buy from the SAME `build_curve_buy_core` SSOT the leg builds through (v1
        // buy_exact_sol_in ⇒ no extra WSOL ATA).
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
        let buy_ix = t
            .build_curve_buy_core(
                BundleBuyVariant::BuyExactSolIn, &wallet, &mint.pubkey(), &pdas, &user_ata,
                20_000_000, min_out,
            )
            .unwrap()
            .buy_ix;

        let compute = &t.config.compute;
        let expected = vec![
            CBI::set_compute_unit_limit(compute.curve_create_buy_cu),
            CBI::set_compute_unit_price(compute.price_micro_lamports),
            create_ix,
            ata_ix,
            buy_ix,
            expected_bundle_tip_ix(&t, 0),
        ];
        assert_eq!(got, expected);
    }

    /// A legacy `create_v1` with a `buy_exact_quote_in_v2` dev-buy fuses the **v2**
    /// buy layout — the v2 encodings always use the v2 account set, independent of
    /// the (absent, for v1) cashback flag. Live `simulateTransaction` (the
    /// `launch-sim-matrix` harness) proves a v2 buy self-provisions its quote-side
    /// accounts and fills against a legacy curve, so the fused block is
    /// `[create, base_ata, wsol_ata, v2_buy]` (7 ixs incl. the WSOL quote ATA) and the
    /// buy names the 27-account v2 layout. (The `sharing_config` mis-derivation that
    /// once reverted this 2006 is fixed in `curve_buy_core_v2`.)
    #[test]
    fn create_v1_exact_quote_dev_buy_uses_v2_layout() {
        let t = with_global(trader());
        let wallet = t.config.signer.pubkey();
        let mint = Keypair::new();
        let args = CreateTokenArgs {
            name: "V1".into(),
            symbol: "V1".into(),
            uri: "ipfs://x".into(),
            creator: wallet, // dev IS the creator
        };
        let accounts = derive_create_accounts(&mint.pubkey(), TokenProgram::Legacy, false);
        let create_ix = build_create_ix(&mint.pubkey(), wallet, &args, &accounts).unwrap();

        // create_v1 has no cashback (passes `false`), yet the v2 quote-in encoding
        // still routes to the v2 layout — the buy variant alone decides it.
        let ixs = t
            .assemble_create_ixs(
                create_ix,
                &mint.pubkey(),
                wallet,
                TokenProgram::Legacy,
                false,
                Some(DevBuy {
                    sol: 0.02,
                    lamports: 20_000_000,
                    slippage_bps: Some(500),
                    variant: BundleBuyVariant::BuyExactQuoteIn,
                }),
                0,
            )
            .unwrap();

        // Canonical create layout = [cu_limit, cu_price, create, base_ata, wsol_ata,
        // buy, tip]. The v2 routing inserts the WSOL quote ATA → 7 ixs; the buy is at
        // index 5 and names the 27-account v2 layout.
        assert_eq!(ixs.len(), 7, "v2 dev-buy fuses the WSOL quote ATA");
        let buy_ix = &ixs[5];
        assert_eq!(buy_ix.program_id, protocol::PUMP_FUN);
        assert_eq!(buy_ix.accounts.len(), 27, "must be the 27-account v2 buy layout");
    }

    /// An authored `create_layout` reshapes the create tx — a lean `[Core]`
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
            .assemble_create_ixs(create_ix.clone(), &mint.pubkey(), wallet, TokenProgram::Token2022, false, None, 0)
            .unwrap();
        assert_eq!(canonical.len(), 4);

        // Authored lean `[Core]` → just the create ix.
        t.set_create_layout(Some(IxLayout { steps: vec![DecoStep::Core] }));
        let lean = t
            .assemble_create_ixs(create_ix.clone(), &mint.pubkey(), wallet, TokenProgram::Token2022, false, None, 0)
            .unwrap();
        assert_eq!(lean.len(), 1);
        assert_eq!(lean[0], create_ix, "Core placed opaque, byte-identical");
    }
}
