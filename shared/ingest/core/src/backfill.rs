//! `rpc-backfill` feature: convert an RPC `getTransaction(encoding="base64")`
//! result into a [`SubscribeUpdateTransaction`] for [`crate::decode::Decoder`].
//!
//! See [`rpc_to_protobuf`] for the full design rationale.
//!
//! *Not* part of the live ingest path — only used by host backfill routines
//! (e.g. token_sync AMM historical loop). The decoder cannot tell the source apart.

use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::Value;
use solana_sdk::{message::VersionedMessage, transaction::VersionedTransaction};

use crate::proto::geyser::{SubscribeUpdateTransaction, SubscribeUpdateTransactionInfo};
use crate::proto::solana::storage::confirmed_block as scb;

mod pager;
pub use pager::{
    get_signatures_for_address, get_transactions_batch, get_transactions_for_address_page,
    wrap_transaction_result, SignatureInfo,
};

/// Convert one RPC transaction result (`encoding="base64"`) into a
/// [`SubscribeUpdateTransaction`] for [`crate::decode::Decoder::decode_protobuf`].
///
/// The expected shape is `{ signature, slot, blockTime, transaction: {
/// transaction: ["<b64>", "base64"], meta } }`. Returns `None` if any required
/// field is absent or the base64/bincode decode fails.
pub fn rpc_to_protobuf(result: &Value) -> Option<SubscribeUpdateTransaction> {
    let slot = result.get("slot")?.as_u64()?;
    let inner = result.get("transaction")?;

    let tx_b64 = inner.get("transaction")?.get(0)?.as_str()?;
    let tx_bytes = STANDARD.decode(tx_b64).ok()?;
    let vtx: VersionedTransaction = bincode::deserialize(&tx_bytes).ok()?;

    let signature = result
        .get("signature")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .and_then(|s| bs58::decode(s).into_vec().ok())
        .or_else(|| vtx.signatures.first().map(|s| s.as_ref().to_vec()))
        .unwrap_or_default();

    let (account_keys, instructions, versioned) = match &vtx.message {
        VersionedMessage::Legacy(m) => (&m.account_keys, &m.instructions, false),
        VersionedMessage::V0(m) => (&m.account_keys, &m.instructions, true),
    };
    let message = scb::Message {
        account_keys: account_keys.iter().map(|k| k.to_bytes().to_vec()).collect(),
        instructions: instructions
            .iter()
            .map(|ix| scb::CompiledInstruction {
                program_id_index: ix.program_id_index as u32,
                accounts: ix.accounts.clone(),
                data: ix.data.clone(),
            })
            .collect(),
        versioned,
        ..Default::default()
    };

    let meta = meta_from_json(inner.get("meta")?);

    Some(SubscribeUpdateTransaction {
        transaction: Some(SubscribeUpdateTransactionInfo {
            signature,
            transaction: Some(scb::Transaction {
                signatures: vec![],
                message: Some(message),
            }),
            meta: Some(meta),
            ..Default::default()
        }),
        slot,
    })
}

fn meta_from_json(meta: &Value) -> scb::TransactionStatusMeta {
    let loaded = meta.get("loadedAddresses");
    scb::TransactionStatusMeta {
        log_messages: str_vec(meta.get("logMessages")),
        pre_balances: u64_vec(meta.get("preBalances")),
        post_balances: u64_vec(meta.get("postBalances")),
        inner_instructions: inner_from_json(meta.get("innerInstructions")),
        pre_token_balances: token_balances_from_json(meta.get("preTokenBalances")),
        post_token_balances: token_balances_from_json(meta.get("postTokenBalances")),
        loaded_writable_addresses: loaded
            .and_then(|l| l.get("writable"))
            .map(key_vec)
            .unwrap_or_default(),
        loaded_readonly_addresses: loaded
            .and_then(|l| l.get("readonly"))
            .map(key_vec)
            .unwrap_or_default(),
        ..Default::default()
    }
}

fn str_vec(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

fn u64_vec(v: Option<&Value>) -> Vec<u64> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_u64).collect())
        .unwrap_or_default()
}

fn key_vec(v: &Value) -> Vec<Vec<u8>> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .filter_map(|s| bs58::decode(s).into_vec().ok())
                .collect()
        })
        .unwrap_or_default()
}

fn inner_from_json(v: Option<&Value>) -> Vec<scb::InnerInstructions> {
    v.and_then(Value::as_array)
        .map(|groups| {
            groups
                .iter()
                .filter_map(|g| {
                    Some(scb::InnerInstructions {
                        index: g.get("index")?.as_u64()? as u32,
                        instructions: g
                            .get("instructions")?
                            .as_array()?
                            .iter()
                            .filter_map(inner_ix_from_json)
                            .collect(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn inner_ix_from_json(ix: &Value) -> Option<scb::InnerInstruction> {
    Some(scb::InnerInstruction {
        program_id_index: ix.get("programIdIndex")?.as_u64()? as u32,
        accounts: ix
            .get("accounts")?
            .as_array()?
            .iter()
            .filter_map(|n| n.as_u64().map(|n| n as u8))
            .collect(),
        data: bs58::decode(ix.get("data")?.as_str()?).into_vec().ok()?,
        stack_height: ix.get("stackHeight").and_then(Value::as_u64).map(|s| s as u32),
    })
}

fn token_balances_from_json(v: Option<&Value>) -> Vec<scb::TokenBalance> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|tb| {
                    Some(scb::TokenBalance {
                        account_index: tb.get("accountIndex")?.as_u64()? as u32,
                        mint: tb.get("mint")?.as_str()?.to_string(),
                        ui_token_amount: tb
                            .get("uiTokenAmount")
                            .and_then(|u| u.get("amount"))
                            .and_then(Value::as_str)
                            .map(|amount| scb::UiTokenAmount {
                                amount: amount.to_string(),
                                ..Default::default()
                            }),
                        ..Default::default()
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}
