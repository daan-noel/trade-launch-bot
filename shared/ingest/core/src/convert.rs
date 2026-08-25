//! Encoding-agnostic JSON transaction -> protobuf adapter — the ONE place a
//! JSON transaction frame becomes the [`SubscribeUpdateTransaction`] the
//! decoders speak.
//!
//! Three sources produce JSON transactions and all three land here:
//!
//! | Source | `transaction.transaction` shape |
//! | --- | --- |
//! | RPC `getTransaction(encoding="base64")` (backfill) | `["<b64>", "base64"]` |
//! | Helius `transactionSubscribe(encoding="base64")` (WS / NATS relay) | `["<b64>", "base64"]` |
//! | Helius `transactionSubscribe(encoding="jsonParsed")` (NATS relay) | `{ signatures, message }` |
//!
//! Both shapes are auto-detected from the JSON itself, so a publisher that
//! changes its encoding needs no code change here.
//!
//! # Why the jsonParsed output is byte-comparable to gRPC
//!
//! `jsonParsed` pre-resolves address-lookup-table keys **inline** into
//! `message.accountKeys`, tagging each with `source` (`transaction` vs
//! `lookupTable`) and `writable`. Solana emits that array in exactly the order
//! `static ++ loaded_writable ++ loaded_readonly` — which is the same flat index
//! space `decode::grpc::key_at` walks. This module therefore splits the array
//! back into `message.account_keys` + `meta.loaded_{writable,readonly}_addresses`
//! rather than flattening it, so a NATS-sourced update is shaped identically to
//! a gRPC-sourced one and `raw_txs.payload` stays one format.
//!
//! [`ORDER_VIOLATION_FALLBACK`] documents the (never-observed) escape hatch: if a
//! publisher ever emits the three groups interleaved, everything goes into
//! `account_keys` with the loaded vectors empty — indices still resolve, only the
//! static/loaded split is lost.
//!
//! # Known jsonParsed fidelity limits
//!
//! - Instructions belonging to a program the RPC node knows how to parse
//!   (`system`, `spl-token`, `spl-token-2022`, ATA) arrive as `{program, parsed}`
//!   with the raw `data` **dropped**. [`data_from_parsed`] re-encodes those bytes
//!   from `parsed`, because `ix_labels` is built from the instruction
//!   discriminator: an empty `data` labels every one of them `"System Program:
//!   Unknown"`. An instruction type the rebuild does not cover keeps an empty
//!   `data` and that `Unknown` label. pump.fun and ComputeBudget are not in the
//!   node's parsed set, so they arrive raw either way.
//! - `base64` is the preferred publisher encoding: about half the bytes, no
//!   pubkey->index remapping and no rebuild. This module exists so either works.

use std::collections::HashMap;

use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::Value;
use solana_sdk::{message::VersionedMessage, transaction::VersionedTransaction};

use crate::proto::geyser::{SubscribeUpdateTransaction, SubscribeUpdateTransactionInfo};
use crate::proto::solana::storage::confirmed_block as scb;

/// See the module docs: the fallback taken when `accountKeys` is not grouped
/// `static ++ lookupTable-writable ++ lookupTable-readonly`.
pub const ORDER_VIOLATION_FALLBACK: &str = "all keys -> account_keys, loaded_* empty";

/// Borrowed `pubkey -> flat account index`, over the same
/// `account_keys ++ loaded_writable ++ loaded_readonly` space the decoders index.
type KeyIndex<'a> = HashMap<&'a str, u32>;

/// Whether this transaction failed on chain.
///
/// The gRPC subscription filters failures server-side (`failed: Some(false)`), so
/// a JSON feed must apply the same screen itself to stay behaviour-identical.
pub fn json_tx_failed(result: &Value) -> bool {
    result
        .get("transaction")
        .and_then(|t| t.get("meta"))
        .and_then(|m| m.get("err"))
        .is_some_and(|e| !e.is_null())
}

/// Convert one JSON transaction result into a [`SubscribeUpdateTransaction`].
///
/// Accepts the `{ slot, signature, transactionIndex, transaction: { transaction,
/// meta } }` envelope common to RPC `getTransaction` results and Helius
/// `transactionNotification` payloads. Returns `None` if a required field is
/// absent or a decode fails — a partially-converted update would decode into
/// wrong trades, so this rejects rather than guesses.
pub fn json_tx_to_protobuf(result: &Value) -> Option<SubscribeUpdateTransaction> {
    let slot = result.get("slot")?.as_u64()?;
    let inner = result.get("transaction")?;
    let index = result
        .get("transactionIndex")
        .and_then(Value::as_u64)
        .unwrap_or(0);

    let tx_json = inner.get("transaction")?;
    let meta_json = inner.get("meta")?;

    // Base64 carries its own key/index layout inside the bincode blob; jsonParsed
    // needs the pubkey->index map to rebuild compiled instructions.
    let (transaction, keys, split) = if tx_json.is_array() {
        let tx = transaction_from_base64(tx_json)?;
        (tx, None, LoadedSplit::default())
    } else {
        let (tx, keys, split) = transaction_from_json_parsed(tx_json)?;
        (tx, Some(keys), split)
    };

    // The envelope signature is authoritative when present (the relay stamps it);
    // fall back to the first signature inside the transaction.
    let signature = result
        .get("signature")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .and_then(|s| bs58::decode(s).into_vec().ok())
        .or_else(|| transaction.signatures.first().cloned())
        .unwrap_or_default();

    let meta = meta_from_json(meta_json, keys.as_ref(), split);

    Some(SubscribeUpdateTransaction {
        transaction: Some(SubscribeUpdateTransactionInfo {
            signature,
            is_vote: false,
            transaction: Some(transaction),
            meta: Some(meta),
            index,
        }),
        slot,
    })
}

