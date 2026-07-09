// ============================================================
// Sell — hot path.
//
// `sell_token` is the public entry: it ensures PDAs and the user
// token account are cached (override → cache → on-chain lookup),
// then retries `execute_sell` up to MAX_SELL_ATTEMPTS with a fresh
// nonce each time. `execute_sell` is the single-attempt inner that
// assembles, signs, sends, and confirms one sell tx.
// ============================================================

use executor_core::{classify_swap_revert, SwapDirection, SwapRetryDecision, SwapRoute};
use super::{PumpFunTrader, TokenPDAs};
use crate::error::{bail, Context, Result, TradeError};
use crate::protocol;
use crate::types::TokenProgram;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    system_program,
};
use std::str::FromStr;
use std::time::{Duration, Instant};
use tracing::{error, info, warn};

impl PumpFunTrader {
    /// Manual/standalone curve sell: retries [`sell_token_once`] up to
    /// `MAX_SELL_ATTEMPTS` (fresh nonce each time). Callers that own their own
    /// retry/partial-fill loop (the TPSL service) should call `sell_token_once`
    /// directly so the two retry loops don't multiply.
    pub async fn sell_token(
        &self,
        token_mint: &str,
        token_amount: u64,
        creator_override: Option<&str>,
        is_cashback: bool,
        token_account_override: Option<&str>,
        slippage_bps: Option<u64>,
    ) -> Result<bool> {
        // Brief pause before resending only after a genuine network/transport
        // fault — never after a lost auction or a transient min-out revert, which
        // resend immediately (see the retry loop below).
        const SELL_NETWORK_RETRY_BACKOFF: Duration = Duration::from_millis(250);

        let max_sell_attempts = self.config.retry.max_sell_attempts;
        let mut last_err: Option<TradeError> = None;

        for attempt in 0..max_sell_attempts {
            match self
                .sell_token_once(
                    token_mint,
                    token_amount,
                    creator_override,
                    is_cashback,
                    token_account_override,
                    slippage_bps,
                    // Escalate the Jito tip each attempt: a sell that lost the
                    // auction just didn't land (costs nothing), so bid up to win
                    // the next block instead of re-sending the same losing tip.
                    attempt as u8,
                    // Manual/standalone path: RPC-confirm each attempt so the
                    // `OnChainRevert` budget guard below can stop re-paying fees.
                    true,
                )
                .await
            {
                // `Some(sig)` == submitted; the manual path only needs the bool.
                Ok(Some(_)) => return Ok(true),
                Ok(None) => {
                    // Lost the Jito auction / the tx didn't land — it cost nothing,
                    // so don't sit on a fixed backoff while the token dumps. Loop
                    // straight into the next attempt, which escalates the tip and
                    // takes a fresh nonce.
                    let e = TradeError::Other(format!("Sell returned false on attempt {}", attempt + 1));
                    error!("❌ {}", e);
                    last_err = Some(e);
                }
                Err(e) => {
                    error!("❌ Sell attempt {} failed: {}", attempt + 1, e);
                    // Budget guard: if the tx LANDED and reverted on the
                    // min_out=1 path, it failed for a structural reason (already
                    // sold, empty/wrong token account, token migrated) that a
                    // blind re-send can't fix — and each landed-revert re-pays
                    // base + priority fee. Stop now. (With slippage set, a revert
                    // may be a transient min-out miss that next attempt's
                    // fresh-reserve recompute clears, so those still retry.)
                    let landed_revert = matches!(e, TradeError::Reverted { .. });
                    if slippage_bps.is_none() && landed_revert {
                        warn!("⛔ Sell reverted on-chain; not retrying (would only re-pay fees)");
                        return Err(e);
                    }
                    last_err = Some(e);
                    // A landed min-out revert (slippage set) clears on the next
                    // attempt's fresh-reserve recompute, so resend it immediately.
                    // Only a genuine network/transport fault warrants a brief backoff
                    // before resending, so we don't hammer a degraded endpoint.
                    if !landed_revert && attempt < max_sell_attempts - 1 {
                        tokio::time::sleep(SELL_NETWORK_RETRY_BACKOFF).await;
                    }
                }
            }
        }

        Err(last_err.unwrap_or_else(|| {
            TradeError::Other(format!("Sell failed after {max_sell_attempts} attempts"))
        }))
    }

