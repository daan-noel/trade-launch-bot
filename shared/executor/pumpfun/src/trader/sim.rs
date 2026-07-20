// ============================================================
// Venue simulation helpers (Layer 1) — build the REAL pump curve/AMM
// ix set (slippage floor from live reserves) and run it through the
// engine's `simulate_ixs` primitive against LIVE chain state with zero
// SOL at risk.
//
//   simulate_curve_buy  / simulate_curve_sell
//   simulate_amm_buy    / simulate_amm_sell
//
// Reuses the SAME instruction builders the live trade path sends
// (`build_curve_buy_ixs` / `build_amm_buy_ixs` / …) so what's simulated
// is what production would send — not a copy that can drift. The Layer-0
// primitive (`simulate_ixs`) + `SimOutcome` / `AccountDelta` live in
// `executor-core`.
//
// OFF THE HOT PATH: each call is one or two RPC round-trips. Never
// invoke inline before a real send (see CLAUDE.md latency budgets).
// ============================================================

use super::PumpFunTrader;
use crate::error::{bail, Context, Result, TradeError};
use crate::protocol;
use executor_core::SimOutcome;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

impl PumpFunTrader {
    /// Layer 1 — simulate the real curve BUY for `token_mint` at `sol_amount`
    /// (SOL) against live curve state. Resolves routing (creator / token program /
    /// curve PDAs), derives the wallet's ATA — prepending an idempotent ATA-create
    /// when the wallet doesn't hold one yet (the live path uses a pooled
    /// create-with-seed account; a plain ATA create is equivalent for a sim and
    /// consumes no template) — derives the slippage `min_tokens_out` from live
    /// reserves exactly as the live buy would, then simulates. The returned
    /// deltas give the SOL spent (payer) and tokens received (ATA) at no cost.
    pub async fn simulate_curve_buy(
        &self,
        token_mint: &str,
        sol_amount: f64,
        slippage_bps: Option<u64>,
    ) -> Result<SimOutcome> {
        // Validate the size with the same guard the real buy uses.
        let buy_lamports = self.buy_lamports_checked(sol_amount)?;

        if !self.token_pdas.contains_key(token_mint) {
            self.ensure_token_pdas(token_mint).await?;
        }
        let mint = Pubkey::from_str(token_mint)?;
        let pdas = self
            .token_pdas
            .get(token_mint)
            .map(|r| *r)
            .context("PDAs not cached")?;

        let wallet = self.config.signer.pubkey();
        let ata = spl_associated_token_account::get_associated_token_address_with_program_id(
            &wallet,
            &mint,
            &pdas.token_program,
        );
        let ata_exists = self.rpc.get_account(&ata).await.is_ok();
        let account_creation_ixs = if ata_exists {
            Vec::new()
        } else {
            vec![
                spl_associated_token_account::instruction::create_associated_token_account_idempotent(
                    &wallet,
                    &wallet,
                    &mint,
                    &pdas.token_program,
                ),
            ]
        };

        // Same slippage floor the live buy would compute from live reserves.
        let reserves = match slippage_bps {
            Some(_) => self.curve_reserves(token_mint, &pdas.bonding_curve).await.ok(),
            None => None,
        };
        let min_tokens_out = super::buy::compute_curve_buy_min_out(
            buy_lamports,
            slippage_bps,
            reserves,
            self.config.slippage.curve_fee_buffer_bps,
        );

        let ixs = self.build_curve_buy_ixs(
            &mint,
            &pdas,
            &ata,
            account_creation_ixs,
            buy_lamports,
            min_tokens_out,
        )?;
        self.simulate_ixs(ixs, &wallet, &[wallet, ata]).await
    }

    /// Layer 1 — simulate the real curve SELL of `token_amount` (raw base units)
    /// of `token_mint` against live curve state. Resolves PDAs + the wallet's
    /// token account (cache-first, same as the live sell), derives the slippage
    /// `min_sol_output` from live reserves exactly as the live sell would, then
    /// simulates the identical sell ix set. Requires the wallet to actually hold
    /// the token (simulating a sell of a balance you don't have reverts). The
    /// returned deltas give the SOL received (payer) and tokens sold (token
    /// account); `custom_error` surfaces a slippage-floor revert code when set.
    pub async fn simulate_curve_sell(
        &self,
        token_mint: &str,
        token_amount: u64,
        slippage_bps: Option<u64>,
        is_cashback: bool,
    ) -> Result<SimOutcome> {
        let owner = self.config.signer.pubkey();
        self.ensure_token_pdas(token_mint).await?;
        if self.resolve_cached_token_account(token_mint).await?.is_none() {
            bail!("No token account cached/found for mint {token_mint}");
        }
        let user_token_account = self
            .user_token_accounts
            .get(token_mint)
            .map(|r| *r)
            .context("token account not cached")?;
        self.simulate_curve_sell_as(
            token_mint,
            token_amount,
            &owner,
            &user_token_account,
            slippage_bps,
            is_cashback,
        )
        .await
    }

