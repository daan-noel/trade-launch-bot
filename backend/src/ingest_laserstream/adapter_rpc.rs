//! Adapter: RPC `getTransaction(encoding="base64")` result -> the Yellowstone
//! protobuf structs (`scb::*`) that [`super::decoder::HeliusDecoder::decode_protobuf`]
//! consumes. The near-inverse of [`super::adapter::build_raw_blob`]
//! (protobuf -> Value).
//!
//! ## Why base64, not jsonParsed
//!
//! token_sync historically fed the decoder a Helius `jsonParsed` `Value`
//! ([`super::decoder`]'s `json` subtree). Moving token_sync onto the protobuf
//! decoder retires that whole subtree — but only if the RPC bytes can be lowered
//! to `scb::*` *losslessly*. `jsonParsed` can't: it pre-parses Compute-Budget /
//! System / Token instructions into `{parsed, program, programId}` form, dropping
//! the raw `data` bytes that `build_labels_pb` + `extract_compute_budget` read on
//! every instruction. `encoding: "base64"` instead returns the Solana-native
//! `VersionedTransaction` (bincode), whose instructions are uniformly
//! `{program_id_index, accounts, data}` — the exact shape `scb::CompiledInstruction`
//! wants, identical to the gRPC hot path.
//!
//! ## Where the two transports meet
//!
//! - **`transaction`** half: base64 -> `bincode::deserialize::<VersionedTransaction>`
//!   -> `scb::Message` (static account keys + compiled instructions).
//! - **`meta`** half: the RPC `meta` is JSON on *every* encoding, so it is mapped
//!   field-by-field into `scb::TransactionStatusMeta` (the same fields
//!   `build_raw_blob` emits in the other direction).
//!
//! Both halves land in one `SubscribeUpdateTransaction`, structurally identical
//! to a live LaserStream frame, so `decode_protobuf` cannot tell the source apart.
//! `decode_protobuf` rebuilds the combined account-key list as
//! `message.account_keys ++ meta.loaded_writable_addresses ++ meta.loaded_readonly_addresses`,
//! so the static keys go in `Message` and the LUT-loaded keys go in `meta` — never
//! both, never reordered (a wrong order misattributes wallets/mints).

use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::Value;
use solana_sdk::{message::VersionedMessage, transaction::VersionedTransaction};

use super::proto::geyser::{SubscribeUpdateTransaction, SubscribeUpdateTransactionInfo};
use super::proto::solana::storage::confirmed_block as scb;