// -- base64 (bincode VersionedTransaction) ------------------------------------

fn transaction_from_base64(tx_json: &Value) -> Option<scb::Transaction> {
    let tx_b64 = tx_json.get(0)?.as_str()?;
    let tx_bytes = STANDARD.decode(tx_b64).ok()?;
    let vtx: VersionedTransaction = bincode::deserialize(&tx_bytes).ok()?;

    let (account_keys, instructions, header, lookups, blockhash, versioned) = match &vtx.message {
        VersionedMessage::Legacy(m) => (
            &m.account_keys,
            &m.instructions,
            &m.header,
            None,
            m.recent_blockhash,
            false,
        ),
        VersionedMessage::V0(m) => (
            &m.account_keys,
            &m.instructions,
            &m.header,
            Some(&m.address_table_lookups),
            m.recent_blockhash,
            true,
        ),
    };

    Some(scb::Transaction {
        signatures: vtx.signatures.iter().map(|s| s.as_ref().to_vec()).collect(),
        message: Some(scb::Message {
            header: Some(scb::MessageHeader {
                num_required_signatures: header.num_required_signatures as u32,
                num_readonly_signed_accounts: header.num_readonly_signed_accounts as u32,
                num_readonly_unsigned_accounts: header.num_readonly_unsigned_accounts as u32,
            }),
            account_keys: account_keys.iter().map(|k| k.to_bytes().to_vec()).collect(),
            recent_blockhash: blockhash.to_bytes().to_vec(),
            instructions: instructions
                .iter()
                .map(|ix| scb::CompiledInstruction {
                    program_id_index: ix.program_id_index as u32,
                    accounts: ix.accounts.clone(),
                    data: ix.data.clone(),
                })
                .collect(),
            versioned,
            address_table_lookups: lookups
                .map(|ls| {
                    ls.iter()
                        .map(|l| scb::MessageAddressTableLookup {
                            account_key: l.account_key.to_bytes().to_vec(),
                            writable_indexes: l.writable_indexes.clone(),
                            readonly_indexes: l.readonly_indexes.clone(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
        }),
    })
}

// -- jsonParsed ---------------------------------------------------------------

/// The lookup-table keys lifted out of `accountKeys`, ready for `meta`.
#[derive(Default)]
struct LoadedSplit {
    writable: Vec<Vec<u8>>,
    readonly: Vec<Vec<u8>>,
}

fn transaction_from_json_parsed(
    tx_json: &Value,
) -> Option<(scb::Transaction, KeyIndex<'_>, LoadedSplit)> {
    let msg = tx_json.get("message")?;
    let raw_keys = msg.get("accountKeys")?.as_array()?;

    let mut index: KeyIndex = HashMap::with_capacity(raw_keys.len());
    let mut statics: Vec<Vec<u8>> = Vec::with_capacity(raw_keys.len());
    let mut split = LoadedSplit::default();
    // Group each key while remembering the group order actually seen, so the
    // `static ++ writable ++ readonly` assumption is verified, not trusted.
    let mut groups: Vec<u8> = Vec::with_capacity(raw_keys.len());
    let mut header = scb::MessageHeader::default();
    let mut have_flags = false;

    for (i, k) in raw_keys.iter().enumerate() {
        let (pubkey, from_lut, writable, signer) = match k.as_str() {
            // `encoding: "json"` — bare strings, no flags, no ALT resolution.
            Some(s) => (s, false, false, false),
            None => (
                k.get("pubkey")?.as_str()?,
                k.get("source").and_then(Value::as_str) == Some("lookupTable"),
                k.get("writable").and_then(Value::as_bool).unwrap_or(false),
                k.get("signer").and_then(Value::as_bool).unwrap_or(false),
            ),
        };
        let bytes = bs58::decode(pubkey).into_vec().ok()?;

        // First occurrence wins: a duplicate pubkey is not legal in a Solana
        // message, and picking the later index would silently shift accounts.
        index.entry(pubkey).or_insert(i as u32);

        if k.is_object() {
            have_flags = true;
            if !from_lut {
                if signer {
                    header.num_required_signatures += 1;
                    if !writable {
                        header.num_readonly_signed_accounts += 1;
                    }
                } else if !writable {
                    header.num_readonly_unsigned_accounts += 1;
                }
            }
        }

        match (from_lut, writable) {
            (false, _) => {
                groups.push(0);
                statics.push(bytes);
            }
            (true, true) => {
                groups.push(1);
                split.writable.push(bytes);
            }
            (true, false) => {
                groups.push(2);
                split.readonly.push(bytes);
            }
        }
    }

    // If the groups are not contiguous and ascending, the split would reorder the
    // index space. Fall back to one flat `account_keys` (see the module docs).
    if !groups.windows(2).all(|w| w[0] <= w[1]) {
        statics = raw_keys
            .iter()
            .filter_map(|k| {
                k.as_str()
                    .or_else(|| k.get("pubkey").and_then(Value::as_str))
            })
            .filter_map(|s| bs58::decode(s).into_vec().ok())
            .collect();
        if statics.len() != raw_keys.len() {
            return None;
        }
        split = LoadedSplit::default();
    }

    let instructions = msg
        .get("instructions")?
        .as_array()?
        .iter()
        .map(|ix| {
            let (program_id_index, accounts, data) = compiled_parts(ix, &index)?;
            Some(scb::CompiledInstruction {
                program_id_index,
                accounts,
                data,
            })
        })
        .collect::<Option<Vec<_>>>()?;

    let lookups: Vec<scb::MessageAddressTableLookup> = msg
        .get("addressTableLookups")
        .and_then(Value::as_array)
        .map(|ls| {
            ls.iter()
                .filter_map(|l| {
                    Some(scb::MessageAddressTableLookup {
                        account_key: bs58::decode(l.get("accountKey")?.as_str()?)
                            .into_vec()
                            .ok()?,
                        writable_indexes: u8_vec(l.get("writableIndexes")),
                        readonly_indexes: u8_vec(l.get("readonlyIndexes")),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let transaction = scb::Transaction {
        signatures: tx_json
            .get("signatures")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .filter_map(|s| bs58::decode(s).into_vec().ok())
                    .collect()
            })
            .unwrap_or_default(),
        message: Some(scb::Message {
            header: have_flags.then_some(header),
            account_keys: statics,
            recent_blockhash: msg
                .get("recentBlockhash")
                .and_then(Value::as_str)
                .and_then(|s| bs58::decode(s).into_vec().ok())
                .unwrap_or_default(),
            instructions,
            versioned: !lookups.is_empty(),
            address_table_lookups: lookups,
        }),
    };

    Some((transaction, index, split))
}

/// `(program_id_index, accounts, data)` for one jsonParsed instruction.
///
/// An unresolvable pubkey means the frame is malformed (every instruction account
/// is by construction a message account); rejecting the whole transaction is the
/// only safe outcome, since a shifted index decodes into a different wallet.
fn compiled_parts(ix: &Value, index: &KeyIndex<'_>) -> Option<(u32, Vec<u8>, Vec<u8>)> {
    let program_id_index = match ix.get("programIdIndex").and_then(Value::as_u64) {
        Some(i) => i as u32,
        None => *index.get(ix.get("programId")?.as_str()?)?,
    };

    let accounts = match ix.get("accounts").and_then(Value::as_array) {
        Some(a) => {
            let mut out = Vec::with_capacity(a.len());
            for v in a {
                let i = match v.as_u64() {
                    Some(i) => i,
                    None => u64::from(*index.get(v.as_str()?)?),
                };
                out.push(u8::try_from(i).ok()?);
            }
            out
        }
        None => Vec::new(),
    };

    // `{program, parsed}` instructions carry no `data` — the node consumed the
    // raw bytes. Rebuilding them is what keeps `ix_labels` readable; see
    // [`data_from_parsed`].
    let data = match ix.get("data").and_then(Value::as_str) {
        Some(d) => bs58::decode(d).into_vec().ok()?,
        None => data_from_parsed(ix).unwrap_or_default(),
    };

    Some((program_id_index, accounts, data))
}

// -- jsonParsed instruction data ----------------------------------------------

/// Rebuild the raw instruction `data` that a `{program, parsed}` instruction
/// dropped, or `None` when this instruction type is not covered.
///
/// The RPC node consumes the bytes for every program it recognises, so a
/// jsonParsed frame carries `parsed` instead of `data` for `system`,
/// `spl-token`, `spl-token-2022` and ATA instructions. `ix_labels` is derived
/// from those bytes (`decode::instructions::label_instruction` reads the
/// discriminator), so an empty `data` renders each one `"System Program:
/// Unknown"` and the ix-pattern fingerprints go blind on a NATS-sourced frame.
///
/// Only **byte-exact** re-encodings are produced: an instruction is rebuilt
/// exactly as its program serialises it, or nothing is emitted. A truncated or
/// invented payload would be indistinguishable from a real one downstream, so an
/// uncovered type (or a `parsed` shape that does not carry every argument) keeps
/// its empty `data` and its `Unknown` label — the pre-existing behaviour.
fn data_from_parsed(ix: &Value) -> Option<Vec<u8>> {
    let parsed = ix.get("parsed")?;
    let ty = parsed.get("type")?.as_str()?;
    let info = parsed.get("info")?;
    match ix.get("program")?.as_str()? {
        "system" => system_data(ty, info),
        "spl-token" | "spl-token-2022" => token_data(ty, info),
        "spl-associated-token-account" => ata_data(ty),
        // `vote`, `stake`, the loaders, `spl-memo` and address-lookup-table are
        // also parsed by the node; no label path names their instructions, so
        // rebuilding them would buy nothing.
        _ => None,
    }
}

/// `SystemInstruction`, bincode: `u32` LE enum tag, `u64` LE `String` lengths.
fn system_data(ty: &str, info: &Value) -> Option<Vec<u8>> {
    let e = Enc::tag32;
    Some(match ty {
        "createAccount" => e(0)
            .u64(num(info, "lamports")?)
            .u64(num(info, "space")?)
            .key(pubkey(info, "owner")?),
        "assign" => e(1).key(pubkey(info, "owner")?),
        "transfer" => e(2).u64(num(info, "lamports")?),
        "createAccountWithSeed" => e(3)
            .key(pubkey(info, "base")?)
            .string(text(info, "seed")?)
            .u64(num(info, "lamports")?)
            .u64(num(info, "space")?)
            .key(pubkey(info, "owner")?),
        "advanceNonce" => e(4),
        "withdrawFromNonce" => e(5).u64(num(info, "lamports")?),
        "initializeNonce" => e(6).key(pubkey(info, "nonceAuthority")?),
        "authorizeNonce" => e(7).key(pubkey(info, "newAuthorized")?),
        "allocate" => e(8).u64(num(info, "space")?),
        "allocateWithSeed" => e(9)
            .key(pubkey(info, "base")?)
            .string(text(info, "seed")?)
            .u64(num(info, "space")?)
            .key(pubkey(info, "owner")?),
        "assignWithSeed" => e(10)
            .key(pubkey(info, "base")?)
            .string(text(info, "seed")?)
            .key(pubkey(info, "owner")?),
        "transferWithSeed" => e(11)
            .u64(num(info, "lamports")?)
            .string(text(info, "sourceSeed")?)
            .key(pubkey(info, "sourceOwner")?),
        "upgradeNonce" => e(12),
        _ => return None,
    }
    .into())
}

/// `TokenInstruction`, hand-packed: `u8` tag then little-endian fields. Token
/// 2022 shares the base layout; its extension instructions (tag >= 26) carry a
/// sub-discriminator and are left uncovered.
fn token_data(ty: &str, info: &Value) -> Option<Vec<u8>> {
    let e = Enc::tag8;
    Some(match ty {
        "initializeAccount" => e(1),
        "transfer" => e(3).u64(amount(info)?),
        "approve" => e(4).u64(amount(info)?),
        "revoke" => e(5),
        "mintTo" => e(7).u64(amount(info)?),
        "burn" => e(8).u64(amount(info)?),
        "closeAccount" => e(9),
        "freezeAccount" => e(10),
        "thawAccount" => e(11),
        "transferChecked" => e(12).u64(checked_amount(info)?).u8(decimals(info)?),
        "approveChecked" => e(13).u64(checked_amount(info)?).u8(decimals(info)?),
        "mintToChecked" => e(14).u64(checked_amount(info)?).u8(decimals(info)?),
        "burnChecked" => e(15).u64(checked_amount(info)?).u8(decimals(info)?),
        "initializeAccount2" => e(16).key(pubkey(info, "owner")?),
        "syncNative" => e(17),
        "initializeAccount3" => e(18).key(pubkey(info, "owner")?),
        "initializeImmutableOwner" => e(22),
        _ => return None,
    }
    .into())
}

/// `AssociatedTokenAccountInstruction`: a bare `u8` tag, and the legacy `Create`
/// is genuinely zero bytes. Without this, `createIdempotent` — which carries the
/// same empty `data` as `Create` under jsonParsed — labels as `Create`, a *wrong*
/// label rather than an unknown one.
fn ata_data(ty: &str) -> Option<Vec<u8>> {
    match ty {
        "create" => Some(Vec::new()),
        "createIdempotent" => Some(vec![1]),
        "recoverNested" => Some(vec![2]),
        _ => None,
    }
}

// Field readers. A missing or malformed field yields `None`, which drops the
// whole rebuild rather than emitting a short payload.

/// A `u64` argument. The parsers emit lamport-style values as JSON numbers and
/// token amounts as strings; accept either.
fn num(info: &Value, key: &str) -> Option<u64> {
    let v = info.get(key)?;
    v.as_u64().or_else(|| v.as_str()?.parse().ok())
}

fn text<'a>(info: &'a Value, key: &str) -> Option<&'a str> {
    info.get(key)?.as_str()
}

fn pubkey(info: &Value, key: &str) -> Option<[u8; 32]> {
    bs58::decode(text(info, key)?).into_vec().ok()?.try_into().ok()
}

/// Unchecked token instructions carry the raw amount as a decimal string.
fn amount(info: &Value) -> Option<u64> {
    num(info, "amount")
}

/// Checked variants nest it in `tokenAmount` alongside the decimals the
/// instruction pins.
fn checked_amount(info: &Value) -> Option<u64> {
    num(info.get("tokenAmount")?, "amount")
}

fn decimals(info: &Value) -> Option<u8> {
    u8::try_from(num(info.get("tokenAmount")?, "decimals")?).ok()
}

/// Little-endian instruction-data builder. Every method appends; the tag
/// constructors fix the discriminator width the program uses.
struct Enc(Vec<u8>);

impl Enc {
    fn tag32(tag: u32) -> Self {
        Enc(tag.to_le_bytes().to_vec())
    }
    fn tag8(tag: u8) -> Self {
        Enc(vec![tag])
    }
    fn u8(mut self, v: u8) -> Self {
        self.0.push(v);
        self
    }
    fn u64(mut self, v: u64) -> Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn key(mut self, k: [u8; 32]) -> Self {
        self.0.extend_from_slice(&k);
        self
    }
    /// bincode `String`: a `u64` LE byte length, then the bytes.
    fn string(mut self, s: &str) -> Self {
        self.0.extend_from_slice(&(s.len() as u64).to_le_bytes());
        self.0.extend_from_slice(s.as_bytes());
        self
    }
}

impl From<Enc> for Vec<u8> {
    fn from(e: Enc) -> Self {
        e.0
    }
}

// -- meta ---------------------------------------------------------------------

fn meta_from_json(
    meta: &Value,
    index: Option<&KeyIndex<'_>>,
    split: LoadedSplit,
) -> scb::TransactionStatusMeta {
    // `loadedAddresses` is present on base64 frames and absent on jsonParsed ones
    // (already inlined, and lifted back out into `split`).
    let loaded = meta.get("loadedAddresses");
    scb::TransactionStatusMeta {
        err: meta
            .get("err")
            .filter(|e| !e.is_null())
            .map(|e| scb::TransactionError {
                err: e.to_string().into_bytes(),
            }),
        // Without this the struct default (0) reaches the decoder and every
        // backfilled trade reports a zero fee — which reads as "free", not as
        // "unknown". `event::fee_lamports_opt` folds a real 0 back to None.
        fee: meta.get("fee").and_then(Value::as_u64).unwrap_or(0),
        log_messages: str_vec(meta.get("logMessages")),
        pre_balances: u64_vec(meta.get("preBalances")),
        post_balances: u64_vec(meta.get("postBalances")),
        inner_instructions: inner_from_json(meta.get("innerInstructions"), index),
        pre_token_balances: token_balances_from_json(meta.get("preTokenBalances")),
        post_token_balances: token_balances_from_json(meta.get("postTokenBalances")),
        loaded_writable_addresses: loaded
            .and_then(|l| l.get("writable"))
            .map(key_vec)
            .unwrap_or(split.writable),
        loaded_readonly_addresses: loaded
            .and_then(|l| l.get("readonly"))
            .map(key_vec)
            .unwrap_or(split.readonly),
        compute_units_consumed: meta.get("computeUnitsConsumed").and_then(Value::as_u64),
        ..Default::default()
    }
}

fn str_vec(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

fn u64_vec(v: Option<&Value>) -> Vec<u64> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_u64).collect())
        .unwrap_or_default()
}

fn u8_vec(v: Option<&Value>) -> Vec<u8> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_u64)
                .filter_map(|n| u8::try_from(n).ok())
                .collect()
        })
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

fn inner_from_json(v: Option<&Value>, index: Option<&KeyIndex<'_>>) -> Vec<scb::InnerInstructions> {
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
                            .filter_map(|ix| inner_ix_from_json(ix, index))
                            .collect(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn inner_ix_from_json(ix: &Value, index: Option<&KeyIndex<'_>>) -> Option<scb::InnerInstruction> {
    // base64 frames carry numeric `programIdIndex`/`accounts`; jsonParsed frames
    // carry pubkey strings. `compiled_parts` reads whichever is present.
    let empty = KeyIndex::new();
    let (program_id_index, accounts, data) = compiled_parts(ix, index.unwrap_or(&empty))?;
    Some(scb::InnerInstruction {
        program_id_index,
        accounts,
        data,
        stack_height: ix
            .get("stackHeight")
            .and_then(Value::as_u64)
            .map(|s| s as u32),
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn key(b: u8) -> String {
        bs58::encode([b; 32]).into_string()
    }

    /// The flat index space a decoder walks: `account_keys ++ loaded_writable ++
    /// loaded_readonly`. Mirrors `decode::grpc::key_at` in the venue crate.
    fn flat(u: &SubscribeUpdateTransaction) -> Vec<String> {
        let info = u.transaction.as_ref().unwrap();
        let m = info.transaction.as_ref().unwrap().message.as_ref().unwrap();
        let meta = info.meta.as_ref().unwrap();
        m.account_keys
            .iter()
            .chain(meta.loaded_writable_addresses.iter())
            .chain(meta.loaded_readonly_addresses.iter())
            .map(|k| bs58::encode(k).into_string())
            .collect()
    }

    fn parsed_frame() -> Value {
        json!({
            "slot": 441652280u64,
            "signature": bs58::encode([9u8; 64]).into_string(),
            "transactionIndex": 157u64,
            "transaction": {
                "transaction": {
                    "signatures": [bs58::encode([9u8; 64]).into_string()],
                    "message": {
                        "accountKeys": [
                            {"pubkey": key(1), "writable": true,  "signer": true,  "source": "transaction"},
                            {"pubkey": key(2), "writable": false, "signer": false, "source": "transaction"},
                            {"pubkey": key(3), "writable": true,  "signer": false, "source": "lookupTable"},
                            {"pubkey": key(4), "writable": false, "signer": false, "source": "lookupTable"}
                        ],
                        "recentBlockhash": key(9),
                        "instructions": [
                            {"programId": key(2), "accounts": [key(1), key(3), key(4)], "data": "3Bxs", "stackHeight": 1},
                            {"program": "spl-token", "programId": key(4), "accounts": [key(1)],
                             "parsed": {"type": "transfer", "info": {"source": key(1), "destination": key(3), "amount": "1500", "authority": key(1)}}, "stackHeight": 1}
                        ],
                        "addressTableLookups": [
                            {"accountKey": key(8), "writableIndexes": [0], "readonlyIndexes": [1]}
                        ]
                    }
                },
                "meta": {
                    "err": null,
                    "fee": 5000u64,
                    "preBalances": [10u64, 0, 0, 0],
                    "postBalances": [9u64, 0, 0, 0],
                    "logMessages": ["Program log: hi"],
                    "computeUnitsConsumed": 1234u64,
                    "innerInstructions": [
                        {"index": 0, "instructions": [
                            {"programId": key(3), "accounts": [key(2), key(4)], "data": "3Bxs", "stackHeight": 2}
                        ]}
                    ],
                    "preTokenBalances": [],
                    "postTokenBalances": []
                }
            }
        })
    }

    /// The whole reason the split is safe: the flat index space after conversion
    /// must equal the publisher's `accountKeys` order, position for position.
    #[test]
    fn parsed_keys_preserve_the_flat_index_space() {
        let u = json_tx_to_protobuf(&parsed_frame()).unwrap();
        assert_eq!(flat(&u), vec![key(1), key(2), key(3), key(4)]);

        let info = u.transaction.as_ref().unwrap();
        let meta = info.meta.as_ref().unwrap();
        let m = info.transaction.as_ref().unwrap().message.as_ref().unwrap();
        // Static vs loaded really is split, not flattened.
        assert_eq!(m.account_keys.len(), 2);
        assert_eq!(meta.loaded_writable_addresses.len(), 1);
        assert_eq!(meta.loaded_readonly_addresses.len(), 1);
    }

    #[test]
    fn parsed_instructions_map_pubkeys_back_to_indices() {
        let u = json_tx_to_protobuf(&parsed_frame()).unwrap();
        let info = u.transaction.as_ref().unwrap();
        let m = info.transaction.as_ref().unwrap().message.as_ref().unwrap();

        assert_eq!(m.instructions[0].program_id_index, 1);
        assert_eq!(m.instructions[0].accounts, vec![0u8, 2, 3]);
        assert!(!m.instructions[0].data.is_empty());

        // A `parsed` instruction keeps its program + accounts, and the `data` the
        // node consumed is re-encoded (spl-token `transfer` = tag 3 + u64 amount).
        assert_eq!(m.instructions[1].program_id_index, 3);
        assert_eq!(m.instructions[1].accounts, vec![0u8]);
        assert_eq!(m.instructions[1].data, [&[3u8][..], &1500u64.to_le_bytes()].concat());

        let inner = &info.meta.as_ref().unwrap().inner_instructions[0].instructions[0];
        assert_eq!(inner.program_id_index, 2);
        assert_eq!(inner.accounts, vec![1u8, 3]);
        assert_eq!(inner.stack_height, Some(2));
    }

    #[test]
    fn parsed_envelope_fields_survive() {
        let u = json_tx_to_protobuf(&parsed_frame()).unwrap();
        assert_eq!(u.slot, 441652280);
        let info = u.transaction.as_ref().unwrap();
        assert_eq!(info.index, 157);
        assert_eq!(info.signature, vec![9u8; 64]);
        let meta = info.meta.as_ref().unwrap();
        assert_eq!(meta.fee, 5000);
        assert_eq!(meta.compute_units_consumed, Some(1234));
        assert!(meta.err.is_none());
        let m = info.transaction.as_ref().unwrap().message.as_ref().unwrap();
        assert!(m.versioned);
        assert_eq!(m.address_table_lookups.len(), 1);
        // Derived from the per-key signer/writable flags, which gRPC sends outright.
        let h = m.header.as_ref().unwrap();
        assert_eq!(h.num_required_signatures, 1);
        assert_eq!(h.num_readonly_signed_accounts, 0);
        assert_eq!(h.num_readonly_unsigned_accounts, 1);
    }

    #[test]
    fn failed_transactions_are_detectable() {
        assert!(!json_tx_failed(&parsed_frame()));
        let mut f = parsed_frame();
        f["transaction"]["meta"]["err"] = json!({"InstructionError": [0, "Custom"]});
        assert!(json_tx_failed(&f));
        // Still converts — the caller decides whether to drop it.
        let u = json_tx_to_protobuf(&f).unwrap();
        assert!(u.transaction.unwrap().meta.unwrap().err.is_some());
    }

    #[test]
    fn an_unresolvable_instruction_account_rejects_the_transaction() {
        let mut f = parsed_frame();
        f["transaction"]["transaction"]["message"]["instructions"][0]["accounts"][0] =
            json!(key(77));
        assert!(json_tx_to_protobuf(&f).is_none());
    }

    #[test]
    fn interleaved_key_groups_fall_back_to_one_flat_vector() {
        let mut f = parsed_frame();
        // lookupTable key before a static one — the order the split relies on.
        f["transaction"]["transaction"]["message"]["accountKeys"] = json!([
            {"pubkey": key(3), "writable": true,  "signer": false, "source": "lookupTable"},
            {"pubkey": key(1), "writable": true,  "signer": true,  "source": "transaction"},
            {"pubkey": key(2), "writable": false, "signer": false, "source": "transaction"},
            {"pubkey": key(4), "writable": false, "signer": false, "source": "lookupTable"}
        ]);
        f["transaction"]["transaction"]["message"]["instructions"][0]["accounts"] =
            json!([key(3), key(1)]);
        f["transaction"]["transaction"]["message"]["instructions"][0]["programId"] = json!(key(2));
        f["transaction"]["meta"]["innerInstructions"] = json!([]);

        let u = json_tx_to_protobuf(&f).unwrap();
        // Index space still matches the publisher's order — only the split is lost.
        assert_eq!(flat(&u), vec![key(3), key(1), key(2), key(4)]);
        let info = u.transaction.as_ref().unwrap();
        assert!(info
            .meta
            .as_ref()
            .unwrap()
            .loaded_writable_addresses
            .is_empty());
        let m = info.transaction.as_ref().unwrap().message.as_ref().unwrap();
        assert_eq!(m.instructions[0].accounts, vec![0u8, 1]);
    }

    /// `encoding: "json"` (bare string keys, no ALT resolution) still converts.
    #[test]
    fn bare_string_account_keys_are_accepted() {
        let mut f = parsed_frame();
        f["transaction"]["transaction"]["message"]["accountKeys"] =
            json!([key(1), key(2), key(3), key(4)]);
        let u = json_tx_to_protobuf(&f).unwrap();
        assert_eq!(flat(&u), vec![key(1), key(2), key(3), key(4)]);
        let m = u
            .transaction
            .as_ref()
            .unwrap()
            .transaction
            .as_ref()
            .unwrap()
            .message
            .as_ref()
            .unwrap();
        // No per-key flags to derive a header from.
        assert!(m.header.is_none());
    }

    // -- jsonParsed instruction-data rebuild ----------------------------------

    fn parsed_ix(program: &str, ty: &str, info: Value) -> Value {
        json!({"program": program, "parsed": {"type": ty, "info": info}})
    }

    /// The tags and field widths here duplicate a fact solana-sdk owns. Rebuild
    /// against its own encoder rather than against a hand-written expectation, so
    /// the copy cannot drift: a `SystemInstruction` built by `system_instruction`
    /// must serialise to exactly what `data_from_parsed` produces for the frame
    /// the node emits for it.
    #[test]
    fn system_rebuild_matches_the_sdk_encoder() {
        use solana_sdk::{pubkey::Pubkey, system_instruction};

        let pk = |b: u8| Pubkey::new_from_array([b; 32]);

        let cases: Vec<(Value, Vec<u8>)> = vec![
            (
                parsed_ix(
                    "system",
                    "transfer",
                    json!({"source": key(1), "destination": key(2), "lamports": 890_880u64}),
                ),
                system_instruction::transfer(&pk(1), &pk(2), 890_880).data,
            ),
            (
                parsed_ix(
                    "system",
                    "advanceNonce",
                    json!({
                        "nonceAccount": key(1),
                        "recentBlockhashesSysvar": key(9),
                        "nonceAuthority": key(2),
                    }),
                ),
                system_instruction::advance_nonce_account(&pk(1), &pk(2)).data,
            ),
            (
                parsed_ix(
                    "system",
                    "createAccount",
                    json!({
                        "source": key(1), "newAccount": key(2),
                        "lamports": 2_039_280u64, "space": 165u64, "owner": key(3),
                    }),
                ),
                system_instruction::create_account(&pk(1), &pk(2), 2_039_280, 165, &pk(3)).data,
            ),
            (
                parsed_ix("system", "allocate", json!({"account": key(1), "space": 165u64})),
                system_instruction::allocate(&pk(1), 165).data,
            ),
            (
                parsed_ix("system", "assign", json!({"account": key(1), "owner": key(3)})),
                system_instruction::assign(&pk(1), &pk(3)).data,
            ),
            (
                parsed_ix(
                    "system",
                    "createAccountWithSeed",
                    json!({
                        "source": key(1), "newAccount": key(2), "base": key(1),
                        "seed": "nonce-seed", "lamports": 1_500_000u64, "space": 80u64,
                        "owner": key(3),
                    }),
                ),
                system_instruction::create_account_with_seed(
                    &pk(1), &pk(2), &pk(1), "nonce-seed", 1_500_000, 80, &pk(3),
                )
                .data,
            ),
        ];

        for (ix, want) in cases {
            assert_eq!(data_from_parsed(&ix), Some(want), "{ix}");
        }
    }

    /// spl-token packs a `u8` tag then little-endian fields; the crate is not a
    /// dependency here, so the layouts are pinned literally.
    #[test]
    fn token_rebuild_pins_the_spl_layouts() {
        let transfer = parsed_ix(
            "spl-token",
            "transfer",
            json!({"source": key(1), "destination": key(2), "amount": "42"}),
        );
        assert_eq!(
            data_from_parsed(&transfer),
            Some([&[3u8][..], &42u64.to_le_bytes()].concat()),
        );

        let checked = parsed_ix(
            "spl-token-2022",
            "transferChecked",
            json!({
                "source": key(1), "mint": key(2), "destination": key(3),
                "tokenAmount": {"amount": "7", "decimals": 6, "uiAmountString": "0.000007"},
            }),
        );
        assert_eq!(
            data_from_parsed(&checked),
            Some([&[12u8][..], &7u64.to_le_bytes(), &[6u8][..]].concat()),
        );

        let close = parsed_ix(
            "spl-token",
            "closeAccount",
            json!({"account": key(1), "destination": key(2), "owner": key(1)}),
        );
        assert_eq!(data_from_parsed(&close), Some(vec![9]));
    }

    /// `createIdempotent` shares the legacy `Create`'s empty jsonParsed `data`, so
    /// without the rebuild it labels as `Create` — a wrong label, not an unknown
    /// one. The legacy `Create` really is zero bytes on chain.
    #[test]
    fn ata_create_idempotent_is_distinguishable_from_create() {
        let ata = |ty| parsed_ix("spl-associated-token-account", ty, json!({"wallet": key(1)}));
        assert_eq!(data_from_parsed(&ata("create")), Some(vec![]));
        assert_eq!(data_from_parsed(&ata("createIdempotent")), Some(vec![1]));
        assert_eq!(data_from_parsed(&ata("recoverNested")), Some(vec![2]));
    }

    /// Exact-or-nothing: an uncovered type, an unparsed program, and a missing
    /// argument all leave `data` empty rather than emitting a short payload.
    #[test]
    fn an_incomplete_parsed_instruction_rebuilds_nothing() {
        // `setAuthority` carries a COption the parsed view flattens - uncovered.
        assert_eq!(
            data_from_parsed(&parsed_ix("spl-token", "setAuthority", json!({"account": key(1)}))),
            None,
        );
        // A parsed program with no label path of its own.
        assert_eq!(
            data_from_parsed(&parsed_ix("vote", "vote", json!({"voteAccount": key(1)}))),
            None,
        );
        // Covered type, argument absent.
        assert_eq!(
            data_from_parsed(&parsed_ix("system", "transfer", json!({"source": key(1)}))),
            None,
        );
        // A raw (unparsed) instruction has no `parsed` at all.
        assert_eq!(data_from_parsed(&json!({"programId": key(1), "data": "3Bxs"})), None);
    }
}
