//! JSON extraction and amount-computation helpers shared across the decoder.
//!
//! These operate directly on the raw Helius `transaction`/`meta` JSON and have
//! no knowledge of pump.fun semantics beyond locating instructions and balances.

use serde_json::Value;

use crate::config::constants::{CREATE_INSTRUCTION_DISCRIMINATOR, CREATE_V2_INSTRUCTION_DISCRIMINATOR};

use super::instructions::resolve_instruction_program_id;

// ---------------------------------------------------------------------------
// JSON extraction helpers
// ---------------------------------------------------------------------------

pub(super) fn extract_logs<'a>(meta: &'a Value) -> Vec<&'a str> {
    meta["logMessages"]
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default()
}

/// With `jsonParsed` encoding, accountKeys is:
///   [{pubkey: "...", signer: bool, writable: bool}, ...]
/// Fall back to plain string array for other encodings.
pub(super) fn extract_account_keys(message: &Value) -> Vec<String> {
    message["accountKeys"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    v["pubkey"]
                        .as_str()
                        .or_else(|| v.as_str())
                        .map(|s| s.to_string())
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Find ALL instructions that belong to `program_id`, searching both the
/// top-level `message.instructions` list and every entry in
/// `meta.innerInstructions[*].instructions`.

/// This is needed because trading bots (Terminal, Axiom, …) wrap pump.fun
/// calls as CPI — the pump.fun instruction appears only as an inner call.
pub(super) fn find_pump_ixs_anywhere<'a>(
    message: &'a Value,
    meta: &'a Value,
    account_keys: &[String],
    program_id: &str,
) -> Vec<&'a Value> {
    let mut result = Vec::new();

    if let Some(outer) = message["instructions"].as_array() {
        for ix in outer {
            if resolve_instruction_program_id(ix, account_keys) == program_id {
                result.push(ix);
            }
        }
    }

    if let Some(groups) = meta["innerInstructions"].as_array() {
        for group in groups {
            if let Some(inner) = group["instructions"].as_array() {
                for ix in inner {
                    if resolve_instruction_program_id(ix, account_keys) == program_id {
                        result.push(ix);
                    }
                }
            }
        }
    }

    result
}

/// Return true if the instruction's data starts with a known pump.fun Create
/// discriminator (v1 or v2).
pub(super) fn is_pump_create_ix(ix: &Value) -> bool {
    let data_b58 = match ix["data"].as_str() {
        Some(s) => s,
        None => return false,
    };
    let data = match bs58::decode(data_b58).into_vec() {
        Ok(d) => d,
        Err(_) => return false,
    };
    data.len() >= 8
        && (data[..8] == CREATE_INSTRUCTION_DISCRIMINATOR
            || data[..8] == CREATE_V2_INSTRUCTION_DISCRIMINATOR)
}

/// Resolve account pubkeys from a pump instruction.

/// Helius with `jsonParsed` encoding may return accounts as either:
///   - Resolved pubkey strings: `["pk1", "pk2", ...]`
///   - Integer indices into `message.accountKeys`: `[0, 1, 2, ...]`
/// Both are handled; unresolvable entries are silently skipped.
pub(super) fn resolve_pump_accounts(ix: &Value, account_keys: &[String]) -> Vec<String> {
    ix["accounts"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    v.as_str().map(|s| s.to_string()).or_else(|| {
                        v.as_u64()
                            .and_then(|i| account_keys.get(i as usize))
                            .cloned()
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn extract_balances(balances: &Value) -> Vec<u64> {
    balances
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_u64()).collect())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Amount computation
// ---------------------------------------------------------------------------

/// Compute the absolute SOL change (in SOL, not lamports) for `wallet`
/// using the flat pre/post balance arrays (indexed by accountKeys order).
pub(super) fn compute_sol_change(
    wallet: &str,
    account_keys: &[String],
    pre: &[u64],
    post: &[u64],
) -> f64 {
    account_keys
        .iter()
        .position(|k| k == wallet)
        .map(|idx| {
            let pre_bal = pre.get(idx).copied().unwrap_or(0);
            let post_bal = post.get(idx).copied().unwrap_or(0);
            (pre_bal as f64 - post_bal as f64).abs() / 1_000_000_000.0
        })
        .unwrap_or(0.0)
}

/// Compute the absolute token amount change for `user_ata` (the trader's
/// Associated Token Account) using pre/post token balance entries.
/// Token balance entries carry an `accountIndex` that maps into `account_keys`.
///
/// Returns RAW base units (no decimal scaling) to stay consistent with the
/// authoritative log-event path (`decode_trade_events_from_logs`), which stores
/// `token_amount` as raw units. Reading `uiTokenAmount.uiAmount` here instead
/// would yield a decimal-adjusted amount (raw / 10^decimals), making this
/// fallback path's `price_per_token` inflated by 10^decimals and poisoning ATH.
pub(super) fn compute_token_change(
    user_ata: &str,
    mint: &str,
    account_keys: &[String],
    meta: &Value,
) -> f64 {
    let ata_idx = match account_keys.iter().position(|k| k == user_ata) {
        Some(i) => i as u64,
        None => return 0.0,
    };

    let find_amount = |balances: &Value| -> f64 {
        balances
            .as_array()
            .and_then(|arr| {
                arr.iter().find(|entry| {
                    entry["accountIndex"].as_u64() == Some(ata_idx)
                        && entry["mint"].as_str() == Some(mint)
                })
            })
            // `amount` is the raw base-unit integer as a string; parse it so the
            // units match the log-event path. (`uiAmount` is decimal-adjusted.)
            .and_then(|entry| entry["uiTokenAmount"]["amount"].as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0)
    };

    let pre = find_amount(&meta["preTokenBalances"]);
    let post = find_amount(&meta["postTokenBalances"]);
    (post - pre).abs()
}
