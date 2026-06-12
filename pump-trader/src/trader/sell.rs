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
    hash::Hash,
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
                    if slippage_bps.is_none() && e.downcast_ref::<OnChainRevert>().is_some() {
                        warn!("⛔ Sell reverted on-chain; not retrying (would only re-pay fees)");
                        return Err(e);
                    }
                    last_err = Some(e);
                }
            }

            if attempt < MAX_SELL_ATTEMPTS - 1 {
                tokio::time::sleep(Duration::from_millis(250)).await;
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
        // Ensure PDAs are cached (reads the bonding-curve PDA on a miss).
        if !self.token_pdas.lock().unwrap().contains_key(token_mint) {
            self.get_creator_from_mint_pda(token_mint).await?;
        }

        // Ensure the wallet's token account is known: an explicit override wins;
        // otherwise resolve cache-first (one wallet scan on a miss). `execute_sell`
        // reads the cached account, so populate it here.
        if let Some(token_account) = token_account_override {
            let pk = Pubkey::from_str(token_account).context("Invalid token_account_override")?;
            self.user_token_accounts
                .lock()
                .unwrap()
                .insert(token_mint.to_string(), pk);
        } else if self.resolve_cached_token_account(token_mint).await?.is_none() {
            anyhow::bail!("No token account found for mint {token_mint}");
        }

        let (nonce_pubkey, nonce_hash) = self.acquire_nonce().await?;
        info!("🔁 Sell — token: {} nonce: {}", token_mint, nonce_pubkey);

        let res = self
            .execute_sell(
                token_mint,
                token_amount,
                creator_override,
                is_cashback,
                slippage_bps,
                &nonce_pubkey,
                nonce_hash,
                tip_level,
                confirm,
            )
            .await;

        self.schedule_nonce_refresh(nonce_pubkey);
        res
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
        nonce_pubkey: &Pubkey,
        nonce_hash: Hash,
        tip_level: u8,
        confirm: bool,
    ) -> Result<bool> {
        let t0 = Instant::now();
        let keypair = &self.config.keypair;

        let mint = Pubkey::from_str(token_mint)?;

        // Read cached PDAs (must exist from buy)
        let mut pdas = self
            .token_pdas
            .lock()
            .unwrap()
            .get(token_mint)
            .copied()
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
            .lock()
            .unwrap()
            .get(token_mint)
            .copied()
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
                    let gross =
                        vq.saturating_mul(token_amount as u128) / (vt + token_amount as u128);
                    let net = gross.saturating_mul(10_000 - CURVE_FEE_BUFFER_BPS) / 10_000;
                    ((net * 10_000u128.saturating_sub(slip as u128) / 10_000) as u64).max(1)
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

        // ── Sign & send ─────────────────────────────────────────────────────
        let tx = self.build_nonce_tx(ixs, nonce_pubkey, nonce_hash, keypair)?;
        let sig = self.send_transaction(&tx).await?;

        info!(
            "📤 Sell sent — sig: {} | amount: {} | {}ms",
            sig,
            token_amount,
            t0.elapsed().as_millis()
        );

        // Feed-confirm path (`confirm == false`): the caller watches the
        // LaserStream-fed `trades` balance, so blocking on the RPC poll here just
        // adds 1–4 s of latency before that poll can even start. Manual/API
        // callers keep the RPC confirm (and its `OnChainRevert` budget signal).
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

    /// Assemble the curve-sell instruction set (compute budget + `sell` + Jito
    /// tip) for a known mint/account. Pure tx construction, no RPC or signing —
    /// extracted from `execute_sell` so the simulate probe builds the *identical*
    /// instructions the live sell path sends. `tip_level` escalates the tip (see
    /// `jito_tip`); `min_sol_output` is the slippage floor (1 = no protection).
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

        let mut ixs = Vec::with_capacity(5);
        ixs.extend_from_slice(&self.cu_ixs_curve_sell);

        // Sell (Sell exact token in)
        let mut sell_data = vec![0x33, 0xe6, 0x85, 0xa4, 0x01, 0x7f, 0x83, 0xad];
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