/// Convert one RPC transaction result (wrapped by
/// [`crate::services::helius_rpc::wrap_transaction_result`], `encoding="base64"`)
/// into a [`SubscribeUpdateTransaction`] for `decode_protobuf`.
///
/// The wrapped shape is `{ signature, slot, blockTime, transaction: { transaction,
/// meta } }`, where `transaction.transaction` is the base64 tuple `["<b64>",
/// "base64"]` and `transaction.meta` is the JSON meta. Returns `None` if any
/// required field is absent or the base64/bincode decode fails — the caller treats
/// that exactly like `decode_protobuf` returning `Ignored`.
pub fn rpc_to_protobuf(result: &Value) -> Option<SubscribeUpdateTransaction> {
    let slot = result.get("slot")?.as_u64()?;
    let inner = result.get("transaction")?;

    // ── base64 -> bincode -> Solana message (the "transaction" half) ───────────
    // `encoding: "base64"` returns `transaction` as `["<base64>", "base64"]`.
    let tx_b64 = inner.get("transaction")?.get(0)?.as_str()?;
    let tx_bytes = STANDARD.decode(tx_b64).ok()?;
    let vtx: VersionedTransaction = bincode::deserialize(&tx_bytes).ok()?;

    // Prefer the caller-supplied base58 signature (authoritative); fall back to
    // the first signature in the decoded tx. The base64 fetch paths (gTFA) have no
    // signature string in the result — they pass `""` and rely on this fallback,
    // so an empty/blank string must NOT be treated as a (zero-byte) signature.
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

    // ── JSON meta -> scb::TransactionStatusMeta (the "meta" half) ──────────────
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

/// Map the RPC JSON `meta` to the subset of `scb::TransactionStatusMeta` the
/// decoder reads. Missing fields default to empty — `decode_protobuf` already
/// tolerates a tx with no inner instructions / balances / token balances.
fn meta_from_json(meta: &Value) -> scb::TransactionStatusMeta {
    let loaded = meta.get("loadedAddresses");
    scb::TransactionStatusMeta {
        log_messages: str_vec(meta.get("logMessages")),
        pre_balances: u64_vec(meta.get("preBalances")),
        post_balances: u64_vec(meta.get("postBalances")),
        inner_instructions: inner_from_json(meta.get("innerInstructions")),
        pre_token_balances: token_balances_from_json(meta.get("preTokenBalances")),
        post_token_balances: token_balances_from_json(meta.get("postTokenBalances")),
        loaded_writable_addresses: loaded.and_then(|l| l.get("writable")).map(key_vec).unwrap_or_default(),
        loaded_readonly_addresses: loaded.and_then(|l| l.get("readonly")).map(key_vec).unwrap_or_default(),
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

/// base58 pubkey strings (the `loadedAddresses` form) -> raw 32-byte keys, the
/// shape `decode_protobuf` re-encodes when building the account-key list.
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
    // Inner-instruction `data` is base58 in the RPC `meta`, regardless of the
    // top-level `encoding` (it is a compiled instruction, not the parsed tx body).
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
                        // Only `uiTokenAmount.amount` (raw base units, as a string)
                        // is read downstream — mirror `adapter::token_balances`.
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

#[cfg(test)]
mod tests {
    //! End-to-end: build a real `VersionedTransaction`, bincode+base64 it exactly
    //! as `encoding="base64"` would, wrap it as token_sync's RPC path does, then
    //! decode through `rpc_to_protobuf -> decode_protobuf` and assert the trade.
    //! Proves the base64 -> `scb` lowering produces a frame `decode_protobuf`
    //! decodes into the expected trade (the `decoder::grpc` unit tests cover
    //! `decode_protobuf` itself).

    use base64::{engine::general_purpose::STANDARD, Engine};
    use borsh::BorshSerialize;
    use chrono::Utc;
    use serde_json::{json, Value};
    use solana_sdk::{
        hash::Hash,
        instruction::CompiledInstruction as SdkCompiledInstruction,
        message::{v0, MessageHeader, VersionedMessage},
        pubkey::Pubkey,
        signature::Signature,
        transaction::VersionedTransaction,
    };

    use super::rpc_to_protobuf;
    use super::super::decoder::{DecodeOutput, HeliusDecoder};
    use crate::config::constants::{
        BUY_DISCRIMINATOR, COMPUTE_BUDGET_PROGRAM_ID, PUMP_FUN_PROGRAM_ID, TRADE_EVENT_DISCRIMINATOR,
    };
    use crate::models::events::InternalEvent;

    fn pk(b: u8) -> Pubkey {
        Pubkey::new_from_array([b; 32])
    }
    fn id(s: &str) -> Pubkey {
        Pubkey::new_from_array(bs58::decode(s).into_vec().unwrap().try_into().unwrap())
    }

    /// Borsh layout of pump.fun's on-chain TradeEvent (serialize side).
    #[derive(BorshSerialize)]
    struct TestTradeEvent {
        mint: [u8; 32],
        sol_amount: u64,
        token_amount: u64,
        is_buy: bool,
        user: [u8; 32],
        timestamp: i64,
        virtual_sol_reserves: u64,
        virtual_token_reserves: u64,
        real_sol_reserves: u64,
        real_token_reserves: u64,
    }

    fn trade_event_log(mint: [u8; 32], user: [u8; 32], is_buy: bool) -> String {
        let ev = TestTradeEvent {
            mint,
            sol_amount: 1_500_000_000,
            token_amount: 42_000_000_000,
            is_buy,
            user,
            timestamp: 0,
            virtual_sol_reserves: 30_000_000_000,
            virtual_token_reserves: 1_000_000_000_000,
            real_sol_reserves: 5_000_000_000,
            real_token_reserves: 800_000_000_000,
        };
        let mut bytes = TRADE_EVENT_DISCRIMINATOR.to_vec();
        bytes.extend(borsh::to_vec(&ev).unwrap());
        format!("Program data: {}", STANDARD.encode(bytes))
    }

    #[test]
    fn rpc_base64_buy_decodes_through_protobuf() {
        let payer = pk(1);
        let pump = id(PUMP_FUN_PROGRAM_ID);
        let cbudget = id(COMPUTE_BUDGET_PROGRAM_ID);
        // account_keys index: 0=payer, 1=pump program, 2=compute budget program
        let account_keys = vec![payer, pump, cbudget];

        let cb_ix = SdkCompiledInstruction {
            program_id_index: 2,
            accounts: vec![],
            data: {
                let mut d = vec![2u8]; // SetComputeUnitLimit
                d.extend(200_000u32.to_le_bytes());
                d
            },
        };
        let buy_ix = SdkCompiledInstruction {
            program_id_index: 1,
            accounts: vec![0],
            data: {
                let mut d = BUY_DISCRIMINATOR.to_vec();
                d.extend([0u8; 16]);
                d
            },
        };

        let msg = v0::Message {
            header: MessageHeader {
                num_required_signatures: 1,
                num_readonly_signed_accounts: 0,
                num_readonly_unsigned_accounts: 2,
            },
            account_keys,
            recent_blockhash: Hash::default(),
            instructions: vec![cb_ix, buy_ix],
            address_table_lookups: vec![],
        };
        let vtx = VersionedTransaction {
            signatures: vec![Signature::default()],
            message: VersionedMessage::V0(msg),
        };

        // Encode exactly as `encoding="base64"` would (bincode wire format).
        let tx_b64 = STANDARD.encode(bincode::serialize(&vtx).unwrap());

        // Wrap as `helius_rpc::wrap_transaction_result` does for a base64 tx.
        let result = json!({
            "signature": bs58::encode([9u8; 64]).into_string(),
            "slot": 100,
            "blockTime": Value::Null,
            "transaction": {
                "transaction": [tx_b64, "base64"],
                "meta": {
                    "logMessages": [
                        format!("Program {PUMP_FUN_PROGRAM_ID} invoke [1]"),
                        trade_event_log([9u8; 32], [8u8; 32], true),
                        format!("Program {PUMP_FUN_PROGRAM_ID} success"),
                    ],
                    "innerInstructions": [],
                    "preBalances": [],
                    "postBalances": [],
                    "preTokenBalances": [],
                    "postTokenBalances": [],
                    "loadedAddresses": { "writable": [], "readonly": [] },
                }
            }
        });

        let update = rpc_to_protobuf(&result).expect("adapter should lower the RPC result");
        let decoder = HeliusDecoder::new(PUMP_FUN_PROGRAM_ID.to_string());
        let out = decoder.decode_protobuf(&update, Utc::now());

        let DecodeOutput::Transaction { events, .. } = out else {
            panic!("expected a decoded transaction");
        };
        let trades: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                InternalEvent::TradeExecuted(t) => Some(&t.trade),
                _ => None,
            })
            .collect();
        assert_eq!(trades.len(), 1, "expected exactly one buy");
        let t = trades[0];
        assert_eq!(t.mint_address, bs58::encode([9u8; 32]).into_string());
        assert_eq!(t.wallet_address, bs58::encode([8u8; 32]).into_string());
        assert_eq!(t.instruction_type, "Buy");
        let labels = t.instruction_labels.to_string();
        assert!(labels.contains("Pump.Fun: Buy"), "labels carry the buy: {labels}");
        assert!(
            labels.contains("Compute Budget: SetComputeUnitLimit"),
            "raw compute-budget data survived base64 lowering (the jsonParsed path would drop it): {labels}"
        );
    }
}