    /// Like [`Self::simulate_curve_sell`], but for an arbitrary `owner` /
    /// `token_account` (zero SOL — `sigVerify=false`). Used by probes to dry-run
    /// a sell against a top holder when the configured wallet holds none.
    pub async fn simulate_curve_sell_as(
        &self,
        token_mint: &str,
        token_amount: u64,
        owner: &Pubkey,
        token_account: &Pubkey,
        slippage_bps: Option<u64>,
        is_cashback: bool,
    ) -> Result<SimOutcome> {
        // Always re-derive the creator from chain (don't trust a possibly-stale
        // cached vault): pump.fun can change `bonding_curve.creator` via
        // `set_creator` after a buy cached this mint's PDAs, which makes the live
        // sell revert with Anchor ConstraintSeeds (2006). Forcing a fresh read here
        // (off the hot path — this is a manual probe) means the sim reflects the
        // CURRENT creator, so a simulate-sell that passes proves the live sell would
        // build the correct creator_vault.
        self.ensure_token_pdas(token_mint).await?;
        let mint = Pubkey::from_str(token_mint)?;
        let pdas = self
            .token_pdas
            .get(token_mint)
            .map(|r| *r)
            .context("PDAs not cached")?;

        let reserves = match slippage_bps {
            Some(_) => self.curve_reserves(token_mint, &pdas.bonding_curve).await.ok(),
            None => None,
        };
        let min_sol_output = super::sell::compute_curve_sell_min_out(
            token_amount,
            slippage_bps,
            reserves,
            self.config.slippage.curve_fee_buffer_bps,
        );

        let ixs = self.build_curve_sell_ixs(
            &mint,
            &pdas,
            owner,
            token_account,
            token_amount,
            min_sol_output,
            is_cashback,
            0,
        )?;
        // Fee payer = configured wallet (has SOL). Instruction `user` stays the
        // holder so token-ownership constraints pass; sigVerify=false means the
        // holder need not actually sign. Using the holder as fee payer often
        // hits InvalidAccountForFee (vault PDAs / empty wallets).
        let payer = self.config.signer.pubkey();
        self.simulate_ixs(ixs, &payer, &[payer, *owner, *token_account])
            .await
    }

    /// Layer 1 — simulate the real PumpSwap **AMM** buy of a migrated token,
    /// spending `sol_amount` SOL. Reuses `build_amm_buy_ixs` (pool/config/reserve
    /// reads + slippage floor + WSOL wrap/unwrap) and assembles the same
    /// CU-budget + tip envelope `amm_buy` sends, so the dry-run exercises the
    /// pump_amm program — including the trailing buyback-fee recipient
    /// (`amm_buyback_fee_recipient`) — exactly as production would. Deltas report
    /// SOL spent (payer) and base tokens received (base ATA). `base_token_program_id`
    /// is the token's SPL program (legacy/2022), resolved by the caller.
    pub async fn simulate_amm_buy(
        &self,
        token_mint: &str,
        base_token_program_id: &str,
        sol_amount: f64,
        pool_override: Option<&str>,
        slippage_bps: Option<u64>,
    ) -> Result<SimOutcome> {
        let user = self.config.signer.pubkey();
        let (core_ixs, user_base) = self
            .build_amm_buy_ixs(
                token_mint,
                base_token_program_id,
                sol_amount,
                pool_override,
                slippage_bps,
                &user,
            )
            .await?;
        // Mirror `amm_buy`'s tx assembly (CU budget + core swap + level-0 tip) so
        // the simulated bytes match the live send.
        let mut ixs = Vec::with_capacity(core_ixs.len() + self.engine.cu_ixs_amm.len() + 1);
        ixs.extend_from_slice(&self.engine.cu_ixs_amm);
        ixs.extend(core_ixs);
        ixs.push(self.jito_tip_ix(0));
        self.simulate_ixs(ixs, &user, &[user, user_base]).await
    }

    /// Layer 1 — simulate the real PumpSwap **AMM** sell of `token_amount` raw
    /// base units. Reuses `build_amm_sell_ixs`, requires the wallet to actually
    /// hold the token (resolution bails otherwise, like the curve sell sim), and
    /// reports SOL received (payer, after the WSOL proceeds unwrap+close) and
    /// tokens sold (base account). Exercises the pump_amm buyback-fee recipient.
    pub async fn simulate_amm_sell(
        &self,
        token_mint: &str,
        token_amount: u64,
        base_token_program_id: &str,
        pool_override: Option<&str>,
        token_account_override: Option<&str>,
        slippage_bps: Option<u64>,
    ) -> Result<SimOutcome> {
        let user = self.config.signer.pubkey();
        self.simulate_amm_sell_as(
            token_mint,
            token_amount,
            base_token_program_id,
            &user,
            pool_override,
            token_account_override,
            slippage_bps,
        )
        .await
    }

