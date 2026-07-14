//! Server-Sent Events push layer for the LIVE box (audit §1: "poll → push").
//!
//! Replaces the frontend↔backend double-poll: instead of the browser polling REST
//! every 3–5s while a background loop polls the same DB rows to advance state, the
//! ingest writer + the ingest toggle *push* events onto a `tokio::sync::broadcast`
//! bus, and a single `GET /api/stream` SSE endpoint fans them out to every
//! connected browser (one shared `EventSource`). Pollers stay only as slow
//! gap-heal fallbacks.
//!
//! Design vs hunter: hunter renders frames on a separate bridge task because its
//! `live_stats` read touches a shared `token_cache` that contends with the ingest
//! writer's `get_mut`. forge has no such cache, and its only high-frequency
//! publisher (the decoupled [`DbWriter`](crate::ingest::db_writer)) already runs
//! off the hot recv loop — so events render to wire bytes at publish time: one
//! `json!` per event, and only when a subscriber is actually connected
//! (`receiver_count() == 0` skips the render entirely, the common idle case where
//! no dashboard is open).

use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use futures_util::stream;
use ingest_laserstream::event::{TokenCreated as IlTokenCreated, Trade as IlTrade};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::broadcast;

use crate::ingest::map::trade_type_str;

/// Broadcast backlog per subscriber. A slow client that lags past this many unread
/// frames gets `Lagged` (skipped forward) rather than stalling the producer — live
/// data is refetchable, never worth blocking ingest for.
const CHANNEL_CAPACITY: usize = 1024;

/// One SSE event rendered to wire bytes exactly once and broadcast as
/// `Arc<SseFrame>`, so every subscriber clones a ref-counted buffer instead of
/// re-serializing per connection. `mint` carries the event scope for the cheap
/// per-subscriber `?mint_address=` filter (a string compare, no re-render):
///   * `Some(mint)` — deliver only to subscribers with no filter or a matching mint.
///   * `None`       — not mint-scoped (e.g. ingest status); deliver to everyone.
pub struct SseFrame {
    pub mint: Option<String>,
    pub bytes: web::Bytes,
}

/// The push bus. Cloned into `app_data` (for the `/api/stream` handler + the ingest
/// toggle) and into the `DbWriter` (for trade/token events). Cheap to clone — it's
/// just a `broadcast::Sender` (an `Arc` internally).
#[derive(Clone)]
pub struct SseHub {
    tx: broadcast::Sender<Arc<SseFrame>>,
}

