//! Adapter: Yellowstone `SubscribeUpdateTransaction` (protobuf) -> the Helius
//! `params.result` JSON shape consumed by the cloned decoder's `decode_result`.
//!
//! This lets the decoder be reused essentially unchanged: it reads `logMessages`,
//! base58 instruction `data`, string `accountKeys`, and index-based
//! `accounts`/`programIdIndex` — all synthesized here from the protobuf. The same
//! `Value` is also persisted verbatim as the gRPC raw-tx blob.

use serde_json::{json, Value};

use crate::config::constants::PUMP_SWAP_PROGRAM_ID;

use super::proto::geyser::SubscribeUpdateTransaction;
use super::proto::solana::storage::confirmed_block as scb;

/// Convert a transaction update into the decoder's expected `result` object.
///
/// Returns `None` if the update lacks the transaction / message / meta we need
/// (e.g. a non-transaction update, or a malformed frame), or if a cheap log-based
/// pre-filter shows the decoder would ignore it anyway — so the heap-heavy `Value`
/// is never built for the many txs the subscription delivers that touch neither
/// the pump.fun curve program nor a PumpSwap (AMM) pool. `pump_program_id` is the
/// same curve program id the decoder gates on, so this never drops a decodable tx.
pub fn update_tx_to_value(
    update: &SubscribeUpdateTransaction,
    pump_program_id: &str,
) -> Option<Value> {
    let meta = update.transaction.as_ref()?.meta.as_ref()?;

    // Pre-filter: the decoder keeps only bonding-curve txs (curve program in the
    // logs) and PumpSwap AMM swaps (AMM program in the logs); everything else is
    // dropped one call later. Mirror that gate here on the raw `log_messages` and
    // skip the whole `Value` build for the rest — the biggest per-event win.
    if !meta
        .log_messages
        .iter()
        .any(|l| l.contains(pump_program_id) || l.contains(PUMP_SWAP_PROGRAM_ID))
    {
        return None;
    }

    build_raw_blob(update)
}

/// Synthesise the Helius-shaped `params.result` blob from the protobuf, **without**
/// the pre-filter — the persisted gRPC raw-tx blob (`raw_transactions.raw_data`).
///
/// Tier B runs this off the ingest hot path, inside the DbWriter flush, only for
/// txs we actually persist: the heap-heavy `Value` synthesis (base58 of the
/// signature + every account key + every instruction `data`, plus the `json!`
/// tree) no longer blocks the serial ingest task. The bytes are identical to the
/// old inline build, so historical blobs and replay stay byte-for-byte consistent.
/// `update_tx_to_value` = this, gated by the cheap log pre-filter (used by the
/// cold replay path, which needs a decodable `Value`).
pub fn build_raw_blob(update: &SubscribeUpdateTransaction) -> Option<Value> {
    let info = update.transaction.as_ref()?;
    let tx = info.transaction.as_ref()?;
    let message = tx.message.as_ref()?;
    let meta = info.meta.as_ref()?;

    let signature = bs58::encode(&info.signature).into_string();

    // accountKeys = static keys ++ loaded writable ++ loaded readonly, in that
    // exact order. Instruction `accounts`/`programIdIndex` index into this
    // combined list for v0 (versioned) transactions; a wrong order silently
    // misattributes wallets/mints. (Adapter design note 1.)
    let account_keys: Vec<Value> = message
        .account_keys
        .iter()
        .chain(meta.loaded_writable_addresses.iter())
        .chain(meta.loaded_readonly_addresses.iter())
        .map(|k| Value::String(bs58::encode(k).into_string()))
        .collect();

    let instructions: Vec<Value> = message.instructions.iter().map(compiled_ix).collect();

    let inner_instructions: Vec<Value> = meta
        .inner_instructions
        .iter()
        .map(|g| {
            json!({
                "index": g.index,
                "instructions": g.instructions.iter().map(inner_ix).collect::<Vec<_>>(),
            })
        })
        .collect();

    Some(json!({
        "signature": signature,
        "slot": update.slot,
        // Not present on tx updates; decoder falls back to now(). received_at is
        // the real ingest clock and is set downstream.
        "blockTime": Value::Null,
        "transaction": {
            "meta": {
                "logMessages": meta.log_messages,
                "innerInstructions": inner_instructions,
                "preBalances": meta.pre_balances,
                "postBalances": meta.post_balances,
                "preTokenBalances": token_balances(&meta.pre_token_balances),
                "postTokenBalances": token_balances(&meta.post_token_balances),
            },
            "transaction": {
                "message": {
                    "accountKeys": account_keys,
                    "instructions": instructions,
                }
            }
        }
    }))
}

fn compiled_ix(ix: &scb::CompiledInstruction) -> Value {
    json!({
        "programIdIndex": ix.program_id_index,
        "accounts": account_indexes(&ix.accounts),
        "data": bs58::encode(&ix.data).into_string(),
    })
}

