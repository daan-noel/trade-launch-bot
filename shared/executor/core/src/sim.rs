// ============================================================
// Simulation primitive — run any instruction set through
// `simulateTransaction` against LIVE chain state with zero SOL at
// risk and no signing key required.
//
//   simulate_ixs — Layer 0 primitive: any instructions, any fee payer,
//                  optional tracked-account deltas. No keypair needed
//                  (sig_verify = false), so it can simulate a trade for
//                  ANY wallet.
//
// The venue-specific Layer-1 helpers (simulate_curve_buy / _sell,
// simulate_amm_buy / _sell) live in the venue crate and call this
// primitive via the engine, so they simulate exactly the ix set the
// venue's live trade path would send.
//
// OFF THE HOT PATH: each call is one or two RPC round-trips. Never
// invoke inline before a real send.
// ============================================================

use crate::engine::Engine;
use crate::error::{Context, Result};
use crate::send::custom_error_code;
use base64::{engine::general_purpose, Engine as _};
use solana_account_decoder::{UiAccountData, UiAccountEncoding};
use solana_client::rpc_config::{RpcSimulateTransactionAccountsConfig, RpcSimulateTransactionConfig};
use solana_sdk::{
    commitment_config::CommitmentConfig, instruction::Instruction, message::Message,
    pubkey::Pubkey, transaction::Transaction,
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

impl Engine {
    /// Layer 0 — the simulation primitive. Run `instructions` through
    /// `simulateTransaction` against current chain state and report the outcome
    /// plus pre/post balances for `track_accounts`.
    ///
    /// NO signing key is required: the tx is built UNSIGNED with `payer` as fee
    /// payer, `sig_verify = false`, and `replace_recent_blockhash = true`, so this
    /// can simulate a transaction for ANY wallet — not only the configured one.
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
            custom_error: v.err.as_ref().and_then(custom_error_code),
            err: v.err.map(|e| format!("{e:?}")),
            units_consumed: v.units_consumed,
            logs: v.logs.unwrap_or_default(),
            accounts,
        })
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
