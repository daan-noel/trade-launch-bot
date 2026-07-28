// ============================================================
// Buy — hot path.
//
// `buy_token` derives the per-token PDAs, assembles the (optional
// account-creation +) buy instructions, sends via the nonce tx path,
// and confirms. On the way out it kicks off background nonce refresh
// and pool replenishment so the next buy starts warm.
// ============================================================

use executor_core::{classify_swap_revert, SwapDirection, SwapRetryDecision, SwapRoute, TxAnchor};
use super::PumpFunTrader;
use crate::error::{Context, Result, TradeError};
use crate::protocol;
use crate::types::TokenProgram;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    system_program,
};
use spl_associated_token_account::{
    get_associated_token_address_with_program_id,
    instruction::create_associated_token_account_idempotent,
};
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

/// What a snipe buy produced: the submitted signature **and** the token account
/// that specific buy funded.
///
/// The account is returned rather than left to be re-read from
/// [`PumpFunTrader::cached_token_account`] because that cache is keyed by mint
/// only. Two concurrent snipes on the same mint each draw their own seeded
/// account from the template pool, then both write that one key — last writer
/// wins. The caller that read it back got the *other* buy's account, persisted it
/// on its position, and its exit later sold from an account holding none of its
/// tokens: a structural revert, `ExitStuck`, bag stranded in an account no row
/// referenced. (Observed 2026-07-28 on mints 7cstqrt… and BW7nqMs… — two rules
/// entering the same mint in the same slot.)
///
/// The account a buy funded is a fact about *that buy*, so it travels with it.
#[derive(Debug, Clone)]
pub struct SnipeBuy {
    pub signature: String,
    pub user_token_account: Pubkey,
}

impl PumpFunTrader {
    /// Manual/API curve buy. Takes already-parsed routing pubkeys (the manual
    /// path resolves them once in `resolve_buy_routing`) so nothing on this path
    /// re-parses the mint/creator/program strings.
    /// Returns the submitted transaction signature on success.
    pub async fn buy_token(
        &self,
        mint: &Pubkey,
        creator: &Pubkey,
        token_program: TokenProgram,
        sol_amount: f64,
        slippage_bps: Option<u64>,
        cashback_enabled: bool,
    ) -> Result<String> {
        // Manual buy: no triggering-event reserves in hand, so `buy_token_inner`
        // reads the curve on-chain for the slippage floor (snipe_reserves = None).
        self.buy_token_inner(
            mint,
            creator,
            token_program,
            sol_amount,
            slippage_bps,
            None,
            false,
            false,
            None,
            cashback_enabled,
            None,
            0,
            TxAnchor::Standard,
        )
        .await
        .map(|(sig, _account)| sig)
    }

