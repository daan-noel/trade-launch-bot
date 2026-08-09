//! `raw-tx` feature: capture the verbatim protobuf wire bytes of a
//! `SubscribeUpdateTransaction` and emit them as [`IngestEvent::RawTx`] for
//! persistence in the `raw_txs` hypertable (BYTEA `payload`).
//!
//! Runs entirely off the hot path (the decode task emits it alongside normal
//! events, after decode). The payload is the prost-encoded update — the exact
//! bytes a future replay/derive path re-`decode`s, with no JSON expansion or
//! lossy re-shape — bytes, not JSONB: a JSON expansion is both larger and lossy
//! against the protobuf, so it could not round-trip a replay.

use chrono::{DateTime, Utc};
use prost::Message;

use crate::event::{IngestEvent, RawTx};
use crate::proto::geyser::SubscribeUpdateTransaction;

/// Encode the verbatim protobuf wire bytes for `update` — the value stored in
/// `raw_txs.payload`. Used by both the live decode task and the token_sync
/// backfill so live and historical rows share one byte format.
pub fn encode_payload(update: &SubscribeUpdateTransaction) -> Vec<u8> {
    update.encode_to_vec()
}

/// Wrap `update` as [`IngestEvent::RawTx`] carrying its raw 64-byte signature,
/// block position (`tx_index`), and encoded payload. `block_time` is set to
/// `received_at` — the gRPC stream carries no on-chain block time. Returns
/// `None` if the update has no transaction info.
pub fn build_raw_tx_event(
    update: &SubscribeUpdateTransaction,
    received_at: DateTime<Utc>,
) -> Option<IngestEvent> {
    let info = update.transaction.as_ref()?;
    Some(IngestEvent::RawTx(RawTx {
        signature: info.signature.clone(),
        slot: update.slot,
        tx_index: info.index as u32,
        block_time: received_at,
        payload: encode_payload(update),
    }))
}
