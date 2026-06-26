//! Pump.fun transaction decoder.
//!
//! One protobuf-native decode path, [`grpc`]'s `decode_protobuf`,
//! shared by both the live LaserStream feed and token_sync (which lowers its RPC
//! results to the same `SubscribeUpdateTransaction` via
//! [`super::adapter_rpc::rpc_to_protobuf`]). Source-agnostic pieces live at this
//! root: the [`HeliusDecoder`] struct + [`DecodeOutput`], `decode_migrate`, and
//! the byte/log leaves in [`create`], [`trade`], and [`instructions`].

use std::sync::Arc;

use chrono::{DateTime, Utc};
use dashmap::DashMap;

use backend_core::models::{
    events::{InternalEvent, TokenMigratedEvent},
    transaction::RawTransaction,
};

mod create;
mod grpc;
mod instructions;
mod trade;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub struct HeliusDecoder {
    pump_program_id: String,
    /// Raw 32-byte form of `pump_program_id`, decoded once. Lets the protobuf
    /// decode path match instruction program-ids on bytes instead of base58-
    /// encoding the whole account-key list per transaction.
    pump_program_id_bytes: Vec<u8>,
    /// Shared pool→mint index for resolving live PumpSwap (AMM) swaps, whose
    /// events carry the pool but not the base mint. `None` in contexts that
    /// decode AMM trades with an explicit pool already in hand (token sync).
    pool_index: Option<Arc<DashMap<String, String>>>,
}

/// Which program family a tx's logs matched. The LaserStream client computes
/// this once in its pre-filter and forwards it to the decoder so the per-tx log
/// scan that classifies curve-vs-AMM isn't repeated on the hot path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TxRelevance {
    /// Bonding-curve (pump.fun program) tx — curve takes priority over AMM.
    Curve,
    /// Post-migration PumpSwap (AMM) swap, resolved to a tracked mint via the
    /// shared pool index.
    Amm,
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
        // A startup invariant, not a valid operating state: an undecodable program
        // id would leave the raw-byte key matches silently failing for every tx.
        // Fail loudly at construction instead of degrading the hot path forever.
        let pump_program_id_bytes = bs58::decode(&pump_program_id)
            .into_vec()
            .expect("pump_program_id must be valid base58");
        Self {
            pump_program_id,
            pump_program_id_bytes,
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