impl SseHub {
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(CHANNEL_CAPACITY);
        Self { tx }
    }

    fn subscribe(&self) -> broadcast::Receiver<Arc<SseFrame>> {
        self.tx.subscribe()
    }

    /// `true` when at least one browser holds the stream open. Publishers check
    /// this before building a payload so the idle box does zero serialization.
    fn has_subscribers(&self) -> bool {
        self.tx.receiver_count() > 0
    }

    /// Render + broadcast one already-built payload. Assumes a subscriber exists
    /// (callers gate on [`has_subscribers`](Self::has_subscribers) before building
    /// `data`); the send is a no-op if the last receiver just went away.
    fn emit(&self, mint: Option<String>, event: &str, data: Value) {
        let frame = format!("event: {event}\ndata: {data}\n\n");
        let _ = self.tx.send(Arc::new(SseFrame {
            mint,
            bytes: web::Bytes::from(frame.into_bytes()),
        }));
    }

    /// A new trade landed for `mint`. Mint-scoped so a `?mint_address=` subscriber
    /// (the token-detail page) only wakes for its own token.
    pub fn trade_executed(&self, t: &IlTrade) {
        if !self.has_subscribers() {
            return;
        }
        self.emit(
            Some(t.mint.clone()),
            "trade_executed",
            json!({
                "mint_address": t.mint,
                "trade_type": trade_type_str(t.side),
                "amount_sol": t.sol,
                "token_amount": t.tokens,
                "slot": t.slot,
                "tx_signature": t.signature,
            }),
        );
    }

    /// A new token was created (ingest first saw its create). A near-bare signal —
    /// the launches/tokens lists refetch their current page off it.
    pub fn token_created(&self, tc: &IlTokenCreated) {
        if !self.has_subscribers() {
            return;
        }
        self.emit(
            Some(tc.mint.clone()),
            "token_created",
            json!({
                "mint_address": tc.mint,
                "name": tc.name,
                "symbol": tc.symbol,
                "slot": tc.slot,
            }),
        );
    }

    /// Ingest was paused/resumed (operator toggle). Not mint-scoped — every
    /// dashboard shows the badge, so it goes to all subscribers.
    pub fn ingest_status(&self, live: bool) {
        if !self.has_subscribers() {
            return;
        }
        self.emit(None, "ingest_status", json!({ "live": live }));
    }

    /// A launch reached a terminal status (created / failed / partial) — pushed
    /// from the confirm watcher so the Launch Console updates without polling
    /// `/api/launches/{id}/status`. Mint-scoped for symmetry with the trade
    /// stream; the Launch Console (an unfiltered subscriber) matches on
    /// `launch_id`.
    fn launch_status(&self, ev: &launcher::LaunchStatusEvent) {
        if !self.has_subscribers() {
            return;
        }
        self.emit(
            Some(ev.mint_address.clone()),
            "launch_status",
            json!({
                "launch_id": ev.launch_id,
                "mint_address": ev.mint_address,
                "launch_status": ev.launch_status,
                "bundle_id": ev.bundle_id,
                "bundle_status": ev.bundle_status,
            }),
        );
    }

    /// The managed-wallet pool changed (a funding pass moved SOL / promoted
    /// wallets). A bare, not-mint-scoped signal — the Wallet Pool page refetches
    /// `GET /api/wallet_pool` off it.
    fn wallet_pool_changed(&self) {
        if !self.has_subscribers() {
            return;
        }
        self.emit(None, "wallet_pool", json!({}));
    }

    /// Progress of the keystore-restore job (`POST /api/wallet_pool/restore`): the
    /// current `phase` (`wallets` | `backfill` | `positions`) plus a `done`/`total`
    /// counter so the Wallet Pool page can render a progress bar as the long
    /// backfill loop advances. `total = 0` means "indeterminate" (a phase whose size
    /// isn't known yet). Not mint-scoped — the restore panel on every open dashboard
    /// gets it.
    pub fn restore_progress(&self, phase: &str, done: usize, total: usize) {
        if !self.has_subscribers() {
            return;
        }
        self.emit(
            None,
            "restore_progress",
            json!({ "phase": phase, "done": done, "total": total }),
        );
    }

    /// Terminal frame for a keystore-restore run — carries the full `RestoreSummary`
    /// so the page shows the final counts (wallets / trades / launches / mints /
    /// positions) without a refetch. Not mint-scoped.
    pub fn restore_complete(&self, summary: &Value) {
        if !self.has_subscribers() {
            return;
        }
        self.emit(None, "restore_complete", summary.clone());
    }
}

/// Bridges the `SseHub` to the launcher's [`launcher::EventSink`] seam, so the
/// background confirm watcher + wallet funder (in the `launcher` crate, which can't
/// see this bin's SSE layer) push launch/funding events onto the same bus.
impl launcher::EventSink for SseHub {
    fn launch_status(&self, ev: &launcher::LaunchStatusEvent) {
        SseHub::launch_status(self, ev);
    }
    fn wallet_pool_changed(&self) {
        SseHub::wallet_pool_changed(self);
    }
}

impl Default for SseHub {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Deserialize)]
pub struct StreamQuery {
    pub mint_address: Option<String>,
}

/// `GET /api/stream[?mint_address=<addr>]` — the single SSE endpoint every browser
/// shares. Streams `event: <type>\ndata: <json>\n\n` frames. A `?mint_address=`
/// filters mint-scoped frames to that token; non-scoped frames (e.g.
/// `ingest_status`) always pass. GET, so the fail-closed bearer gate lets it
/// through as a safe read.
pub async fn stream_events(
    hub: web::Data<SseHub>,
    query: web::Query<StreamQuery>,
) -> impl Responder {
    let rx = hub.subscribe();
    let mint_filter = query.into_inner().mint_address;

    let sse_stream = stream::unfold((rx, mint_filter), |(mut rx, mint_filter)| async move {
        loop {
            match rx.recv().await {
                Ok(frame) => {
                    // Mint-scoped frame against a mint filter: cheap string compare,
                    // no re-render. Non-scoped frames (mint == None) always pass.
                    if let (Some(scope), Some(want)) = (&frame.mint, &mint_filter) {
                        if scope != want {
                            continue;
                        }
                    }
                    return Some((
                        Ok::<_, actix_web::Error>(frame.bytes.clone()),
                        (rx, mint_filter),
                    ));
                }
                // Slow consumer fell behind: skip forward instead of stalling.
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    });

    HttpResponse::Ok()
        .content_type("text/event-stream")
        .insert_header(("Cache-Control", "no-cache"))
        // Defensive: tell any nginx reverse proxy not to buffer the stream.
        .insert_header(("X-Accel-Buffering", "no"))
        .streaming(sse_stream)
}