fn inner_ix(ix: &scb::InnerInstruction) -> Value {
    json!({
        "programIdIndex": ix.program_id_index,
        "accounts": account_indexes(&ix.accounts),
        "data": bs58::encode(&ix.data).into_string(),
    })
}

/// Account-index bytes -> JSON number array (the decoder reads these as `u64`).
fn account_indexes(accounts: &[u8]) -> Vec<Value> {
    accounts.iter().map(|&b| json!(b as u64)).collect()
}

/// Map protobuf token balances to the subset the decoder reads: `accountIndex`,
/// `mint`, and the raw base-unit `uiTokenAmount.amount` (kept as a string so the
/// decoder's `parse::<f64>()` yields raw units, not a decimal-scaled amount).
fn token_balances(balances: &[scb::TokenBalance]) -> Vec<Value> {
    balances
        .iter()
        .map(|tb| {
            let amount = tb
                .ui_token_amount
                .as_ref()
                .map(|u| u.amount.clone())
                .unwrap_or_default();
            json!({
                "accountIndex": tb.account_index,
                "mint": tb.mint,
                "uiTokenAmount": { "amount": amount },
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::proto::geyser::{SubscribeUpdateTransaction, SubscribeUpdateTransactionInfo};
    use super::super::proto::solana::storage::confirmed_block as scb;
    use super::update_tx_to_value;

    fn pk(b: u8) -> Vec<u8> {
        vec![b; 32]
    }

    /// A pump.fun curve program id used to satisfy the log pre-filter in tests.
    const TEST_PUMP_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";

    #[test]
    fn account_keys_concat_static_then_loaded_writable_then_readonly() {
        let message = scb::Message {
            account_keys: vec![pk(1), pk(2)], // static, global indices 0,1
            instructions: vec![scb::CompiledInstruction {
                program_id_index: 0,
                accounts: vec![2, 3], // -> loaded writable (2), loaded readonly (3)
                data: vec![1, 2, 3],
            }],
            versioned: true,
            ..Default::default()
        };
        let meta = scb::TransactionStatusMeta {
            loaded_writable_addresses: vec![pk(3)], // global index 2
            loaded_readonly_addresses: vec![pk(4)], // global index 3
            log_messages: vec![
                "Program log: hi".to_string(),
                format!("Program {TEST_PUMP_PROGRAM_ID} invoke [1]"),
            ],
            pre_token_balances: vec![scb::TokenBalance {
                account_index: 1,
                mint: "MINT".to_string(),
                ui_token_amount: Some(scb::UiTokenAmount {
                    amount: "12345".to_string(),
                    ..Default::default()
                }),
                ..Default::default()
            }],
            ..Default::default()
        };
        let update = SubscribeUpdateTransaction {
            transaction: Some(SubscribeUpdateTransactionInfo {
                signature: vec![9u8; 64],
                transaction: Some(scb::Transaction {
                    signatures: vec![],
                    message: Some(message),
                }),
                meta: Some(meta),
                ..Default::default()
            }),
            slot: 42,
        };

        let v = update_tx_to_value(&update, TEST_PUMP_PROGRAM_ID)
            .expect("adapter should produce a value");

        // accountKeys order: static (0,1) -> loaded writable (2) -> loaded readonly (3)
        let keys = v["transaction"]["transaction"]["message"]["accountKeys"]
            .as_array()
            .unwrap();
        assert_eq!(keys.len(), 4);
        assert_eq!(keys[0], json!(bs58::encode(pk(1)).into_string()));
        assert_eq!(keys[1], json!(bs58::encode(pk(2)).into_string()));
        assert_eq!(keys[2], json!(bs58::encode(pk(3)).into_string()));
        assert_eq!(keys[3], json!(bs58::encode(pk(4)).into_string()));

        assert_eq!(v["slot"], json!(42));
        assert_eq!(
            v["signature"],
            json!(bs58::encode(vec![9u8; 64]).into_string())
        );

        // instruction: numeric account indices preserved; data round-trips base58
        let ix = &v["transaction"]["transaction"]["message"]["instructions"][0];
        assert_eq!(ix["programIdIndex"], json!(0));
        assert_eq!(ix["accounts"], json!([2, 3]));
        assert_eq!(
            bs58::decode(ix["data"].as_str().unwrap()).into_vec().unwrap(),
            vec![1u8, 2, 3]
        );

        // token balance amount preserved as a string; logs passed through
        let tb = &v["transaction"]["meta"]["preTokenBalances"][0];
        assert_eq!(tb["accountIndex"], json!(1));
        assert_eq!(tb["mint"], json!("MINT"));
        assert_eq!(tb["uiTokenAmount"]["amount"], json!("12345"));
        assert_eq!(
            v["transaction"]["meta"]["logMessages"][0],
            json!("Program log: hi")
        );
    }
}
