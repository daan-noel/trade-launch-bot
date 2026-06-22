// ============================================================
// Simulation engine — run any instruction set through
// `simulateTransaction` against LIVE chain state with zero SOL at
// risk and (for the primitive) no signing key required.
//
//   simulate_ixs        — Layer 0 primitive: any instructions, any
//                         fee payer, optional tracked-account deltas.
//                         No keypair needed (sig_verify = false), so
//                         it can simulate a trade for ANY wallet.
//   simulate_curve_buy  — Layer 1: build the real curve-buy ix set
//   simulate_curve_sell   (slippage floor from live reserves) and
//                         simulate it, reporting SOL/token deltas.
//
// Reuses the SAME instruction builders the live trade path sends
// (`build_curve_buy_ixs` / `build_curve_sell_ixs`) so what's
// simulated is what production would send — not a copy that can drift.
//
// OFF THE HOT PATH: each call is one or two RPC round-trips. Never
// invoke inline before a real send (see CLAUDE.md latency budgets).
// ============================================================

use super::PumpFunTrader;
use anyhow::{Context, Result};
use base64::{engine::general_purpose, Engine as _};
use solana_account_decoder::{UiAccountData, UiAccountEncoding};
use solana_client::rpc_config::{RpcSimulateTransactionAccountsConfig, RpcSimulateTransactionConfig};
use solana_sdk::{
    commitment_config::CommitmentConfig, instruction::Instruction, message::Message,
    pubkey::Pubkey, signature::Signer, transaction::Transaction,
};
use std::str::FromStr;

/// Pre/post balance snapshot for one account tracked through a simulation.
#[derive(Debug, Clone)]
pub struct AccountDelta {
    pub pubkey: Pubkey,
    pub lamports_before: u64,
    pub lamports_after: u64,
    /// SPL token amount (raw base units) before/after — `Some` only when the
    /// account is (or becomes) an SPL / Token-2022 token account; `None` for a
    /// plain SOL account such as the fee payer.
    pub token_before: Option<u64>,
    pub token_after: Option<u64>,
}

impl AccountDelta {
    /// Signed lamports change (after − before). Negative = SOL spent. Note the
    /// network base fee is NOT reflected by `simulateTransaction`, so a buy's
    /// payer delta is the on-chain spend (buy lamports + Jito tip), not the full
    /// wallet debit.
    pub fn lamports_delta(&self) -> i64 {
        self.lamports_after as i64 - self.lamports_before as i64
    }

    /// Signed token change (after − before) when this is a token account. An
    /// account created during the sim (no `before`) counts its full `after` as a
    /// gain.
    pub fn token_delta(&self) -> Option<i64> {
        match (self.token_before, self.token_after) {
            (Some(b), Some(a)) => Some(a as i64 - b as i64),
            (None, Some(a)) => Some(a as i64),
            _ => None,
        }
    }
}

/// Outcome of a `simulateTransaction` run against live chain state.
#[derive(Debug, Clone)]
pub struct SimOutcome {
    /// `true` when the simulated tx would NOT revert.
    pub success: bool,
    /// The revert message (debug-formatted `TransactionError`) when it would.
    pub err: Option<String>,
    /// The program's custom (Anchor) error code on a reverted sim — e.g. curve
    /// `TooLittleSolReceived` = 6003, AMM `ExceededSlippage` = 6004 — so a caller
    /// can tell a slippage revert (retryable) from a structural one. `None` when
    /// the failure wasn't an `InstructionError::Custom`.
    pub custom_error: Option<u32>,
    pub units_consumed: Option<u64>,
    pub logs: Vec<String>,
    /// Pre/post balances for the accounts the caller asked to track.
    pub accounts: Vec<AccountDelta>,
}

impl SimOutcome {
    /// The tracked delta for `pubkey`, if it was tracked.
    pub fn delta_for(&self, pubkey: &Pubkey) -> Option<&AccountDelta> {
        self.accounts.iter().find(|d| &d.pubkey == pubkey)
    }
}

