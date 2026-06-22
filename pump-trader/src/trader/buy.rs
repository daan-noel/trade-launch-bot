// ============================================================
// Buy — hot path.
//
// `buy_token` derives the per-token PDAs, assembles the (optional
// account-creation +) buy instructions, sends via the nonce tx path,
// and confirms. On the way out it kicks off background nonce refresh
// and pool replenishment so the next buy starts warm.
// ============================================================

use super::PumpFunTrader;
use crate::constants::{CONFIRM_MAX_RETRIES, CURVE_FEE_BUFFER_BPS};
use crate::types::TokenProgram;
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
    /// Manual/API curve buy. Takes already-parsed routing pubkeys (the manual
    /// path resolves them once in `resolve_buy_routing`) so nothing on this path
    /// re-parses the mint/creator/program strings.
    pub async fn buy_token(
        &self,
        mint: &Pubkey,
        creator: &Pubkey,
        token_program: TokenProgram,
        sol_amount: f64,
        slippage_bps: Option<u64>,
    ) -> Result<bool> {
        // Manual buy: no triggering-event reserves in hand, so `buy_token_inner`
        // reads the curve on-chain for the slippage floor (snipe_reserves = None).
        self.buy_token_inner(mint, creator, token_program, sol_amount, slippage_bps, None, false, false)
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
    ///
    /// `reserves` is the curve's virtual `(token, quote=lamports)` reserves the
    /// caller already has in hand from the triggering trade event — threaded
    /// straight into the slippage `min_out` so this hot path never makes an inline
    /// reserve RPC. `None` (no snapshot available) yields `min_out=1` (no
    /// protection) rather than blocking the snipe on a network read.
    pub async fn buy_token_snipe(
        &self,
        token_mint: &str,
        creator: &str,
        token_program_id: &str,
        sol_amount: f64,
        slippage_bps: Option<u64>,
        reserves: Option<(u128, u128)>,
    ) -> Result<String> {
        // Parse the feed/DB strings once here (the snipe path's only source is
        // strings); `buy_token_inner` then works purely in parsed forms.
        let mint = Pubkey::from_str(token_mint)?;
        let creator_pubkey = Pubkey::from_str(creator)?;
        let token_program = TokenProgram::from_id(token_program_id);
        self.buy_token_inner(&mint, &creator_pubkey, token_program, sol_amount, slippage_bps, reserves, true, true)
            .await
    }

    // Trade-path fn — the buy needs every routing/slippage/skip input threaded in;
    // `too_many_arguments` is allowed by design (see CLAUDE.md), like the sell path.
    #[allow(clippy::too_many_arguments)]
    async fn buy_token_inner(
        &self,
        mint: &Pubkey,
        creator_pubkey: &Pubkey,
        token_program: TokenProgram,
        sol_amount: f64,
        slippage_bps: Option<u64>,
        // Caller-supplied virtual `(token, quote=lamports)` reserves for the
        // slippage floor — `Some` on the snipe path (from the triggering event),
        // `None` on the manual path (read the curve on-chain below).
        snipe_reserves: Option<(u128, u128)>,
        skip_ata_check: bool,
        skip_confirm: bool,
    ) -> Result<String> {
        let t0 = Instant::now();
        // Guard the real spend before any work: both curve public entries
        // (`buy_token`, `buy_token_snipe`) funnel through here, so this single
        // check rejects a NaN/∞, non-positive, oversized, or rounds-to-zero
        // `sol_amount` for both. API callers are also validated up front; this
        // is the crate's own backstop.
        let buy_lamports = super::buy_lamports_checked(sol_amount)?;
        let keypair = &self.config.keypair;

        async {
            let global = self.global_account.as_ref().context("Not initialized")?;

            let token_program_pk = token_program.pubkey();
            // Cache keys are the mint's base58 string; compute it once.
            let mint_str = mint.to_string();

            // Curve PDAs via the shared derivation (same source of truth as the
            // query path). `Pubkey` is `Copy`, so the locals below are copies and
            // `pdas` is still moved into the cache.
            let pdas = self.derive_token_pdas(mint, creator_pubkey, &token_program_pk, false);
            let bonding_curve = pdas.bonding_curve;
            let bonding_curve_v2 = pdas.bonding_curve_v2;
            let assoc_bonding_curve = pdas.associated_bonding_curve;
            let creator_vault = pdas.creator_vault;

            self.token_pdas.insert(mint_str.clone(), pdas);

            // Check if ATA exists. On the snipe path the wallet provably holds
            // no account for this just-created mint, so we skip the RPC and go
            // straight to the seed-account (template) path.
            let ata = get_associated_token_address_with_program_id(
                &keypair.pubkey(),
                mint,
                &token_program_pk,
            );
            // Resolve the user token account + (if needed) its create template.
            // The snipe path provably holds no account for this just-created mint,
            // so it skips the RPC entirely. The manual path must probe — but the
            // probe's RPC RTT is overlapped with template acquisition (a buy needs
            // one unless the ATA already exists), so the round-trip hides behind
            // work we'd do anyway; an unneeded template is handed straight back.
            let (user_token_account, template_opt) = if skip_ata_check {
                let template = self.acquire_buy_template(token_program).await?;
                let account = template.user_token_account;
                self.replenish_pool_async(token_program);
                (account, Some(template))
            } else {
                let (ata_exists, template) = tokio::join!(
                    async { self.rpc.get_account(&ata).await.is_ok() },
                    self.acquire_buy_template(token_program),
                );
                let template = template?;
                if ata_exists {
                    // ATA already exists (re-buy): the template is unconsumed and
                    // holds no on-chain resource, so return it to the pool.
                    self.return_buy_template(token_program, template).await;
                    (ata, None)
                } else {
                    let account = template.user_token_account;
                    // A template was just consumed — kick the background refill off
                    // here so it rebuilds concurrently with the tx assembly + send +
                    // confirm below, instead of only after the buy returns. The
                    // rebuild gets a head start of the whole send/confirm window, so
                    // the next buy is more likely to hit a warm pool.
                    self.replenish_pool_async(token_program);
                    (account, Some(template))
                }
            };

            // Cache for sell
            self.user_token_accounts
                .insert(mint_str.clone(), user_token_account);

            let mut ixs = Vec::with_capacity(6);
            ixs.extend_from_slice(&self.cu_ixs_curve_buy);

            if let Some(template) = template_opt {
                // FIX: use the same template we already acquired above
                ixs.push(template.create_with_seed_ix);

                let init_ix = match token_program {
                    TokenProgram::Legacy => spl_token::instruction::initialize_account3(
                        &token_program_pk,
                        &user_token_account,
                        mint,
                        &keypair.pubkey(),
                    )?,
                    TokenProgram::Token2022 => spl_token_2022::instruction::initialize_account3(
                        &token_program_pk,
                        &user_token_account,
                        mint,
                        &keypair.pubkey(),
                    )?,
                };
                ixs.push(init_ix);
            }

            // `buy_exact_sol_in(spendable_quote_in, min_tokens_out)`: slippage
            // floor on tokens received. `None` slippage keeps the legacy min_out=1
            // (no protection) and never touches reserves. With a slippage tolerance,
            // the reserve source is:
            //   - the snipe path's caller-supplied `snipe_reserves` (the triggering
            //     event's virtual reserves) — NO inline RPC on the hot path;
            //   - the manual path reads the curve on-chain (`curve_reserves`);
            //   - a snipe with no snapshot in hand falls back to min_out=1 rather
            //     than an inline reserve RPC, so a missing read never blocks the buy.
            let reserves: Option<(u128, u128)> = match (slippage_bps, snipe_reserves) {
                (None, _) => None,
                (Some(_), Some(r)) => Some(r),
                // Manual path only: read the curve on-chain. The snipe path
                // (skip_ata_check) deliberately never reads here.
                (Some(_), None) if !skip_ata_check => {
                    match self.curve_reserves(&mint_str, &bonding_curve).await {
                        Ok(r) => Some(r),
                        Err(e) => {
                            warn!("curve buy slippage: reserve read failed ({e}); using min_out=1");
                            None
                        }
                    }
                }
                (Some(_), None) => None,
            };
            let min_tokens_out: u64 = match (slippage_bps, reserves) {
                (Some(slip), Some((vt, vq))) => {
                    let net = (buy_lamports as u128)
                        .saturating_mul(10_000u128.saturating_sub(CURVE_FEE_BUFFER_BPS))
                        / 10_000;
                    // Saturating throughout on untrusted reserve reads; a zero
                    // denominator (both reserves read as 0) falls back to the
                    // unprotected min_out=1 rather than panicking.
                    let denom = vq.saturating_add(net);
                    if denom == 0 {
                        1
                    } else {
                        let expected = vt.saturating_mul(net) / denom;
                        ((expected.saturating_mul(10_000u128.saturating_sub(slip as u128)) / 10_000)
                            as u64)
                            .max(1)
                    }
                }
                _ => 1,
            };

            // 8-byte discriminator + two u64 args: size up front so the two
            // extends below don't reallocate on the buy hot path.
            let mut buy_data = Vec::with_capacity(24);
            buy_data.extend_from_slice(&[0x38, 0xfc, 0x74, 0x08, 0x9e, 0xdf, 0xcd, 0x5f]);
            buy_data.extend_from_slice(&buy_lamports.to_le_bytes());
            buy_data.extend_from_slice(&min_tokens_out.to_le_bytes());
            ixs.push(Instruction {
                program_id: self.pump_program,
                accounts: vec![
                    AccountMeta::new_readonly(global.global_pda, false),
                    AccountMeta::new(global.fee_recipient, false),
                    AccountMeta::new(*mint, false),
                    AccountMeta::new(bonding_curve, false),
                    AccountMeta::new(assoc_bonding_curve, false),
                    AccountMeta::new(user_token_account, false),
                    AccountMeta::new(keypair.pubkey(), true),
                    AccountMeta::new_readonly(self.system_program, false),
                    AccountMeta::new_readonly(token_program_pk, false),
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

            // Buys are a single shot (the snipe re-send only fires on a revert,
            // which a bigger tip can't fix), so always the level-0 tip.
            ixs.push(self.jito_tip_ix(0));

            // Acquire the nonce only now — after PDA derivation, the ATA-exists
            // RPC, template acquisition, and the slippage reserve read above — so
            // the slot isn't held `in_use` across any of those reads. Only the
            // build/send/confirm below can fail while holding it, and the inner
            // block always falls through to `schedule_nonce_refresh`.
            let (nonce_pubkey, nonce_hash) = self.acquire_nonce().await?;
            let sent: Result<String> = async {
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
            sent
        }
        .await
    }
}
