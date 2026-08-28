//! Pump.fun ingest venue — the decoders, the protocol descriptor, and the
//! assembly root that wires them onto whichever transports are configured.
//!
//! ```text
//! Ingest::builder()
//!     .endpoint(url)                 // Yellowstone gRPC endpoint
//!     .api_key(key)                  // becomes the x-token auth
//!     .nats(Some(nats_cfg))          // optional relay, feature `nats`
//!     .curve_feed(FeedKind::Nats)    // who carries the bonding curve
//!     .protocol(Protocol::pump_fun())
//!     .config(IngestConfig::default())
//!     .build()?
//!     .start(live)
//! -> (Receiver<IngestEvent>, IngestHandle)
//! ```
//!
//! The host owns all sinks (DB, cache, SSE, strategy, watchdog). This crate
//! emits decoded [`event::IngestEvent`]s out a bounded mpsc channel and never
//! reads env.
//!
//! **Structure — three axes, four crates.** `ingest-core` is the engine (the
//! `Feed` seam, the one reconnect/route supervisor, the `IngestVenue` seam, the
//! session, the event contract); `ingest-laserstream` and `ingest-nats` are one
//! wire each; this crate is the venue (classify + decode + pool derivation) plus
//! [`assembly`], the only module in the workspace that knows which wires exist.
//! A new transport is a fifth crate and one arm in `assembly` — nothing here or
//! in the engine moves.

// ── Venue-agnostic engine (owned by `ingest-core`) ─────────────────────────────
// Re-exported at their original paths so internal `crate::proto` / `crate::config`
// / `crate::error` references and consumer `ingest_pumpfun::{proto, event, …}`
// paths both resolve.
pub use ingest_core::{config, error, event, proto, slot_anchor};

#[cfg(feature = "rpc-backfill")]
pub use ingest_core::backfill;
#[cfg(feature = "raw-tx")]
pub use ingest_core::raw_tx;

// ── Pump.fun venue (this crate) ────────────────────────────────────────────────
pub mod assembly;
pub mod decode;
pub mod pool;
pub mod protocol;
pub mod venue;

pub use ingest_core::{
    Commitment, FeedCaps, IngestConfig, IngestError, IngestEvent, PushHooks, Result, StreamScope,
};

/// The Yellowstone gRPC feed. Consumers that open their own short-lived stream
/// (the one-shot replay service) reach `connect` / `build_subscribe_request`
/// through here.
pub use ingest_laserstream as laserstream;
pub use ingest_laserstream::{Auth, GrpcConfig};

/// The NATS relay feed (see `ingest_nats`). Only the host's probe/diagnostic
/// paths need this; the assembly root wires it internally.
#[cfg(feature = "nats")]
pub use ingest_nats as nats;
#[cfg(feature = "nats")]
pub use ingest_nats::NatsConfig;

pub use assembly::FeedKind;
pub use pool::PoolIndex;
pub use protocol::Protocol;
pub use venue::PumpFunVenue;

use std::sync::Arc;

use tokio::sync::{mpsc, watch};

// ── Session façade ──────────────────────────────────────────────────────────────

/// Validated, ready-to-start pump.fun ingest session.
pub struct Ingest {
    inner: ingest_core::Ingest<PumpFunVenue>,
    venue: Arc<PumpFunVenue>,
    config: IngestConfig,
    grpc: GrpcConfig,
    #[cfg(feature = "nats")]
    nats: Option<NatsConfig>,
    curve: FeedKind,
    feeds: usize,
}

/// Fluent builder for [`Ingest`]. `.protocol(Protocol)` selects the pump.fun
/// venue; `.api_key(..)` becomes the gRPC feed's `x-token` auth.
#[derive(Default)]
pub struct IngestBuilder {
    grpc: GrpcConfig,
    endpoint: Option<String>,
    api_key: Option<String>,
    #[cfg(feature = "nats")]
    nats: Option<NatsConfig>,
    curve: Option<FeedKind>,
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

    /// Replace the whole gRPC feed config. `endpoint`/`api_key` still override
    /// their two fields, so a host that only needs a provider swap can set just
    /// those.
    pub fn grpc(mut self, cfg: GrpcConfig) -> Self {
        self.grpc = cfg;
        self
    }

    /// Configure the NATS relay feed.
    ///
    /// Supplying it does **not** select it: the relay is spawned (idle,
    /// disconnected) whenever it is configured, which is what makes
    /// [`IngestHandle::set_curve_feed`] an instant switch instead of a restart.
    #[cfg(feature = "nats")]
    pub fn nats(mut self, cfg: Option<NatsConfig>) -> Self {
        self.nats = cfg;
        self
    }