impl PumpFunTrader {
    /// Layer 0 — the simulation primitive. Run `instructions` through
    /// `simulateTransaction` against current chain state and report the outcome
    /// plus pre/post balances for `track_accounts`.
    ///
    /// NO signing key is required: the tx is built UNSIGNED with `payer` as fee
    /// payer, `sig_verify = false`, and `replace_recent_blockhash = true`, so this
    /// can simulate a transaction for ANY wallet — not only the configured one.
    /// (The domain helpers below still build wallet-specific trade ixs, so those
    /// simulate the configured wallet; the generality lives here.)
    ///
    /// OFF THE HOT PATH — one batched pre-state RPC plus the simulate RPC. Never
    /// call inline before a real send.
    pub async fn simulate_ixs(
        &self,
        instructions: Vec<Instruction>,
        payer: &Pubkey,
        track_accounts: &[Pubkey],
    ) -> Result<SimOutcome> {
        // Pre-state for the tracked accounts (one batched RPC). A missing account
        // reads as zero lamports / not-yet-a-token-account.
        let pre = if track_accounts.is_empty() {
            Vec::new()
        } else {
            self.rpc
                .get_multiple_accounts(track_accounts)
                .await
                .context("simulate: fetch pre-state")?
        };

        let tx = Transaction::new_unsigned(Message::new(&instructions, Some(payer)));

        let accounts_cfg =
            (!track_accounts.is_empty()).then(|| RpcSimulateTransactionAccountsConfig {
                encoding: Some(UiAccountEncoding::Base64),
                addresses: track_accounts.iter().map(|p| p.to_string()).collect(),
            });
        let cfg = RpcSimulateTransactionConfig {
            sig_verify: false,
            replace_recent_blockhash: true,
            commitment: Some(CommitmentConfig::processed()),
            accounts: accounts_cfg,
            ..Default::default()
        };
        let resp = self
            .rpc
            .simulate_transaction_with_config(&tx, cfg)
            .await
            .context("simulateTransaction RPC")?;
        let v = resp.value;

        let post = v.accounts.unwrap_or_default();
        let mut accounts = Vec::with_capacity(track_accounts.len());
        for (i, pk) in track_accounts.iter().enumerate() {
            let (lamports_before, token_before) = match pre.get(i).and_then(|o| o.as_ref()) {
                Some(acct) => (acct.lamports, decode_token_amount(&acct.owner, &acct.data)),
                None => (0, None),
            };
            let (lamports_after, token_after) = match post.get(i).and_then(|o| o.as_ref()) {
                Some(ui) => {
                    let token = Pubkey::from_str(&ui.owner)
                        .ok()
                        .zip(decode_ui_data(&ui.data))
                        .and_then(|(owner, data)| decode_token_amount(&owner, &data));
                    (ui.lamports, token)
                }
                None => (0, None),
            };
            accounts.push(AccountDelta {
                pubkey: *pk,
                lamports_before,
                lamports_after,
                token_before,
                token_after,
            });
        }

        Ok(SimOutcome {
            success: v.err.is_none(),
            custom_error: v.err.as_ref().and_then(super::tx::custom_error_code),
            err: v.err.map(|e| format!("{e:?}")),
            units_consumed: v.units_consumed,
            logs: v.logs.unwrap_or_default(),
            accounts,
        })
    }

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
        let buy_lamports = super::buy_lamports_checked(sol_amount)?;

        if !self.token_pdas.contains_key(token_mint) {
            self.ensure_token_pdas(token_mint).await?;
        }
        let mint = Pubkey::from_str(token_mint)?;
        let pdas = self
            .token_pdas
            .get(token_mint)
            .map(|r| *r)
            .context("PDAs not cached")?;

        let wallet = self.config.keypair.pubkey();
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
        let min_tokens_out =
            super::buy::compute_curve_buy_min_out(buy_lamports, slippage_bps, reserves);

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
        if !self.token_pdas.contains_key(token_mint) {
            self.ensure_token_pdas(token_mint).await?;
        }
        if self.resolve_cached_token_account(token_mint).await?.is_none() {
            anyhow::bail!("No token account cached/found for mint {token_mint}");
        }
        let mint = Pubkey::from_str(token_mint)?;
        let pdas = self
            .token_pdas
            .get(token_mint)
            .map(|r| *r)
            .context("PDAs not cached")?;
        let user_token_account = self
            .user_token_accounts
            .get(token_mint)
            .map(|r| *r)
            .context("token account not cached")?;

        let reserves = match slippage_bps {
            Some(_) => self.curve_reserves(token_mint, &pdas.bonding_curve).await.ok(),
            None => None,
        };
        let min_sol_output =
            super::sell::compute_curve_sell_min_out(token_amount, slippage_bps, reserves);

        let ixs = self.build_curve_sell_ixs(
            &mint,
            &pdas,
            &user_token_account,
            token_amount,
            min_sol_output,
            is_cashback,
            0,
        )?;
        let payer = self.config.keypair.pubkey();
        self.simulate_ixs(ixs, &payer, &[payer, user_token_account])
            .await
    }
}

/// Decode a `UiAccountData` payload into raw bytes. Handles the base64 encoding
/// the engine requests (and legacy base58 for completeness); `None` for the JSON
/// encoding, which the engine never asks for.
fn decode_ui_data(data: &UiAccountData) -> Option<Vec<u8>> {
    match data {
        UiAccountData::Binary(s, UiAccountEncoding::Base64) => {
            general_purpose::STANDARD.decode(s).ok()
        }
        UiAccountData::LegacyBinary(s) => bs58::decode(s).into_vec().ok(),
        _ => None,
    }
}

/// Read the raw SPL token amount from an account's `(owner, data)` when it's an
/// SPL Token / Token-2022 account — the amount is a little-endian `u64` at offset
/// 64 in both layouts (Token-2022 appends extensions after the 165-byte base).
/// `None` for any non-token account.
fn decode_token_amount(owner: &Pubkey, data: &[u8]) -> Option<u64> {
    let is_token = *owner == spl_token::id() || *owner == spl_token_2022::id();
    if !is_token || data.len() < 72 {
        return None;
    }
    Some(u64::from_le_bytes(data[64..72].try_into().ok()?))
}
