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

use crate::pool::PoolIndex;
use crate::protocol::Protocol;

// `DecodeOutput` is the venue-neutral decode result; it lives in `ingest-core`
// (the `IngestVenue::decode` return type) and is re-exported here so the pump
// decoder modules keep referencing `super::DecodeOutput` unchanged.
pub use ingest_core::venue::DecodeOutput;

mod create;
mod grpc;
mod instructions;
mod program_registry;
mod trade;

pub use program_registry::program_friendly_name;

// ── Public types ──────────────────────────────────────────────────────────────

/// Which program family a tx matched (computed once, in the transport task).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TxRelevance {
    /// Bonding-curve tx that **creates** a token (`create` / `create_v2`).
    ///
    /// Decodes exactly like [`TxRelevance::Curve`] — the tag drives the create
    /// fast lane (`IngestVenue::is_create_lane` → dedicated transport→decode
    /// channel). A create that is missed here (and so arrives tagged `Curve`)
    /// costs a routing hint, never a decoded event: both arms run the same decode.
    Create,
    /// Bonding-curve (pump.fun program) tx.
    Curve,
    /// Post-migration PumpSwap (AMM) swap, resolved via the shared pool index.
    Amm,
}

impl TxRelevance {
    /// `true` for the bonding-curve family (`Create` and `Curve`) — the two tags
    /// that share one decode path. Keeps every `Create`-vs-`Curve` split in ONE
    /// place instead of re-matching at each call site.
    pub fn is_curve(self) -> bool {
        matches!(self, TxRelevance::Create | TxRelevance::Curve)
    }
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

    /// Classify a tx by log messages. **Backfill only** — the live transport
    /// pre-filter is [`Decoder::classify_accounts`], which reads the message
    /// instead of substring-scanning every log line. Kept for the backfill path
    /// (and as the parity reference the guard test measures the new classify
    /// against). Returns `None` when the tx is not relevant to any tracked
    /// program.
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