    /// One curve-sell attempt: ensure the mint's PDAs and the wallet's token
    /// account are known (cache-first, single on-chain read on a miss), acquire a
    /// fresh nonce, then build/send/confirm exactly one sell tx. Public so an
    /// outer orchestrator (TPSL `sell_with_retries`, which already re-reads the
    /// balance and re-routes on migration each pass) can drive retries without
    /// the redundant inner loop `sell_token` keeps for manual callers.
    /// `tip_level` escalates the Jito tip on retries (see `jito_tip`).
    /// `confirm` selects the confirmation source: `true` blocks on the RPC
    /// signature-status poll (manual/API callers, which also want the
    /// `OnChainRevert` budget-guard signal); `false` returns as soon as the
    /// sender accepts the tx and leaves confirmation to the caller's own feed —
    /// the live TPSL loop already polls the LaserStream-fed `trades` balance, so
    /// the inner 1s RPC poll is pure redundant latency there.
    ///
    /// Returns the submitted transaction signature (`Some(sig)`) so a feed-confirm
    /// caller can run an off-path `signature_state` classification after its own
    /// poll window — a landed-and-reverted sell (min_out=1 ⇒ structural) never
    /// clears and re-pays fees on every blind retry, so the caller needs the sig
    /// to detect it. `None` is the (currently unreachable) not-submitted case.
    #[allow(clippy::too_many_arguments)]
    pub async fn sell_token_once(
        &self,
        token_mint: &str,
        token_amount: u64,
        creator_override: Option<&str>,
        is_cashback: bool,
        token_account_override: Option<&str>,
        slippage_bps: Option<u64>,
        tip_level: u8,
        confirm: bool,
    ) -> Result<Option<String>> {
        self.sell_token_once_inner(
            token_mint,
            token_amount,
            creator_override,
            is_cashback,
            token_account_override,
            slippage_bps,
            tip_level,
            confirm,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn sell_token_once_inner(
        &self,
        token_mint: &str,
        token_amount: u64,
        creator_override: Option<&str>,
        is_cashback: bool,
        token_account_override: Option<&str>,
        slippage_bps: Option<u64>,
        tip_level: u8,
        confirm: bool,
    ) -> Result<Option<String>> {
        // Ensure PDAs are cached (reads the bonding-curve PDA on a miss). Use the
        // String-free warm helper — this call only populates `token_pdas` and
        // discards the creator, so `get_creator_from_mint_pda`'s `.to_string()`
        // would be wasted here.
        if !self.token_pdas.contains_key(token_mint) {
            self.ensure_token_pdas(token_mint).await?;
        }

        // Ensure the wallet's token account is known: an explicit override wins;
        // otherwise resolve cache-first (one wallet scan on a miss). `execute_sell`
        // reads the cached account, so populate it here.
        if let Some(token_account) = token_account_override {
            let pk = Pubkey::from_str(token_account).context("Invalid token_account_override")?;
            self.user_token_accounts
                .insert(token_mint.to_string(), pk);
        } else if self.resolve_cached_token_account(token_mint).await?.is_none() {
            bail!("No token account found for mint {token_mint}");
        }

        // `execute_sell` acquires the nonce itself, *after* its account/reserve
        // reads, so the slot isn't held `in_use` across those RPCs.
        self.execute_sell(
            token_mint,
            token_amount,
            creator_override,
            is_cashback,
            slippage_bps,
            tip_level,
            confirm,
        )
        .await
    }

    /// Fire-and-forget rent reclaim: close the wallet's (already-emptied) token
    /// account for `token_mint`, returning its ~0.002 SOL rent to the wallet.
    /// OFF the exit hot path — uses a RECENT BLOCKHASH (no durable nonce slot),
    /// NO Jito tip, and does NOT confirm. SPL `close_account` reverts if the
    /// account still holds any balance, so this is only meaningful after the
    /// caller has confirmed the balance is cleared; preflight is left ENABLED so a
    /// still-funded (dust) account fails cheaply in simulation without paying a
    /// landed-revert fee. Errors are returned for the caller to log and ignore.
    pub async fn close_token_account(
        &self,
        token_mint: &str,
        token_account_override: Option<&str>,
    ) -> Result<()> {
        // Resolve the wallet's token account: explicit override wins (and warms
        // the cache like `sell_token_once_inner`); otherwise resolve cache-first
        // (one wallet scan on a miss). No account → nothing to close.
        let user_token_account = if let Some(token_account) = token_account_override {
            let pk = Pubkey::from_str(token_account).context("Invalid token_account_override")?;
            self.user_token_accounts.insert(token_mint.to_string(), pk);
            pk
        } else {
            match self.resolve_cached_token_account(token_mint).await? {
                Some(pk) => pk,
                None => bail!("No token account found for mint {token_mint}; nothing to close"),
            }
        };

        // Determine the token program (legacy SPL vs Token-2022): ensure the
        // mint's PDAs are cached (one bonding-curve read on a miss), then read the
        // cached `token_program` to route the close builder.
        if !self.token_pdas.contains_key(token_mint) {
            self.ensure_token_pdas(token_mint).await?;
        }
        let pdas = self
            .token_pdas
            .get(token_mint)
            .map(|r| *r)
            .context("Token PDAs not cached after ensure")?;

        // Single `close_account` ix, routed by token program through the shared
        // `TokenProgram` classifier so this path uses the *same* legacy-vs-2022
        // convention (`!= TOKEN_PROGRAM_ID ⇒ 2022`) as `from_id`/`from_pubkey`
        // everywhere else — owner == destination == wallet, returning the rent to
        // the wallet; empty signer slice (the fee-payer signs).
        let owner = self.config.signer.pubkey();
        let close_ix = match TokenProgram::from_pubkey(&pdas.token_program) {
            TokenProgram::Token2022 => spl_token_2022::instruction::close_account(
                &pdas.token_program,
                &user_token_account,
                &owner,
                &owner,
                &[],
            )?,
            TokenProgram::Legacy => spl_token::instruction::close_account(
                &pdas.token_program,
                &user_token_account,
                &owner,
                &owner,
                &[],
            )?,
        };

        // RECENT BLOCKHASH, no durable nonce — this runs off the exit hot path so
        // it never competes for a nonce slot. No Jito tip ix is appended.
        let tx = self
            .build_recent_tx(vec![close_ix], self.config.signer.as_ref())
            .await?;

        // Send via the RPC `sendTransaction` path with PREFLIGHT ENABLED (the
        // nonblocking client's default: skip_preflight = false) so a still-funded
        // account is rejected in simulation without paying a landed-revert fee.
        // NOT the Jito sender fan-out, and we do NOT block on confirmation.
        self.rpc
            .send_transaction(&tx)
            .await
            .context("close_account sendTransaction")?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Sell inner — one attempt
    // -----------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    async fn execute_sell(
        &self,
        token_mint: &str,
        token_amount: u64,
        creator_override: Option<&str>,
        is_cashback: bool,
        slippage_bps: Option<u64>,
        tip_level: u8,
        confirm: bool,
    ) -> Result<Option<String>> {
        let mint = Pubkey::from_str(token_mint)?;
        let signer = self.config.signer.as_ref();

        // At most one self-heal resend: a `confirm=true` caller synchronously
        // learns a stale-creator `ConstraintSeeds` (2006) revert (`tx.rs`'s
        // `confirm_transaction` surfaces it before returning); if the freshly
        // re-read `creator_vault` actually changed, rebuild with a fresh nonce
        // and resend once — a confirmed revert bought/sold nothing, so this
        // can't double-spend. `confirm=false` callers never see the error here
        // (their own feed classifies it off-path — see the bot exit loop's
        // `RefreshCreator` branch, which shares this same decision).
        let mut healed = false;
        let mut tip_level = tip_level;

        loop {
            let t0 = Instant::now();

            // Read cached PDAs (must exist from buy). On the healed resend this
            // reads the vault `refresh_curve_creator_vault` just wrote below.
            let mut pdas = self
                .token_pdas
                .get(token_mint)
                .map(|r| *r)
                .context("Token PDAs not cached — buy must precede sell")?;

            // Allow caller to override creator vault (e.g. after a creator
            // update) — but not on the healed resend, where the just-refreshed
            // cache IS the fresh creator and a caller-supplied override (from
            // the same stale routing read that caused the revert) must not
            // shadow it back to the stale vault.
            if !healed {
                if let Some(creator_str) = creator_override {
                    let creator = Pubkey::from_str(creator_str)?;
                    let (vault, _) = Pubkey::find_program_address(
                        &[b"creator-vault", creator.as_ref()],
                        &protocol::PUMP_FUN,
                    );
                    if pdas.creator_vault != vault {
                        info!(
                            "🔄 Creator vault updated: {} → {}",
                            pdas.creator_vault, vault
                        );
                        pdas.creator_vault = vault;
                    }
                }
            }

            // Read cached user token account (must exist from buy)
            let user_token_account = self
                .user_token_accounts
                .get(token_mint)
                .map(|r| *r)
                .context("User token account not cached — buy must precede sell")?;
            warn!(
                user_token_account = user_token_account.to_string(),
                "=== Using cached user token account for sell"
            );

            // `sell(amount, min_sol_output)`: slippage floor on SOL received. `None`
            // keeps the legacy min_out=1 (snipe path, no extra RPC); `Some` reads the
            // curve's virtual reserves for a conservative lower bound, falling back to
            // 1 if the read fails so slippage never blocks a sell.
            let reserves: Option<(u128, u128)> = match slippage_bps {
                Some(_) => match self.curve_reserves(token_mint, &pdas.bonding_curve).await {
                    Ok(r) => Some(r),
                    Err(e) => {
                        warn!("curve sell slippage: reserve read failed ({e}); using min_out=1");
                        None
                    }
                },
                None => None,
            };
            let min_sol_output = compute_curve_sell_min_out(
                token_amount,
                slippage_bps,
                reserves,
                self.config.slippage.curve_fee_buffer_bps,
            );

            let ixs = self.build_curve_sell_ixs(
                &mint,
                &pdas,
                &user_token_account,
                token_amount,
                min_sol_output,
                is_cashback,
                tip_level,
            )?;

            // ── Acquire nonce + sign & send ─────────────────────────────────
            // Acquire the nonce only now — after the cached-PDA / token-account
            // reads and the slippage `curve_reserves` read above — so the slot
            // isn't held `in_use` across any RPC. Only the build/send/confirm
            // below runs while holding it, and we always fall through to
            // `schedule_nonce_refresh`.
            let (nonce_pubkey, nonce_hash) = self.acquire_nonce().await?;
            info!("🔁 Sell — token: {} nonce: {}", token_mint, nonce_pubkey);

            let res: Result<Option<String>> = async {
                let tx = self.build_nonce_tx(ixs, &nonce_pubkey, nonce_hash, signer)?;
                let sig = self.send_transaction(&tx).await?;

                info!(
                    "📤 Sell sent — sig: {} | amount: {} | {}ms",
                    sig,
                    token_amount,
                    t0.elapsed().as_millis()
                );

                // Feed-confirm path (`confirm == false`): the caller watches the
                // LaserStream-fed `trades` balance, so blocking on the RPC poll here
                // just adds 1–4 s of latency before that poll can even start.
                // Manual/API callers keep the RPC confirm (and its `OnChainRevert`
                // budget signal).
                if confirm {
                    self.confirm_transaction(&sig, self.config.retry.confirm_max_retries)
                        .await?;
                    info!(
                        "✅ Sell confirmed — sig: {} | {}ms",
                        sig,
                        t0.elapsed().as_millis()
                    );
                }
                // Hand the signature back so a feed-confirm caller can classify a
                // landed-revert off its own poll window (see `sell_token_once`).
                Ok(Some(sig))
            }
            .await;

            self.schedule_nonce_refresh(nonce_pubkey);

            if let Err(TradeError::Reverted { custom }) = &res {
                if !healed
                    && classify_swap_revert(*custom, SwapRoute::Curve, SwapDirection::Sell)
                        == SwapRetryDecision::RefreshCreator
                {
                    match self.refresh_curve_creator_vault(token_mint).await {
                        Ok(Some(vault)) => {
                            info!(
                                "🔄 Sell reverted on a stale creator_vault (pump set_creator); \
                                 refreshed to {vault}, resending once"
                            );
                            healed = true;
                            tip_level = tip_level.saturating_add(1);
                            continue;
                        }
                        // Unchanged creator (the retry would derive the identical
                        // vault) or the refresh itself failed — stop rather than
                        // re-pay fees on a resend that can't fix anything.
                        Ok(None) | Err(_) => return res,
                    }
                }
            }
            return res;
        }
    }

    /// Assemble the curve-sell instruction set (compute budget + `sell` +
    /// optional `close_account` + Jito tip) for a known mint/account. Pure tx
    /// construction, no RPC or signing — extracted from `execute_sell` so the
    /// simulate probe builds the *identical* instructions the live sell path
    /// sends. `tip_level` escalates the tip (see `jito_tip`); `min_sol_output` is
    /// the slippage floor (1 = no protection). Token-account rent is recovered
    /// off this path by a separate post-clear [`Self::close_token_account`] tx,
    /// not by appending a close here — bundling it would revert the whole sell if
    /// any dust remained.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_curve_sell_ixs(
        &self,
        mint: &Pubkey,
        pdas: &TokenPDAs,
        user_token_account: &Pubkey,
        token_amount: u64,
        min_sol_output: u64,
        is_cashback: bool,
        tip_level: u8,
    ) -> Result<Vec<Instruction>> {
        let global = self.global_account.as_ref().context("Not initialized")?;

        let mut ixs = Vec::with_capacity(6);
        ixs.extend_from_slice(&self.engine.cu_ixs_curve_sell);

        // Sell (Sell exact token in)
        // 8-byte discriminator + two u64 args: size up front so the two extends
        // below don't reallocate on the sell hot path.
        let mut sell_data = Vec::with_capacity(24);
        sell_data.extend_from_slice(&[0x33, 0xe6, 0x85, 0xa4, 0x01, 0x7f, 0x83, 0xad]);
        sell_data.extend_from_slice(&token_amount.to_le_bytes());
        sell_data.extend_from_slice(&min_sol_output.to_le_bytes()); // min_sol_output (slippage floor)

        let mut accounts = vec![
            AccountMeta::new_readonly(global.global_pda, false),
            AccountMeta::new(global.fee_recipient, false),
            AccountMeta::new_readonly(*mint, false),
            AccountMeta::new(pdas.bonding_curve, false),
            AccountMeta::new(pdas.associated_bonding_curve, false),
            AccountMeta::new(*user_token_account, false),
            AccountMeta::new(self.config.signer.pubkey(), true),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new(pdas.creator_vault, false),
            AccountMeta::new_readonly(pdas.token_program, false),
            AccountMeta::new_readonly(protocol::EVENT_AUTHORITY, false),
            AccountMeta::new_readonly(protocol::PUMP_FUN, false),
            AccountMeta::new_readonly(global.fee_config, false),
            AccountMeta::new_readonly(protocol::FEE_PROGRAM, false),
        ];

        // Include the cashback account if the caller knows the token is
        // cashback-enabled, or if the chain-read PDA flag says so. The caller
        // flag matters because `buy_token` caches `cashback_enabled: false`,
        // so a buy→sell flow would otherwise never include this account.
        if is_cashback || pdas.cashback_enabled {
            accounts.push(AccountMeta::new(global.user_volume_accumulator, false));
        }

        accounts.push(AccountMeta::new_readonly(pdas.bonding_curve_v2, false));
        accounts.push(AccountMeta::new(protocol::PUMP_CURVE_FEE_RECIPIENT, false));

        ixs.push(Instruction {
            program_id: protocol::PUMP_FUN,
            accounts,
            data: sell_data,
        });

        // Jito tip — escalated by the caller's retry level.
        ixs.push(self.jito_tip_ix(tip_level));

        Ok(ixs)
    }
}

/// Curve-sell slippage floor: the minimum lamports the `sell` must return or
/// revert. Pure (no RPC) so both the live sell path and the simulate path derive
/// the floor identically (mirrors [`super::buy::compute_curve_buy_min_out`]).
/// `None` slippage (or a missing/zero reserve read) yields `1` — no protection —
/// so slippage never blocks a sell. `reserves` is the curve's virtual
/// `(token, quote=lamports)` reserves; the math mirrors the on-chain
/// constant-product fill, net of the curve fee buffer.
pub(super) fn compute_curve_sell_min_out(
    token_amount: u64,
    slippage_bps: Option<u64>,
    reserves: Option<(u128, u128)>,
    curve_fee_buffer_bps: u128,
) -> u64 {
    match (slippage_bps, reserves) {
        (Some(slip), Some((vt, vq))) => {
            // Saturating throughout on untrusted reserve reads; a zero
            // denominator falls back to the unprotected min_out=1.
            let denom = vt.saturating_add(token_amount as u128);
            if denom == 0 {
                1
            } else {
                let gross = vq.saturating_mul(token_amount as u128) / denom;
                let net = gross.saturating_mul(10_000u128.saturating_sub(curve_fee_buffer_bps))
                    / 10_000;
                ((net.saturating_mul(10_000u128.saturating_sub(slip as u128)) / 10_000) as u64)
                    .max(1)
            }
        }
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::compute_curve_sell_min_out;
    use crate::trader::{GlobalAccount, PumpFunTrader, TokenPDAs, TraderConfig};
    use solana_sdk::{message::Message, pubkey::Pubkey, signature::Keypair};
    use std::sync::Arc;

    /// Hard Solana transaction wire-size limit (bytes).
    const TX_LIMIT: usize = 1232;
    /// Default `config.slippage.curve_fee_buffer_bps`.
    const FEE_BUF: u128 = 200;

    fn dummy_trader() -> PumpFunTrader {
        let mut t = PumpFunTrader::new(Arc::new(TraderConfig::new(
            "http://localhost".into(),
            vec!["http://localhost".into()],
            Arc::new(Keypair::new()),
            vec![Pubkey::new_unique()],
        )));
        // `build_curve_sell_ixs` needs the global account + compute-budget ixs
        // that `initialize()` would populate; set the minimum it reads here.
        t.global_account = Some(GlobalAccount {
            global_pda: Pubkey::new_unique(),
            fee_recipient: Pubkey::new_unique(),
            global_volume_accumulator: Pubkey::new_unique(),
            user_volume_accumulator: Pubkey::new_unique(),
            fee_config: Pubkey::new_unique(),
            stable_quote_mint: None,
        });
        t.engine.cu_ixs_curve_sell = vec![
            solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(100_000),
            solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_price(1),
        ];
        t
    }

    #[test]
    fn sell_min_out_is_unprotected_without_slippage_or_reserves() {
        assert_eq!(compute_curve_sell_min_out(1_000_000, None, Some((1_000, 2_000)), FEE_BUF), 1);
        assert_eq!(compute_curve_sell_min_out(1_000_000, Some(500), None, FEE_BUF), 1);
        assert_eq!(compute_curve_sell_min_out(1_000_000, Some(500), Some((0, 0)), FEE_BUF), 1);
    }

    #[test]
    fn sell_tighter_slippage_raises_the_floor() {
        let reserves = Some((1_000_000_000u128, 30_000_000u128));
        let loose = compute_curve_sell_min_out(1_000_000, Some(5_000), reserves, FEE_BUF); // 50%
        let tight = compute_curve_sell_min_out(1_000_000, Some(100), reserves, FEE_BUF); // 1%
        assert!(tight >= loose, "tighter slippage must demand at least as many lamports");
        assert!(loose >= 1 && tight >= 1, "floor is always >= 1");
    }

    /// Worst-case PDAs for tx size: cashback-enabled (adds the
    /// user_volume_accumulator account). `token_program` must be a real SPL token
    /// program id (legacy here) because `close_account` validates it; both program
    /// ids are 32 bytes, so the wire size is identical either way.
    fn worst_case_pdas() -> TokenPDAs {
        TokenPDAs {
            token_program: spl_token::id(),
            bonding_curve: Pubkey::new_unique(),
            bonding_curve_v2: Pubkey::new_unique(),
            associated_bonding_curve: Pubkey::new_unique(),
            creator_vault: Pubkey::new_unique(),
            cashback_enabled: true,
        }
    }

    fn wire_size(msg: &Message) -> usize {
        1 + 64 * msg.header.num_required_signatures as usize + msg.serialize().len()
    }

    /// The worst-case curve sell (compute budget, `sell`, Jito tip), wrapped in a
    /// durable-nonce message, must stay under the 1232 B wire limit.
    #[test]
    fn curve_sell_with_nonce_fits() {
        let t = dummy_trader();
        let user = t.config.signer.pubkey();
        let mint = Pubkey::new_unique();
        let pdas = worst_case_pdas();
        let uta = Pubkey::new_unique();
        let nonce = Pubkey::new_unique();

        let ixs = t
            .build_curve_sell_ixs(&mint, &pdas, &uta, 1_000_000, 1, true, 0)
            .expect("build curve sell");
        let msg = Message::new_with_nonce(ixs, Some(&user), &nonce, &user);
        let size = wire_size(&msg);
        eprintln!("worst-case curve sell + nonce = {size} B (limit {TX_LIMIT})");
        assert!(
            size <= TX_LIMIT,
            "curve sell + nonce = {size} B (limit {TX_LIMIT})"
        );
    }

    /// The standalone rent-reclaim close (single `close_account`, recent
    /// blockhash, no nonce / no tip) is trivially under the wire limit. Guards
    /// that the lone-ix close tx builds and stays sane.
    #[test]
    fn standalone_close_tx_fits() {
        let t = dummy_trader();
        let owner = t.config.signer.pubkey();
        let uta = Pubkey::new_unique();
        let close_ix = spl_token::instruction::close_account(
            &spl_token::id(),
            &uta,
            &owner,
            &owner,
            &[],
        )
        .expect("build close ix");
        let msg = Message::new(&[close_ix], Some(&owner));
        let size = wire_size(&msg);
        eprintln!("standalone close tx = {size} B (limit {TX_LIMIT})");
        assert!(size <= TX_LIMIT, "standalone close = {size} B (limit {TX_LIMIT})");
    }
}
