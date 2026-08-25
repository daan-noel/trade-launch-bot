//! `rpc-backfill` feature: turn RPC `getTransaction` results into decoder frames,
//! plus the JSON-RPC pager that fetches them.
//!
//! The conversion itself lives in [`crate::convert`] — one adapter shared with
//! the live JSON feeds (NATS / WebSocket), so a frame decodes identically no
//! matter which transport delivered it.
//!
//! *Not* part of the live gRPC ingest path — only used by host backfill routines
//! (e.g. the token_sync AMM historical loop). The decoder cannot tell the sources
//! apart.

use serde_json::Value;

use crate::proto::geyser::SubscribeUpdateTransaction;

mod pager;
pub use pager::{
    get_signatures_for_address, get_transactions_batch, get_transactions_for_address_page,
    wrap_transaction_result, SignatureInfo,
};

/// Convert one RPC transaction result into a [`SubscribeUpdateTransaction`].
///
/// The expected shape is `{ signature, slot, blockTime, transaction: {
/// transaction: ["<b64>", "base64"], meta } }`. Returns `None` if any required
/// field is absent or the base64/bincode decode fails.
///
/// The backfill pager always requests `encoding="base64"`; [`crate::convert`]
/// also accepts `jsonParsed`, so a result fetched either way converts here.
pub fn rpc_to_protobuf(result: &Value) -> Option<SubscribeUpdateTransaction> {
    crate::convert::json_tx_to_protobuf(result)
}
