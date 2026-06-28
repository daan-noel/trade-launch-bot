//! Pump.fun transaction decoder — source-agnostic core.
//!
//! [`Decoder`] holds the pre-decoded protocol descriptor and the shared
//! pool→mint index. It exposes two entry points:
//! - [`Decoder::decode_protobuf`] — self-classifies then dispatches (used by
//!   backfill / token-sync paths).
//! - [`Decoder::decode_relevant_pb`] — the hot-path entry; the transport task
//!   pre-classified the tx so the log scan is not repeated here.

use std::sync::Arc;

use tokio::sync::Notify;

use crate::event::IngestEvent;
use crate::pool::PoolIndex;
use crate::protocol::Protocol;

mod create;
mod grpc;
mod instructions;
mod trade;

// ── Public types ──────────────────────────────────────────────────────────────

/// Which program family a tx's logs matched (compute-once in the transport task).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TxRelevance {
    /// Bonding-curve (pump.fun program) tx.
    Curve,
    /// Post-migration PumpSwap (AMM) swap, resolved via the shared pool index.
    Amm,
}

/// Outcome of decoding one LaserStream transaction update.
pub enum DecodeOutput {
    /// One or more events decoded; may include [`IngestEvent::RawTx`] when the
    /// `raw-json` feature is enabled.
    Events(Vec<IngestEvent>),
    /// Not relevant (other program, ping, parse failure, dust, etc.).
    Ignored,
}

// ── Decoder ───────────────────────────────────────────────────────────────────

/// Back-compat alias — the type was called `HeliusDecoder` in the v1 API.
pub type HeliusDecoder = Decoder;

/// Stateful pump.fun tx decoder. Holds the pre-decoded protocol descriptor
/// (program-ID bytes + discriminator bytes) and the shared pool→mint index
/// (for AMM swap attribution).
pub struct Decoder {
    pub(crate) protocol: Arc<Protocol>,
    /// Shared pool→mint index — `None` on paths that decode AMM trades via an
    /// explicit pool argument (backfill), set on the live path.
    pub(crate) pool_index: Option<PoolIndex>,
    /// Fires whenever a new pool is auto-registered (via `TokenMigrated`) so
    /// the transport task resubscribes with the updated pool set. `None` on
    /// backfill paths.
    pub(crate) pools_changed: Option<Arc<Notify>>,
}

impl Decoder {
    pub fn new(protocol: Arc<Protocol>) -> Self {
        Self {
            protocol,
            pool_index: None,
            pools_changed: None,
        }
    }

    /// Attach a shared pool→mint index, enabling live AMM-swap attribution.
    pub fn with_pool_index(mut self, index: PoolIndex) -> Self {
        self.pool_index = Some(index);
        self
    }

    /// Attach the Notify used to signal pool-set changes to the transport task.
    pub fn with_pools_changed(mut self, notify: Arc<Notify>) -> Self {
        self.pools_changed = Some(notify);
        self
    }

    /// Classify a tx by log messages (same logic as the transport pre-filter).
    /// Returns `None` when the tx is not relevant to any tracked program.
    pub(crate) fn classify_logs(&self, logs: &[String]) -> Option<TxRelevance> {
        let pump_id = &self.protocol.programs.pump_fun.base58;
        let swap_id = &self.protocol.programs.pump_swap.base58;

        if logs.iter().any(|l| l.contains(pump_id.as_str())) {
            Some(TxRelevance::Curve)
        } else if self.pool_index.is_some()
            && logs.iter().any(|l| l.contains(swap_id.as_str()))
        {
            Some(TxRelevance::Amm)
        } else {
            None
        }
    }
}
