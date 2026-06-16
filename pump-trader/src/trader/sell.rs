// ============================================================
// Sell — hot path.
//
// `sell_token` is the public entry: it ensures PDAs and the user
// token account are cached (override → cache → on-chain lookup),
// then retries `execute_sell` up to MAX_SELL_ATTEMPTS with a fresh
// nonce each time. `execute_sell` is the single-attempt inner that
// assembles, signs, sends, and confirms one sell tx.
// ============================================================

use super::tx::OnChainRevert;
use super::{PumpFunTrader, TokenPDAs};
use crate::constants::{CONFIRM_MAX_RETRIES, CURVE_FEE_BUFFER_BPS, MAX_SELL_ATTEMPTS};
use anyhow::{Context, Result};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::Signer,
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

        let mut last_err: Option<anyhow::Error> = None;

        for attempt in 0..MAX_SELL_ATTEMPTS {
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
                Ok(true) => return Ok(true),
                Ok(false) => {
                    // Lost the Jito auction / the tx didn't land — it cost nothing,
                    // so don't sit on a fixed backoff while the token dumps. Loop
                    // straight into the next attempt, which escalates the tip and
                    // takes a fresh nonce.
                    let e = anyhow::anyhow!("Sell returned false on attempt {}", attempt + 1);
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
                    let landed_revert = e.downcast_ref::<OnChainRevert>().is_some();
                    if slippage_bps.is_none() && landed_revert {
                        warn!("⛔ Sell reverted on-chain; not retrying (would only re-pay fees)");
                        return Err(e);
                    }
                    last_err = Some(e);
                    // A landed min-out revert (slippage set) clears on the next
                    // attempt's fresh-reserve recompute, so resend it immediately.
                    // Only a genuine network/transport fault warrants a brief backoff
                    // before resending, so we don't hammer a degraded endpoint.
                    if !landed_revert && attempt < MAX_SELL_ATTEMPTS - 1 {
                        tokio::time::sleep(SELL_NETWORK_RETRY_BACKOFF).await;
                    }
                }
            }
        }

        Err(last_err
            .unwrap_or_else(|| anyhow::anyhow!("Sell failed after {} attempts", MAX_SELL_ATTEMPTS)))
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
    ) -> Result<bool> {
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
    ) -> Result<bool> {
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
            anyhow::bail!("No token account found for mint {token_mint}");
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
                None => anyhow::bail!("No token account found for mint {token_mint}; nothing to close"),
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

        // Single `close_account` ix, routed by token program exactly like the
        // terminal clearing sell does — owner == destination == wallet, returning
        // the rent to the wallet; empty signer slice (the fee-payer signs).
        let owner = self.config.keypair.pubkey();
        let close_ix = if pdas.token_program == spl_token_2022::id() {
            spl_token_2022::instruction::close_account(
                &pdas.token_program,
                &user_token_account,
                &owner,
                &owner,
                &[],
            )?
        } else {
            spl_token::instruction::close_account(
                &pdas.token_program,
                &user_token_account,
                &owner,
                &owner,
                &[],
            )?
        };

        // RECENT BLOCKHASH, no durable nonce — this runs off the exit hot path so
        // it never competes for a nonce slot. No Jito tip ix is appended.
        let tx = self
            .build_recent_tx(vec![close_ix], &self.config.keypair)
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
    ) -> Result<bool> {
        let t0 = Instant::now();
        let keypair = &self.config.keypair;

        let mint = Pubkey::from_str(token_mint)?;

        // Read cached PDAs (must exist from buy)
        let mut pdas = self
            .token_pdas
            .get(token_mint)
            .map(|r| *r)
            .context("Token PDAs not cached — buy must precede sell")?;

        // Allow caller to override creator vault (e.g. after a creator update)
        if let Some(creator_str) = creator_override {
            let creator = Pubkey::from_str(creator_str)?;
            let (vault, _) = Pubkey::find_program_address(
                &[b"creator-vault", creator.as_ref()],
                &self.pump_program,
            );
            if pdas.creator_vault != vault {
                info!(
                    "🔄 Creator vault updated: {} → {}",
                    pdas.creator_vault, vault
                );
                pdas.creator_vault = vault;
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
        let min_sol_output: u64 = match slippage_bps {
            Some(slip) => match self.curve_reserves(token_mint, &pdas.bonding_curve).await {
                Ok((vt, vq)) => {
                    // Saturating throughout on untrusted reserve reads; a zero
                    // denominator falls back to the unprotected min_out=1.
                    let denom = vt.saturating_add(token_amount as u128);
                    if denom == 0 {
                        1
                    } else {
                        let gross = vq.saturating_mul(token_amount as u128) / denom;
                        let net =
                            gross.saturating_mul(10_000u128.saturating_sub(CURVE_FEE_BUFFER_BPS))
                                / 10_000;
                        ((net.saturating_mul(10_000u128.saturating_sub(slip as u128)) / 10_000)
                            as u64)
                            .max(1)
                    }
                }
                Err(e) => {
                    warn!("curve sell slippage: reserve read failed ({e}); using min_out=1");
                    1
                }
            },
            None => 1,
        };

        let ixs = self.build_curve_sell_ixs(
            &mint,
            &pdas,
            &user_token_account,
            token_amount,
            min_sol_output,
            is_cashback,
            tip_level,
        )?;

        // ── Acquire nonce + sign & send ─────────────────────────────────────
        // Acquire the nonce only now — after the cached-PDA / token-account reads
        // and the slippage `curve_reserves` read above — so the slot isn't held
        // `in_use` across any RPC. Only the build/send/confirm below runs while
        // holding it, and we always fall through to `schedule_nonce_refresh`.
        let (nonce_pubkey, nonce_hash) = self.acquire_nonce().await?;
        info!("🔁 Sell — token: {} nonce: {}", token_mint, nonce_pubkey);

        let res: Result<bool> = async {
            let tx = self.build_nonce_tx(ixs, &nonce_pubkey, nonce_hash, keypair)?;
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
                self.confirm_transaction(&sig, CONFIRM_MAX_RETRIES).await?;
                info!(
                    "✅ Sell confirmed — sig: {} | {}ms",
                    sig,
                    t0.elapsed().as_millis()
                );
            }
            Ok(true)
        }
        .await;

        self.schedule_nonce_refresh(nonce_pubkey);
        res
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
        ixs.extend_from_slice(&self.cu_ixs_curve_sell);

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
            AccountMeta::new(self.config.keypair.pubkey(), true),
            AccountMeta::new_readonly(self.system_program, false),
            AccountMeta::new(pdas.creator_vault, false),
            AccountMeta::new_readonly(pdas.token_program, false),
            AccountMeta::new_readonly(self.event_authority, false),
            AccountMeta::new_readonly(self.pump_program, false),
            AccountMeta::new_readonly(global.fee_config, false),
            AccountMeta::new_readonly(self.fee_program, false),
        ];

        // Include the cashback account if the caller knows the token is
        // cashback-enabled, or if the chain-read PDA flag says so. The caller
        // flag matters because `buy_token` caches `cashback_enabled: false`,
        // so a buy→sell flow would otherwise never include this account.
        if is_cashback || pdas.cashback_enabled {
            accounts.push(AccountMeta::new(global.user_volume_accumulator, false));
        }

        accounts.push(AccountMeta::new_readonly(pdas.bonding_curve_v2, false));
        accounts.push(AccountMeta::new(self.upgrade_fee_recipient, false));

        ixs.push(Instruction {
            program_id: self.pump_program,
            accounts,
            data: sell_data,
        });

        // Jito tip — escalated by the caller's retry level.
        ixs.push(self.jito_tip_ix(tip_level));

        Ok(ixs)
    }
}

#[cfg(test)]
mod tests {
    use crate::trader::{GlobalAccount, PumpFunTrader, TokenPDAs, TraderConfig};
    use solana_sdk::{
        message::Message, pubkey::Pubkey, signature::Keypair, signature::Signer,
    };
    use std::sync::Arc;

    /// Hard Solana transaction wire-size limit (bytes).
    const TX_LIMIT: usize = 1232;

    fn dummy_trader() -> PumpFunTrader {
        let mut t = PumpFunTrader::new(Arc::new(TraderConfig {
            rpc_url: "http://localhost".into(),
            helius_sender_urls: vec!["http://localhost".into()],
            keypair: Keypair::new(),
            nonce_accounts: vec![Pubkey::new_unique().to_string()],
        }));
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
        t.cu_ixs_curve_sell = vec![
            solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(100_000),
            solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_price(1),
        ];
        t
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
        let user = t.config.keypair.pubkey();
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
        let owner = t.config.keypair.pubkey();
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