    /// Latency-optimized write-ahead buy for fresh-token snipes. Identical to
    /// [`buy_token`] but skips the ATA-existence RPC round-trip (safe only when the
    /// wallet provably holds no account for `token_mint` yet — e.g. a token just
    /// seen via the pump.fun create event; the only consequence if that assumption
    /// is ever wrong is one extra create-with-seed token account, never a failed or
    /// misrouted trade) and invokes `on_signed` with the buy's signature the instant
    /// the tx is signed and **before** it is submitted — the Phase 2
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
        cashback_enabled: bool,
        // When `Some`, buy directly into this already-existing token account —
        // skip the create-with-seed template pool entirely. Set on a subsequent
        // bot buy into a mint already held (the account was persisted on the first
        // fill), so both buys land in ONE account. `None` = first buy: the template
        // mints (and caches) a fresh account as before.
        user_token_account_override: Option<Pubkey>,
        // Tip ladder level (0 = first attempt). Live retries after a confirmed
        // safe revert pass the journal length so the next send bids up.
        tip_level: u8,
    ) -> Result<SnipeBuy> {
        let mint = Pubkey::from_str(token_mint)?;
        let creator_pubkey = Pubkey::from_str(creator)?;
        let token_program = TokenProgram::from_id(token_program_id);
        self.buy_token_inner(
            &mint,
            &creator_pubkey,
            token_program,
            sol_amount,
            slippage_bps,
            reserves,
            true,
            true,
            Some(on_signed),
            cashback_enabled,
            user_token_account_override,
            tip_level,
            // The ONE latency-critical buy: never block on nonce contention.
            TxAnchor::Entry,
        )
        .await
        .map(|(signature, user_token_account)| SnipeBuy { signature, user_token_account })
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
        // The true cashback flag from routing (create_v2 tokens). Threaded here so
        // the cached PDAs carry the correct `cashback_enabled`, preventing a future
        // sell that reads only `pdas.cashback_enabled` from silently dropping the
        // UVA account and reverting with pump.fun error 6024.
        cashback_enabled: bool,
        // When `Some` (subsequent bot buy into a held mint), buy directly into this
        // existing account — no template pool, no create-with-seed prefix — so both
        // buys land in ONE account. `None` = first buy / manual path: resolve the
        // account via template (snipe) or the real ATA (manual) as below.
        user_token_account_override: Option<Pubkey>,
        tip_level: u8,
        // Per-call nonce-wait policy. `Entry` on the snipe (bail to a recent
        // blockhash in ~40 ms rather than spin up to 4 s); `Standard` on the
        // manual/API buy, which has no slot budget to protect.
        anchor: TxAnchor,
    ) -> Result<(String, Pubkey)> {
        let t0 = Instant::now();
        // Guard the real spend before any work: both curve public entries
        // (`buy_token`, `buy_token_snipe_write_ahead`) funnel through here, so this single
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
            let pdas = self.derive_token_pdas(mint, creator_pubkey, &token_program_pk, cashback_enabled);
            // `bonding_curve` is read below for the manual-path slippage reserve
            // read; the rest of the curve PDAs are consumed inside
            // `build_curve_buy_ixs` straight off `pdas` (it's `Copy`, so the insert
            // below doesn't move it away).
            let bonding_curve = pdas.bonding_curve;

            self.token_pdas.insert(mint_str.clone(), pdas);

            // Resolve the user token account + (if needed) its account-creation
            // prefix. Three cases:
            //   - caller-supplied override (subsequent bot buy into a held mint):
            //     the account already exists, so buy straight into it — no
            //     template, no create prefix;
            //   - snipe path (`skip_ata_check`): the wallet provably holds no
            //     account for this just-created mint, so skip any existence probe
            //     and go straight to the seed-account (template) pool — this is
            //     the latency-critical path the pool exists for;
            //   - manual path: not latency-sensitive (an RPC round trip already
            //     happens either way), so always target the real ATA and prefix
            //     an idempotent create-ATA ix — a no-op if it already exists, and
            //     no create-with-seed account for indexers like GMGN to miss.
            let (user_token_account, template_opt) = if let Some(existing) = user_token_account_override {
                (existing, None)
            } else if skip_ata_check {
                let template = self.acquire_buy_template(token_program).await?;
                let account = template.user_token_account;
                self.replenish_pool_async(token_program);
                (account, Some(template))
            } else {
                let ata = get_associated_token_address_with_program_id(
                    &signer.pubkey(),
                    mint,
                    &token_program_pk,
                );
                (ata, None)
            };

            // Convenience cache for cold/manual sells that have no account in hand.
            // NOT the source of truth: it is keyed by mint, so concurrent buys on
            // one mint overwrite each other. A position must persist the account
            // returned in `SnipeBuy` and pass it back as `token_account_override`.
            self.user_token_accounts
                .insert(mint_str.clone(), user_token_account);

            // Account-creation prefix: when the wallet held no token account, the
            // just-acquired template carries the create-with-seed + initialize ixs
            // that must run before the buy. Empty on a re-buy (ATA already exists)
            // and on the manual path, which uses an idempotent create-ATA ix
            // instead (always safe to include; a no-op if the ATA already exists).
            // Built here (not in `build_curve_buy_ixs`) because only the live buy
            // path consumes a pooled template; the simulate path mints its own ATA.
            let mut account_creation_ixs: Vec<Instruction> = Vec::new();
            if let Some(template) = template_opt {
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
            } else if user_token_account_override.is_none() && !skip_ata_check {
                account_creation_ixs.push(create_associated_token_account_idempotent(
                    &signer.pubkey(),
                    &signer.pubkey(),
                    mint,
                    &token_program_pk,
                ));
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

            // At most one self-heal resend on a confirmed stale-`creator_vault`
            // 2006 — the curve-buy analogue of `sell.rs::execute_sell`'s heal
            // (same shared `classify_swap_revert` decision). Only reachable when
            // `!skip_confirm` (manual/API callers); the snipe path always sets
            // `skip_confirm = true` and never sees a sync revert here — its
            // recovery stays feed-driven (`classify_silent_send`). A confirmed
            // revert bought nothing, and the resend takes a fresh nonce, so it
            // can't double-buy.
            let mut healed = false;
            let mut current_pdas = pdas;
            // `on_signed` fires once, on the FIRST signed tx only — the healed
            // resend is a distinct tx the caller's write-ahead marker doesn't
            // need to re-anchor to (the original signature already covers "a buy
            // for this position is in flight").
            let mut on_signed = on_signed;
            // Cloned up front: `build_curve_buy_ixs` consumes its instructions,
            // and the healed resend rebuilds with the SAME (idempotent) creation
            // prefix against the freshly-refreshed PDAs.
            let account_creation_ixs_retry = account_creation_ixs.clone();

            // Stale-creator heal bumps the tip one rung on the resend — a 2006
            // itself isn't tip-related, but the second send still competes in
            // the same auction and a free re-bid is cheap insurance.
            let mut tip_level = tip_level;
            loop {
                let ixs = self.build_curve_buy_ixs(
                    mint,
                    &current_pdas,
                    &user_token_account,
                    if healed {
                        account_creation_ixs_retry.clone()
                    } else {
                        account_creation_ixs.clone()
                    },
                    buy_lamports,
                    min_tokens_out,
                    tip_level,
                )?;

                // Build the tx only now — after PDA derivation, the ATA-exists RPC,
                // template acquisition, and the slippage reserve read above. In
                // durable-nonce mode (hunter) this acquires a slot held only across
                // the build/send/confirm below, always freed via
                // `schedule_nonce_refresh`; in recent-blockhash mode (forge's
                // ephemeral wallets) there is no slot to hold.
                let (tx, nonce_to_refresh) = self.build_trade_tx(ixs, signer, anchor).await?;
                let sent: Result<String> = async {
                    // Write-ahead persist (Phase 2): the signature is fixed the
                    // instant we sign — before any network round-trip — so hand it to
                    // the hook to durably record the "buy in flight" marker BEFORE the
                    // submit below. A crash anywhere after this point is recoverable; a
                    // crash before it means the tx never went out, so no tokens can
                    // exist. Off the ingest hot path (this is the spawned buy task).
                    if let Some(hook) = on_signed.take() {
                        let sig = tx
                            .signatures
                            .first()
                            .map(|s| s.to_string())
                            .context("signed buy tx has no signature")?;
                        hook(sig).await;
                    }
                    // Snipe on the cheap best-effort Sender tier: keep re-posting
                    // the identical signed tx in the background so it gets multiple
                    // landing chances across leader slots (the bank dedups on
                    // signature → at most one execution, tip paid once). Gated on
                    // the ENTRY anchor, not on `durable_nonce`: the entry can fall
                    // back to a recent blockhash under nonce contention and must
                    // keep rebroadcasting when it does (a blockhash outlives the 5 s
                    // window ~12x). The manual/confirm path sends once as before.
                    let sig = if matches!(anchor, TxAnchor::Entry) {
                        self.send_transaction_rebroadcast(&tx).await?
                    } else {
                        self.send_transaction(&tx).await?
                    };
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

                if let Some(nonce_pubkey) = nonce_to_refresh {
                    self.schedule_nonce_refresh(nonce_pubkey);
                }

                if let Err(TradeError::Reverted { custom }) = &sent {
                    if !healed
                        && classify_swap_revert(*custom, SwapRoute::Curve, SwapDirection::Buy)
                            == SwapRetryDecision::RefreshCreator
                    {
                        match self.refresh_curve_creator_vault(&mint_str).await {
                            Ok(Some(vault)) => {
                                if let Some(fresh) = self.token_pdas.get(&mint_str).map(|r| *r) {
                                    current_pdas = fresh;
                                }
                                info!(
                                    "🔄 Buy reverted on a stale creator_vault (pump set_creator); \
                                     refreshed to {vault}, resending once"
                                );
                                healed = true;
                                tip_level = tip_level.saturating_add(1);
                                continue;
                            }
                            // Unchanged creator or the refresh itself failed — stop
                            // rather than re-pay fees on a resend that can't fix anything.
                            Ok(None) | Err(_) => return sent.map(|s| (s, user_token_account)),
                        }
                    }
                }
                return sent.map(|s| (s, user_token_account));
            }
        }
        .await
    }

    /// Assemble the curve-buy instruction set (compute budget + optional
    /// account-creation prefix + `buy_exact_sol_in` + tip) for a known
    /// mint/account. Pure tx construction — no RPC, no signing — extracted from
    /// `buy_token_inner` so the simulate path builds the *identical* buy
    /// instruction the live path sends (mirrors [`Self::build_curve_sell_ixs`]).
    /// `account_creation_ixs` is the create-with-seed + initialize prefix on a
    /// first buy (empty on a re-buy); `min_tokens_out` is the slippage floor
    /// (1 = no protection). `tip_level` escalates the Sender tip on retries
    /// (see `jito_tip`).
    pub(super) fn build_curve_buy_ixs(
        &self,
        mint: &Pubkey,
        pdas: &super::TokenPDAs,
        user_token_account: &Pubkey,
        account_creation_ixs: Vec<Instruction>,
        buy_lamports: u64,
        min_tokens_out: u64,
        tip_level: u8,
    ) -> Result<Vec<Instruction>> {
        let mut ixs = Vec::with_capacity(6);
        ixs.extend_from_slice(&self.engine.cu_ixs_curve_buy);
        ixs.extend(account_creation_ixs);
        ixs.push(self.curve_buy_ix(mint, pdas, user_token_account, buy_lamports, min_tokens_out)?);
        ixs.push(self.jito_tip_ix(tip_level));

        Ok(ixs)
    }

    /// The bare curve-buy instruction (`buy_exact_sol_in`) — SSOT for the buy
    /// account list + arg encoding. `build_curve_buy_ixs` wraps it with the CU
    /// budget + tip for the standalone buy path; the launch-create dev-buy path
    /// ([`super::create`]) reuses just this ix inside the fused create `Core`
    /// block, so there is no build-then-strip of the CU/tip decorations.
    pub(super) fn curve_buy_ix(
        &self,
        mint: &Pubkey,
        pdas: &super::TokenPDAs,
        user_token_account: &Pubkey,
        buy_lamports: u64,
        min_tokens_out: u64,
    ) -> Result<Instruction> {
        let global = self.global_account.as_ref().context("Not initialized")?;

        // 8-byte discriminator + two u64 args: size up front so the two
        // extends below don't reallocate on the buy hot path.
        let mut buy_data = Vec::with_capacity(24);
        buy_data.extend_from_slice(&protocol::BUY_EXACT_SOL_IN_DISC);
        buy_data.extend_from_slice(&buy_lamports.to_le_bytes());
        buy_data.extend_from_slice(&min_tokens_out.to_le_bytes());
        Ok(Instruction {
            program_id: protocol::PUMP_FUN,
            accounts: vec![
                AccountMeta::new_readonly(global.global_pda, false),
                AccountMeta::new(global.fee_recipient, false),
                AccountMeta::new(*mint, false),
                AccountMeta::new(pdas.bonding_curve, false),
                AccountMeta::new(pdas.associated_bonding_curve, false),
                AccountMeta::new(*user_token_account, false),
                AccountMeta::new(self.config.signer.pubkey(), true),
                AccountMeta::new_readonly(system_program::id(), false),
                AccountMeta::new_readonly(pdas.token_program, false),
                AccountMeta::new(pdas.creator_vault, false),
                AccountMeta::new_readonly(protocol::EVENT_AUTHORITY, false),
                AccountMeta::new_readonly(protocol::PUMP_FUN, false),
                AccountMeta::new(global.global_volume_accumulator, false),
                AccountMeta::new(global.user_volume_accumulator, false),
                AccountMeta::new_readonly(global.fee_config, false),
                AccountMeta::new_readonly(protocol::FEE_PROGRAM, false),
                AccountMeta::new_readonly(pdas.bonding_curve_v2, false),
                AccountMeta::new(protocol::PUMP_CURVE_FEE_RECIPIENT, false),
            ],
            data: buy_data,
        })
    }
}

// The curve-buy slippage floor is single-sourced in `crate::price`; re-export it
// under the historical name so `super::buy::compute_curve_buy_min_out` (the
// simulate/create/bundle callers) and the tests below resolve unchanged.
pub(super) use crate::price::curve_buy_min_out as compute_curve_buy_min_out;

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
