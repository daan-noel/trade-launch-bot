//! WebSocket ingest transport — **empty scaffold**.
//!
//! This crate exists so `live` can swap ingest transports later without
//! reshaping its wiring: it mirrors `ingest_laserstream::spawn`'s signature and
//! returns the same [`trading_core::ingest::IngestHandles`]. The wire protocol +
//! decoder are not implemented yet; [`spawn`] panics if called.
//!
//! When implemented, the decoded-event model + `pipeline`/`db_writer` can be
//! shared with `ingest-laserstream` (a later task); for now this is a stub.
#![allow(unused_variables)]

use std::sync::Arc;

use tokio::sync::{broadcast, watch};
use trading_core::{
    ingest::{IngestHandles, TraderHook},
    models::ingest::SseEvent,
    state::{token_cache::TokenCache, trade_signals::TradeSignals},
    storage::repositories::settings_repo::AppSettings,
};

/// Spawn the websocket ingest transport. Mirrors `ingest_laserstream::spawn` so
/// the two transports are interchangeable behind the `core::ingest` contract.
///
/// NOT YET IMPLEMENTED — panics if called.
#[allow(clippy::too_many_arguments)]
pub fn spawn(
    websocket_url: String,
    api_key: String,
    pump_program_id: String,
    db: sqlx::PgPool,
    token_cache: Arc<TokenCache>,
    sse_tx: broadcast::Sender<SseEvent>,
    settings_rx: watch::Receiver<AppSettings>,
    live_rx: watch::Receiver<bool>,
    trader: Arc<dyn TraderHook>,
    trade_signals: Arc<TradeSignals>,
) -> IngestHandles {
    unimplemented!("ingest-websocket transport is a scaffold; not yet implemented")
}
