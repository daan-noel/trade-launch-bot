// ============================================================
// Buy — hot path.
//
// `buy_token` derives the per-token PDAs, assembles the (optional
// account-creation +) buy instructions, sends via the nonce tx path,
// and confirms. On the way out it kicks off background nonce refresh
// and pool replenishment so the next buy starts warm.
// ============================================================

use super::PumpFunTrader;
use crate::error::{Context, Result, TradeError};
use crate::types::TokenProgram;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};
use spl_associated_token_account::get_associated_token_address_with_program_id;
use std::future::Future;
use std::pin::Pin;
use std::str::FromStr;
use std::time::Instant;
use tracing::{info, warn};

/// Async callback invoked with a buy's transaction signature the instant the tx
/// is **signed** — which, against a durable nonce, is *before* the network
/// round-trip — and BEFORE it is submitted. Lets a caller durably record the
/// signature ahead of the on-chain side effect (write-ahead crash-safety: the
/// signature is on disk before any tokens can arrive, so a crash anywhere after
/// signing is recoverable). Boxed so it threads through the buy path without
/// making it generic; `None` on paths that don't need the marker (manual buys).
pub type BuySignedHook =
    Box<dyn FnOnce(String) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send>;

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
        self.buy_token_inner(mint, creator, token_program, sol_amount, slippage_bps, None, false, false, None)
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
        self.buy_token_inner(&mint, &creator_pubkey, token_program, sol_amount, slippage_bps, reserves, true, true, None)
            .await
    }

    /// As [`buy_token_snipe`], but invokes `on_signed` with the buy's signature
    /// the instant the tx is signed and **before** it is submitted — the Phase 2
    /// *write-ahead* entry point. Because the buy is signed against a durable
    /// nonce, the signature is fixed locally before the network round-trip, so a
    /// caller can persist a durable "buy in flight" marker keyed on that signature
    /// *ahead* of any on-chain side effect. This closes the last persist-after-send
    /// window: a crash between submit and record can no longer strand untracked
    /// tokens, because the signature was already on disk before submit. Returns the
    /// submitted signature (identical to the one handed to `on_signed`).
    // Trade-path fn — the write-ahead buy threads the same routing/slippage inputs
    // plus the persist hook; `too_many_arguments` is allowed by design (see
    // CLAUDE.md), like `buy_token_inner`/the sell path.
    #[allow(clippy::too_many_arguments)]
    pub async fn buy_token_snipe_write_ahead(
        &self,
        token_mint: &str,
        creator: &str,
        token_program_id: &str,
        sol_amount: f64,
        slippage_bps: Option<u64>,
        reserves: Option<(u128, u128)>,
        on_signed: BuySignedHook,
    ) -> Result<String> {
        let mint = Pubkey::from_str(token_mint)?;
        let creator_pubkey = Pubkey::from_str(creator)?;
        let token_program = TokenProgram::from_id(token_program_id);
        self.buy_token_inner(&mint, &creator_pubkey, token_program, sol_amount, slippage_bps, reserves, true, true, Some(on_signed))
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
        // Write-ahead hook: invoked with the signed tx's signature BEFORE submit
        // (the durable-nonce signature is fixed at signing), so the caller can
        // persist a recovery marker ahead of the on-chain side effect. `None` on
        // the manual/legacy paths that don't need it.
        on_signed: Option<BuySignedHook>,
    ) -> Result<String> {
        let t0 = Instant::now();
        // Guard the real spend before any work: both curve public entries
        // (`buy_token`, `buy_token_snipe`) funnel through here, so this single
        // check rejects a NaN/∞, non-positive, oversized, or rounds-to-zero
        // `sol_amount` for both. API callers are also validated up front; this
        // is the crate's own backstop.
        let buy_lamports = self.buy_lamports_checked(sol_amount)?;
        let signer = self.config.signer.as_ref();

        async {
            // Fail fast if the trader isn't initialized (the buy ix builder needs
            // the global account); the actual read happens inside the builder.
            if self.global_account.is_none() {
                return Err(TradeError::NotInitialized);
            }

            let token_program_pk = token_program.pubkey();
            // Cache keys are the mint's base58 string; compute it once.
            let mint_str = mint.to_string();

            // Curve PDAs via the shared derivation (same source of truth as the
            // query path). `Pubkey` is `Copy`, so the locals below are copies and
            // `pdas` is still moved into the cache.
            let pdas = self.derive_token_pdas(mint, creator_pubkey, &token_program_pk, false);
            // `bonding_curve` is read below for the manual-path slippage reserve
            // read; the rest of the curve PDAs are consumed inside
            // `build_curve_buy_ixs` straight off `pdas` (it's `Copy`, so the insert
            // below doesn't move it away).
            let bonding_curve = pdas.bonding_curve;

            self.token_pdas.insert(mint_str.clone(), pdas);

            // Check if ATA exists. On the snipe path the wallet provably holds
            // no account for this just-created mint, so we skip the RPC and go
            // straight to the seed-account (template) path.
            let ata = get_associated_token_address_with_program_id(
                &signer.pubkey(),
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

            // Account-creation prefix: when the wallet held no token account, the
            // just-acquired template carries the create-with-seed + initialize ixs
            // that must run before the buy. Empty on a re-buy (ATA already exists).
            // Built here (not in `build_curve_buy_ixs`) because only the live buy
            // path consumes a pooled template; the simulate path mints its own ATA.
            let mut account_creation_ixs: Vec<Instruction> = Vec::new();
            if let Some(template) = template_opt {
                // FIX: use the same template we already acquired above
                account_creation_ixs.push(template.create_with_seed_ix);

                let init_ix = match token_program {
                    TokenProgram::Legacy => spl_token::instruction::initialize_account3(
                        &token_program_pk,
                        &user_token_account,
                        mint,
                        &signer.pubkey(),
                    )?,
                    TokenProgram::Token2022 => spl_token_2022::instruction::initialize_account3(
                        &token_program_pk,
                        &user_token_account,
                        mint,
                        &signer.pubkey(),
                    )?,
                };
                account_creation_ixs.push(init_ix);
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
            let min_tokens_out = compute_curve_buy_min_out(
                buy_lamports,
                slippage_bps,
                reserves,
                self.config.slippage.curve_fee_buffer_bps,
            );

            // Assemble the curve-buy ix set (compute budget + account creation +
            // `buy` + Jito tip) through the shared builder, so the simulate path
            // sends the byte-identical buy instruction the live path does.
            let ixs = self.build_curve_buy_ixs(
                mint,
                &pdas,
                &user_token_account,
                account_creation_ixs,
                buy_lamports,
                min_tokens_out,
            )?;

            // Acquire the nonce only now — after PDA derivation, the ATA-exists
            // RPC, template acquisition, and the slippage reserve read above — so
            // the slot isn't held `in_use` across any of those reads. Only the
            // build/send/confirm below can fail while holding it, and the inner
            // block always falls through to `schedule_nonce_refresh`.
            let (nonce_pubkey, nonce_hash) = self.acquire_nonce().await?;
            let sent: Result<String> = async {
                let tx = self.build_nonce_tx(ixs, &nonce_pubkey, nonce_hash, signer)?;
                // Write-ahead persist (Phase 2): the durable-nonce signature is
                // fixed the instant we sign — before any network round-trip — so
                // hand it to the hook to durably record the "buy in flight" marker
                // BEFORE the submit below. A crash anywhere after this point is
                // recoverable; a crash before it means the tx never went out, so no
                // tokens can exist. The hook is one DB write under the nonce's
                // `in_use` window (the slot frees in `schedule_nonce_refresh`
                // below regardless) — an intentional, bounded latency trade for
                // crash-safety, off the ingest hot path (this is the spawned buy
                // task).
                if let Some(hook) = on_signed {
                    let sig = tx
                        .signatures
                        .first()
                        .map(|s| s.to_string())
                        .context("signed buy tx has no signature")?;
                    hook(sig).await;
                }
                let sig = self.send_transaction(&tx).await?;
                info!(
                    "📤 Buy sent — sig: {} | SOL: {} | {}ms",
                    sig,
                    sol_amount,
                    t0.elapsed().as_millis()
                );

                if !skip_confirm {
                    self.confirm_transaction(&sig, self.config.retry.confirm_max_retries)
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

    /// Assemble the curve-buy instruction set (compute budget + optional
    /// account-creation prefix + `buy_exact_sol_in` + Jito tip) for a known
    /// mint/account. Pure tx construction — no RPC, no signing — extracted from
    /// `buy_token_inner` so the simulate path builds the *identical* buy
    /// instruction the live path sends (mirrors [`Self::build_curve_sell_ixs`]).
    /// `account_creation_ixs` is the create-with-seed + initialize prefix on a
    /// first buy (empty on a re-buy); `min_tokens_out` is the slippage floor
    /// (1 = no protection). The buy is always a single shot, so the tip is fixed
    /// at level 0.
    pub(super) fn build_curve_buy_ixs(
        &self,
        mint: &Pubkey,
        pdas: &super::TokenPDAs,
        user_token_account: &Pubkey,
        account_creation_ixs: Vec<Instruction>,
        buy_lamports: u64,
        min_tokens_out: u64,
    ) -> Result<Vec<Instruction>> {
        let global = self.global_account.as_ref().context("Not initialized")?;

        let mut ixs = Vec::with_capacity(6);
        ixs.extend_from_slice(&self.cu_ixs_curve_buy);
        ixs.extend(account_creation_ixs);

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
                AccountMeta::new(pdas.bonding_curve, false),
                AccountMeta::new(pdas.associated_bonding_curve, false),
                AccountMeta::new(*user_token_account, false),
                AccountMeta::new(self.config.signer.pubkey(), true),
                AccountMeta::new_readonly(self.system_program, false),
                AccountMeta::new_readonly(pdas.token_program, false),
                AccountMeta::new(pdas.creator_vault, false),
                AccountMeta::new_readonly(self.event_authority, false),
                AccountMeta::new_readonly(self.pump_program, false),
                AccountMeta::new(global.global_volume_accumulator, false),
                AccountMeta::new(global.user_volume_accumulator, false),
                AccountMeta::new_readonly(global.fee_config, false),
                AccountMeta::new_readonly(self.fee_program, false),
                AccountMeta::new_readonly(pdas.bonding_curve_v2, false),
                AccountMeta::new(self.curve_fee_recipient, false),
            ],
            data: buy_data,
        });

        // Buys are a single shot (the snipe re-send only fires on a revert,
        // which a bigger tip can't fix), so always the level-0 tip.
        ixs.push(self.jito_tip_ix(0));

        Ok(ixs)
    }
}

/// Curve-buy slippage floor: the minimum tokens the `buy` must return or revert.
/// Pure (no RPC) so both the live buy path and the simulate path derive the floor
/// identically. `None` slippage (or missing/zero reserves) yields `1` — no
/// protection — rather than blocking on a reserve read. `reserves` is the curve's
/// virtual `(token, quote=lamports)` reserves; the math mirrors the on-chain
/// constant-product fill, net of the curve fee buffer.
pub(super) fn compute_curve_buy_min_out(
    buy_lamports: u64,
    slippage_bps: Option<u64>,
    reserves: Option<(u128, u128)>,
    curve_fee_buffer_bps: u128,
) -> u64 {
    match (slippage_bps, reserves) {
        (Some(slip), Some((vt, vq))) => {
            let net = (buy_lamports as u128)
                .saturating_mul(10_000u128.saturating_sub(curve_fee_buffer_bps))
                / 10_000;
            // Saturating throughout on untrusted reserve reads; a zero
            // denominator (both reserves read as 0) falls back to the
            // unprotected min_out=1 rather than panicking.
            let denom = vq.saturating_add(net);
            if denom == 0 {
                1
            } else {
                let expected = vt.saturating_mul(net) / denom;
                ((expected.saturating_mul(10_000u128.saturating_sub(slip as u128)) / 10_000) as u64)
                    .max(1)
            }
        }
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::compute_curve_buy_min_out;

    /// Default `config.slippage.curve_fee_buffer_bps`.
    const FEE_BUF: u128 = 200;

    #[test]
    fn min_out_is_unprotected_without_slippage_or_reserves() {
        // No slippage tolerance → no floor.
        assert_eq!(compute_curve_buy_min_out(1_000_000, None, Some((1_000, 2_000)), FEE_BUF), 1);
        // Slippage set but no reserves in hand → no floor (never blocks the buy).
        assert_eq!(compute_curve_buy_min_out(1_000_000, Some(500), None, FEE_BUF), 1);
        // Zero reserves (degenerate read) → no floor rather than a panic.
        assert_eq!(compute_curve_buy_min_out(1_000_000, Some(500), Some((0, 0)), FEE_BUF), 1);
    }

    #[test]
    fn tighter_slippage_raises_the_floor() {
        let reserves = Some((1_000_000_000u128, 30_000_000u128));
        let loose = compute_curve_buy_min_out(1_000_000, Some(5_000), reserves, FEE_BUF); // 50%
        let tight = compute_curve_buy_min_out(1_000_000, Some(100), reserves, FEE_BUF); // 1%
        assert!(tight >= loose, "tighter slippage must demand at least as many tokens");
        assert!(loose >= 1 && tight >= 1, "floor is always >= 1");
    }
}
