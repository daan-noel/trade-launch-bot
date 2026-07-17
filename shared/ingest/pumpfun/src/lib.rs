//! LaserStream (Yellowstone gRPC) pump.fun ingest venue — standalone drop-in.
//!
//! ```text
//! Ingest::builder()
//!     .endpoint(url)
//!     .api_key(key)
//!     .protocol(Protocol::pump_fun())
//!     .config(IngestConfig::default())
//!     .build()?
//!     .start()
//! -> (Receiver<IngestEvent>, IngestHandle)
//! ```
//!
//! The host owns all sinks (DB, cache, SSE, strategy, watchdog). The crate emits
//! decoded [`event::IngestEvent`]s out a bounded mpsc channel and never reads env.
//!
//! **Structure (Phase H):** the venue-agnostic engine — gRPC transport, reconnect
//! policy, the `IngestVenue` seam, and the generic `Ingest<V>` / `IngestHandle<V>`
//! session — lives in `ingest-core`. This crate supplies the pump.fun
//! [`venue::PumpFunVenue`] (classify + decode + pool derivation) and a
//! back-compat façade so `ingest_laserstream::{Ingest, IngestBuilder,
//! IngestHandle, …}` compile unchanged (zero consumer source edits).
// ── Venue-agnostic engine (owned by `ingest-core`) ─────────────────────────────
// Re-exported at their original paths so both external `ingest_laserstream::{proto,
// config, event, …}` and internal `crate::proto` / `crate::config` / `crate::error`
// references resolve unchanged.
pub use ingest_core::{config, error, event, proto, slot_anchor};

#[cfg(feature = "rpc-backfill")]
pub use ingest_core::backfill;
#[cfg(feature = "raw-tx")]
pub use ingest_core::raw_tx;

// ── Pump.fun venue (this crate) ────────────────────────────────────────────────
pub mod decode;
pub mod pool;
pub mod protocol;
pub mod transport;
pub mod venue;

pub use ingest_core::{Commitment, IngestConfig, IngestError, IngestEvent, PushHooks, Result};
pub use pool::PoolIndex;
pub use protocol::Protocol;
pub use venue::PumpFunVenue;

use std::sync::Arc;

use tokio::sync::mpsc;

// ── Session façade ──────────────────────────────────────────────────────────────
//
// `Ingest` is a thin newtype over `ingest_core::Ingest<PumpFunVenue>` (rather than
// a bare type alias) so the pre-split builder chain `Ingest::builder().endpoint()
// .api_key().protocol(Protocol::pump_fun()).config().build()` resolves unchanged —
// the pump-specific `.protocol()` shim maps onto the generic venue construction.

/// Back-compat control handle: the generic core handle bound to the pump venue.
pub type IngestHandle = ingest_core::IngestHandle<PumpFunVenue>;

/// Validated, ready-to-start pump.fun ingest session.
pub struct Ingest {
    inner: ingest_core::Ingest<PumpFunVenue>,
}

/// Fluent builder for [`Ingest`]. `.protocol(Protocol)` selects the pump.fun
/// venue; `.api_key(..)` becomes the transport's `x-token` auth.
#[derive(Default)]
pub struct IngestBuilder {
    endpoint: Option<String>,
    api_key: Option<String>,
    protocol: Option<Protocol>,
    config: Option<IngestConfig>,
    push_hooks: Option<PushHooks>,
}

impl IngestBuilder {
    pub fn endpoint(mut self, url: impl Into<String>) -> Self {
        self.endpoint = Some(url.into());
        self
    }

    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }

    pub fn protocol(mut self, p: Protocol) -> Self {
        self.protocol = Some(p);
        self
    }

    pub fn config(mut self, c: IngestConfig) -> Self {
        self.config = Some(c);
        self
    }

    /// Optional push-feed hooks (`blocks_meta` + watched-account updates on the
    /// same LaserStream subscription) — see [`PushHooks`]. Off by default.
    pub fn push_hooks(mut self, hooks: PushHooks) -> Self {
        self.push_hooks = Some(hooks);
        self
    }

    pub fn build(self) -> Result<Ingest> {
        let endpoint = self
            .endpoint
            .ok_or_else(|| IngestError::InvalidEndpoint("endpoint() not set".into()))?;
        let api_key = self
            .api_key
            .ok_or_else(|| IngestError::InvalidEndpoint("api_key() not set".into()))?;
        let config = self.config.unwrap_or_default();
        let protocol = self.protocol.unwrap_or_else(Protocol::pump_fun);

        let venue = Arc::new(PumpFunVenue::new(protocol, &config));
        let mut inner = ingest_core::Ingest::new(
            endpoint,
            ingest_core::Auth::XToken(api_key),
            venue,
            config,
        );
        if let Some(hooks) = self.push_hooks {
            inner = inner.with_push_hooks(hooks);
        }
        Ok(Ingest { inner })
    }
}

impl Ingest {
    pub fn builder() -> IngestBuilder {
        IngestBuilder::default()
    }

    /// Spawn the transport + decode tasks and return the event receiver + handle.
    ///
    /// `live` — initial live-mode state (pass `true` to start streaming
    ///   immediately; `false` pauses until `handle.set_live(true)` is called).
    pub fn start(self, live: bool) -> (mpsc::Receiver<IngestEvent>, IngestHandle) {
        self.inner.start(live)
    }
}
