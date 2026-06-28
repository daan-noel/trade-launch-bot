//! `raw-json` feature: synthesise the Helius-shaped raw-tx blob from a protobuf
//! `SubscribeUpdateTransaction` and emit it as [`IngestEvent::RawTx`].
//!
//! Runs entirely off the hot path (the decode task emits it alongside normal
//! events, after decode). The host persists it to `raw_transactions.raw_data`.

use chrono::{DateTime, Utc};
use serde_json::{json, Value};

use crate::event::{IngestEvent, RawTx};
use crate::proto::geyser::SubscribeUpdateTransaction;
use crate::proto::solana::storage::confirmed_block as scb;

/// Synthesise the Helius-shaped JSON blob and wrap it as [`IngestEvent::RawTx`].
/// Returns `None` if the update is missing required fields.
pub fn build_raw_tx_event(
    update: &SubscribeUpdateTransaction,
    received_at: DateTime<Utc>,
) -> Option<IngestEvent> {
    let blob = build_raw_blob(update)?;
    let info = update.transaction.as_ref()?;
    let signature = bs58::encode(&info.signature).into_string();
    Some(IngestEvent::RawTx(RawTx {
        signature,
        slot: update.slot,
        raw_data: blob,
        received_at,
        block_time: received_at, // best approximation; gRPC stream doesn't carry block_time
    }))
}

/// Synthesise the Helius-shaped `params.result` blob from the protobuf.
/// Byte-consistent with the live feed — used by backfill so historical and live
/// blobs share one shape.
pub fn build_raw_blob(update: &SubscribeUpdateTransaction) -> Option<Value> {
    let info = update.transaction.as_ref()?;
    let tx = info.transaction.as_ref()?;
    let message = tx.message.as_ref()?;
    let meta = info.meta.as_ref()?;

    let signature = bs58::encode(&info.signature).into_string();

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

fn account_indexes(accounts: &[u8]) -> Vec<Value> {
    accounts.iter().map(|&b| json!(b as u64)).collect()
}

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
