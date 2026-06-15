//! Pump.fun transaction decoder.
//!
//! Two decode paths, one per byte source, kept as parity-tested siblings:
//! - [`grpc`] — protobuf-native decode of the live LaserStream feed (the hot path);
//! - [`json`] — the Helius `jsonParsed` `Value` decode (RPC token_sync / replay,
//!   and the off-thread persisted-blob synthesis).
//!
//! Everything source-agnostic lives at this root and is shared by both paths:
//! the [`HeliusDecoder`] struct + [`DecodeOutput`], `decode_migrate`, and the
//! byte/log leaves in [`create`], [`trade`], and [`instructions`]. A fix to a
//! shared leaf lands once; a fix to a path-specific step usually belongs in both
//! `grpc` and `json` (the parity tests in `grpc` guard the two stay identical).

use std::sync::Arc;

use chrono::{DateTime, Utc};
use dashmap::DashMap;

use crate::models::{
    events::{InternalEvent, TokenMigratedEvent},
    transaction::RawTransaction,
};

mod create;
mod grpc;
mod instructions;
mod json;
mod trade;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub struct HeliusDecoder {
    pump_program_id: String,
    /// Shared pool→mint index for resolving live PumpSwap (AMM) swaps, whose
    /// events carry the pool but not the base mint. `None` in contexts that
    /// decode AMM trades with an explicit pool already in hand (token sync).
    pool_index: Option<Arc<DashMap<String, String>>>,
}

/// Outcome of decoding one LaserStream transaction update.
pub enum DecodeOutput {
    /// A Pump.fun transaction was decoded successfully.
    Transaction {
        /// Shared so each embedded event clones a pointer, not the full JSON.
        raw_tx: Arc<RawTransaction>,
        events: Vec<InternalEvent>,
    },
    /// Message was not relevant (other program, ping, unrecognised format, etc.)
    Ignored,
}

impl HeliusDecoder {
    pub fn new(pump_program_id: String) -> Self {
        Self {
            pump_program_id,
            pool_index: None,
        }
    }

    /// Attach a shared pool→mint index, enabling the live decode path to
    /// recognise post-migration PumpSwap (AMM) swaps and attribute them back to
    /// the tracked mint that owns the pool.
    pub fn with_pool_index(mut self, index: Arc<DashMap<String, String>>) -> Self {
        self.pool_index = Some(index);
        self
    }

    /// Build a `TokenMigrated` event from a Migrate instruction's accounts.
    /// Shared by both decode paths (the mint is `accounts[2]`).
    pub(super) fn decode_migrate(
        &self,
        signature: &str,
        slot: u64,
        block_time: DateTime<Utc>,
        pump_accounts: &[String],
        raw_tx: &Arc<RawTransaction>,
    ) -> Option<InternalEvent> {
        let mint = pump_accounts.get(2)?.to_string();
        if mint.is_empty() {
            return None;
        }

        Some(InternalEvent::TokenMigrated(TokenMigratedEvent {
            mint_address: mint,
            tx_signature: signature.to_string(),
            slot,
            timestamp: block_time,
            raw_tx: raw_tx.clone(),
        }))
    }
}