    /// Like [`Self::simulate_amm_sell`], but for an arbitrary `owner` (zero SOL —
    /// `sigVerify=false`). Pass `token_account_override` for the holder's ATA.
    pub async fn simulate_amm_sell_as(
        &self,
        token_mint: &str,
        token_amount: u64,
        base_token_program_id: &str,
        owner: &Pubkey,
        pool_override: Option<&str>,
        token_account_override: Option<&str>,
        slippage_bps: Option<u64>,
    ) -> Result<SimOutcome> {
        let payer = self.config.signer.pubkey();
        let core_ixs = self
            .build_amm_sell_ixs(
                token_mint,
                token_amount,
                base_token_program_id,
                pool_override,
                token_account_override,
                slippage_bps,
                owner,
                Some(&payer),
            )
            .await?;
        let mut ixs = Vec::with_capacity(core_ixs.len() + self.engine.cu_ixs_amm.len() + 1);
        ixs.extend_from_slice(&self.engine.cu_ixs_amm);
        ixs.extend(core_ixs);
        ixs.push(self.jito_tip_ix(0));
        let user_base = self
            .resolve_user_base_account(token_mint, token_account_override)
            .await?;
        // See `simulate_curve_sell_as`: funded wallet pays the fee + ATA rent;
        // holder is the instruction user / token-account owner.
        self.simulate_ixs(ixs, &payer, &[payer, *owner, user_base])
            .await
    }

    /// Largest non-zero **trader** token account for `mint` via
    /// `getTokenLargestAccounts` (+ owner lookup). Skips bonding-curve /
    /// PumpSwap vault PDAs (often the #1 holder) — selling "as" those reverts
    /// with `Transfer: from must not carry data` because `user` is a program
    /// account. Off-path probe helper so sells can be dry-run without the
    /// configured wallet holding the mint.
    pub async fn get_token_largest_holder(
        &self,
        mint: &str,
    ) -> Result<(Pubkey /*owner*/, Pubkey /*token_account*/, u64 /*amount*/)> {
        let resp = self
            .rpc_json(
                "getTokenLargestAccounts",
                serde_json::json!([mint, { "commitment": "processed" }]),
            )
            .await?;
        let values = resp
            .get("value")
            .and_then(|v| v.as_array())
            .context("getTokenLargestAccounts: missing value")?;
        for v in values {
            let amount: u64 = v
                .get("amount")
                .and_then(|a| a.as_str())
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            if amount == 0 {
                continue;
            }
            let ata_str = v
                .get("address")
                .and_then(|a| a.as_str())
                .context("getTokenLargestAccounts: missing address")?;
            let ata = Pubkey::from_str(ata_str)?;
            let owner = self.token_account_owner(&ata).await?;
            // Vault ATAs are owned by the curve/pool PDA, which itself is owned
            // by pump / pump_amm. Real traders are system (or wallet-program)
            // accounts — never the pump programs.
            let owner_acct = match self.rpc.get_account(&owner).await {
                Ok(a) => a,
                Err(_) => continue,
            };
            if owner_acct.owner == protocol::PUMP_FUN
                || owner_acct.owner == protocol::PUMP_SWAP
                || owner_acct.executable
            {
                continue;
            }
            return Ok((owner, ata, amount));
        }
        bail!("no non-vault holder for mint {mint}");
    }

    /// First non-zero token account for `mint` owned by `owner` (probe helper).
    pub async fn get_token_account_for_owner(
        &self,
        mint: &str,
        owner: &Pubkey,
    ) -> Result<(Pubkey /*token_account*/, u64 /*amount*/)> {
        let resp = self
            .rpc_json(
                "getTokenAccountsByOwner",
                serde_json::json!([
                    owner.to_string(),
                    { "mint": mint },
                    { "encoding": "jsonParsed", "commitment": "processed" }
                ]),
            )
            .await?;
        let values = resp
            .get("value")
            .and_then(|v| v.as_array())
            .context("getTokenAccountsByOwner: missing value")?;
        for v in values {
            let amount: u64 = v
                .pointer("/account/data/parsed/info/tokenAmount/amount")
                .and_then(|a| a.as_str())
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let Some(ata_str) = v.get("pubkey").and_then(|a| a.as_str()) else {
                continue;
            };
            if amount == 0 {
                continue;
            }
            return Ok((Pubkey::from_str(ata_str)?, amount));
        }
        bail!("owner {owner} holds 0 of mint {mint}");
    }

    async fn token_account_owner(&self, ata: &Pubkey) -> Result<Pubkey> {
        let acct = self
            .rpc
            .get_account(ata)
            .await
            .with_context(|| format!("read token account {ata}"))?;
        if acct.data.len() < 64 {
            bail!("token account {ata} too short");
        }
        Ok(Pubkey::new_from_array(
            acct.data[32..64]
                .try_into()
                .map_err(|_| TradeError::Other("token account owner slice".into()))?,
        ))
    }
}