    /// Which feed carries bonding-curve traffic. Default [`FeedKind::Grpc`] —
    /// the only wire that can replay a gap and the only one whose filter this bot
    /// controls. Changeable at runtime through [`IngestHandle::set_curve_feed`].
    pub fn curve_feed(mut self, kind: FeedKind) -> Self {
        self.curve = Some(kind);
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

    /// Optional push-feed hooks (block metas + watched-account updates on the
    /// same subscription) — see [`PushHooks`]. Off by default.
    pub fn push_hooks(mut self, hooks: PushHooks) -> Self {
        self.push_hooks = Some(hooks);
        self
    }

    pub fn build(mut self) -> Result<Ingest> {
        if let Some(url) = self.endpoint.take() {
            self.grpc.endpoint = url;
        }
        if let Some(key) = self.api_key.take() {
            self.grpc.auth = Auth::XToken(key);
        }
        if self.grpc.endpoint.is_empty() {
            return Err(IngestError::InvalidEndpoint("endpoint() not set".into()));
        }
        if matches!(self.grpc.auth, Auth::None) {
            return Err(IngestError::InvalidEndpoint("api_key() not set".into()));
        }

        let config = self.config.unwrap_or_default();
        let protocol = self.protocol.unwrap_or_else(Protocol::pump_fun);

        #[cfg(feature = "nats")]
        let nats_configured = self.nats.is_some();
        #[cfg(not(feature = "nats"))]
        let nats_configured = false;

        let curve = assembly::resolve_curve(
            self.curve.unwrap_or(FeedKind::Grpc),
            nats_configured,
        );

        let venue = Arc::new(PumpFunVenue::new(protocol, &config));
        let mut inner = ingest_core::Ingest::new(venue.clone(), config.clone());
        if let Some(hooks) = self.push_hooks {
            inner = inner.with_push_hooks(hooks);
        }

        Ok(Ingest {
            inner,
            venue,
            config,
            grpc: self.grpc,
            #[cfg(feature = "nats")]
            nats: self.nats,
            curve,
            feeds: assembly::feed_count(nats_configured),
        })
    }
}

impl Ingest {
    pub fn builder() -> IngestBuilder {
        IngestBuilder::default()
    }

    /// Spawn the decode lanes and one supervisor per configured feed, then return
    /// the event receiver + control handle.
    ///
    /// `live` — initial live-mode state (pass `true` to start streaming
    ///   immediately; `false` pauses until `handle.set_live(true)` is called).
    pub fn start(self, live: bool) -> (mpsc::Receiver<IngestEvent>, IngestHandle) {
        let (event_rx, core, lanes) = self.inner.start(live, self.feeds);
        let curve_tx = assembly::spawn_feeds(
            self.venue,
            lanes,
            &self.config,
            self.grpc,
            #[cfg(feature = "nats")]
            self.nats,
            self.curve,
        );
        (event_rx, IngestHandle { core, curve_tx })
    }
}

// ── Control handle ────────────────────────────────────────────────────────────

/// Control handle for a running pump.fun ingest session.
///
/// Wraps the engine's wire-neutral handle and adds the one thing only the
/// assembly root can answer: which feed carries the curve.
pub struct IngestHandle {
    core: ingest_core::IngestHandle<PumpFunVenue>,
    curve_tx: watch::Sender<FeedKind>,
}

impl IngestHandle {
    /// Pause (`false`) or resume (`true`) every feed.
    pub fn set_live(&self, live: bool) {
        self.core.set_live(live);
    }

    /// Current live-mode state (`true` = streaming, `false` = paused).
    pub fn is_live(&self) -> bool {
        self.core.is_live()
    }

    /// Push updated gap-replay settings. Takes effect on the next reconnect of
    /// any feed that can replay; a feed that cannot warns once and ignores it.
    pub fn set_gap_replay(&self, on: bool, max_window_secs: u64) {
        self.core.set_gap_replay(on, max_window_secs);
    }

    /// Move bonding-curve traffic to another feed.
    ///
    /// Takes effect immediately and without a restart: the newly-selected feed
    /// connects while the old one is still draining, the dedupe ring absorbs the
    /// overlap, and the gRPC subscription re-scopes in place (keeping AMM pools
    /// subscribed throughout, so open positions never lose their feed).
    ///
    /// Selecting a feed that was never configured is a no-op — there would be
    /// nothing to connect to.
    pub fn set_curve_feed(&self, kind: FeedKind) {
        let _ = self.curve_tx.send(kind);
    }

    /// Which feed is currently carrying bonding-curve traffic.
    pub fn curve_feed(&self) -> FeedKind {
        *self.curve_tx.borrow()
    }

    /// Register pool PDAs for the given mints and signal the feeds to
    /// resubscribe. Idempotent — already-registered mints are skipped.
    pub fn track_pools(&self, mints: &[String]) {
        self.core.track_pools(mints);
    }

    /// Remove the given mint pool PDAs from the subscription set.
    pub fn untrack_pools(&self, mints: &[String]) {
        self.core.untrack_pools(mints);
    }

    /// Direct access to the shared pool→mint index (for host backfill paths
    /// that need to query the live tracked set without going through the handle).
    pub fn pool_index(&self) -> PoolIndex {
        self.core.pool_index()
    }

    /// Notify handle — fires when a pool is added/removed (auto or manual).
    pub fn pools_changed(&self) -> Arc<tokio::sync::Notify> {
        self.core.pools_changed()
    }
}
