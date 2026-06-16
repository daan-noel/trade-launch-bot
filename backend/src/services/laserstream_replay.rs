//! One-shot LaserStream (Yellowstone gRPC) replay fetch for incremental syncs.
//!
//! The live ingest (`ingest_laserstream::client`) holds a *forever* subscription;
//! this is its short-lived sibling: open a fresh `Subscribe` stream filtered to a
//! single account (a token's bonding curve or its PumpSwap pool), replay from the
//! sync watermark's slot, drain the replayed transactions, and disconnect. Each
//! transaction is returned as its native `SubscribeUpdateTransaction` protobuf —
//! the exact frame the live ingest decodes — so the caller runs the same
//! `decode_protobuf` hot path (no `Value` synthesis on either side).
//!
//! Why this saves credits: replay is part of the streaming subscription, billed
//! per connection, not per `getTransaction`. For a token synced within the
//! server's replay window (~24h), Fetch New can pull the new trades here for zero
//! Helius credits instead of `getSignaturesForAddress` + per-tx `getTransaction`.
//!
//! Safety: the caller advances the watermark only to the *max slot actually
//! returned here*, never to chain tip — so an early/partial drain can never skip
//! data permanently; the next Fetch New simply replays again from the same slot.
//! Any error (or a too-old slot the server can't replay) propagates so the caller
//! falls back to the RPC path.

use std::time::Duration;

use anyhow::Context;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tracing::info;

use crate::config::constants::PUMP_SWAP_PROGRAM_ID;
use crate::ingest_laserstream::client::{build_subscribe_request, connect};
use crate::ingest_laserstream::proto::geyser::subscribe_update::UpdateOneof;
use crate::ingest_laserstream::proto::geyser::{SubscribeRequest, SubscribeUpdateTransaction};

/// Outbound request queue depth (just the single initial subscribe).
const REQUEST_QUEUE_CAP: usize = 4;
/// Max wait for the FIRST replayed message (connect + subscribe + replay start).
/// If nothing arrives in this window we treat the range as already-caught-up and
/// return empty — safe, because the watermark isn't advanced past unfetched data.
const FIRST_MSG_TIMEOUT: Duration = Duration::from_secs(8);
/// Inter-message idle that signals "replay burst finished, now live/sparse".
/// Replayed txs stream back-to-back (sub-10ms apart); a gap this long means we've
/// caught up to real time for this single account.
const IDLE_TIMEOUT: Duration = Duration::from_millis(2000);
/// Absolute cap so a sync can never hang on a slow/stuck stream.
const HARD_DEADLINE: Duration = Duration::from_secs(90);
/// Count cap alongside the time cap: a Fetch-New replay covers the small gap
/// since the last sync, so a burst this large means the watermark is far older
/// than the replay window is meant for — stop and let the caller fall back to the
/// RPC path rather than buffering an unbounded `Vec` on the live process.
const MAX_REPLAY_TXS: usize = 50_000;

/// A replayed transaction as its native protobuf frame, fed straight to
/// `decode_protobuf` (no `Value` synthesis).
pub struct ReplayedTx {
    pub slot: u64,
    pub update: SubscribeUpdateTransaction,
}

/// Replay transactions referencing `account` from `from_slot` (inclusive) up to
/// the live tip, then disconnect. Returns them slot-ascending.
///
/// `account` is a single bonding-curve or pool address; Yellowstone's
/// `account_include` matches any transaction touching it, so this yields exactly
/// that venue's transactions for the token. The boundary slot is re-delivered
/// (inclusive `from_slot`); the caller dedups against already-saved signatures,
/// so same-slot siblings are never missed.
pub async fn replay_account_from_slot(
    laserstream_url: &str,
    api_key: &str,
    account: &str,
    from_slot: u64,
    pump_program_id: &str,
) -> anyhow::Result<Vec<ReplayedTx>> {
    let mut client = connect(laserstream_url, api_key)
        .await
        .context("laserstream replay: connect failed")?;

    let (req_tx, req_rx) = mpsc::channel::<SubscribeRequest>(REQUEST_QUEUE_CAP);
    let initial = build_subscribe_request(vec![account.to_string()], Some(from_slot));
    req_tx
        .send(initial)
        .await
        .map_err(|_| anyhow::anyhow!("laserstream replay: request channel closed"))?;

    let response = client
        .subscribe(ReceiverStream::new(req_rx))
        .await
        .context("laserstream replay: subscribe failed")?;
    let mut stream = response.into_inner();

    let mut out: Vec<ReplayedTx> = Vec::new();
    let mut got_any = false;

    let hard_deadline = tokio::time::sleep(HARD_DEADLINE);
    tokio::pin!(hard_deadline);

    loop {
        // Idle window resets every message: long for the first (cold connect),
        // short between messages (separates a replay burst from live silence).
        let idle = if got_any { IDLE_TIMEOUT } else { FIRST_MSG_TIMEOUT };

        tokio::select! {
            _ = &mut hard_deadline => {
                info!("laserstream replay: hit hard deadline, returning {} txs", out.len());
                break;
            }
            _ = tokio::time::sleep(idle) => {
                // Caught up (burst finished, or no new txs at all).
                break;
            }
            msg = stream.message() => {
                match msg {
                    Ok(Some(update)) => {
                        if let Some(UpdateOneof::Transaction(tx)) = update.update_oneof {
                            got_any = true;
                            if tx.slot >= from_slot && replay_is_relevant(&tx, pump_program_id) {
                                out.push(ReplayedTx { slot: tx.slot, update: tx });
                                if out.len() >= MAX_REPLAY_TXS {
                                    info!(
                                        "laserstream replay: hit {MAX_REPLAY_TXS}-tx cap, \
                                         returning partial (watermark advances to max slot \
                                         seen; next sync resumes from there)"
                                    );
                                    break;
                                }
                            }
                        }
                        // Non-transaction updates (ping/slot/etc.) ignored.
                    }
                    Ok(None) => break, // server closed the stream cleanly
                    Err(status) => {
                        return Err(anyhow::anyhow!("laserstream replay: stream error: {status}"));
                    }
                }
            }
        }
    }

    out.sort_by_key(|t| t.slot);
    Ok(out)
}

/// Cheap log pre-filter mirroring the decoder's gate: keep only frames the decoder
/// can decode (bonding-curve or PumpSwap), so we don't carry unrelated txs that
/// merely touched the account. `decode_protobuf` would drop them anyway.
fn replay_is_relevant(tx: &SubscribeUpdateTransaction, pump_program_id: &str) -> bool {
    tx.transaction
        .as_ref()
        .and_then(|i| i.meta.as_ref())
        .map(|m| {
            m.log_messages
                .iter()
                .any(|l| l.contains(pump_program_id) || l.contains(PUMP_SWAP_PROGRAM_ID))
        })
        .unwrap_or(false)
}
