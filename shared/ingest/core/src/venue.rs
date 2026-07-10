//! The venue seam — the trait that makes the transport/session generic.
//!
//! `ingest-core` owns the connection mechanics, reconnect policy, pre-filter
//! plumbing and the neutral [`crate::event::IngestEvent`] output; a *venue*
//! (e.g. `ingest-pumpfun`'s `PumpFunVenue`) plugs in the program-family-specific
//! bits: which accounts to subscribe to, how to classify a raw update, how to
//! decode a relevant one, and how to derive its pool PDAs. Static dispatch
//! (`Ingest<V>`, `transport::run<V>`) — no `Box<dyn>` on the hot path — mirrors
//! the write-side `executor-core` `Venue` seam.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use tokio::sync::Notify;

use crate::event::IngestEvent;
use crate::proto::geyser::SubscribeUpdateTransaction;

/// Shared pool address → base mint address index (venue-neutral).
///
/// - A venue's decode task inserts on pool discovery (auto-tracking).
/// - The host inserts known-active pools at warm-restart via
///   [`crate::IngestHandle::track_pools`].
/// - The transport task reads it (through [`IngestVenue::subscription_accounts`])
///   to build the gRPC `account_include` filter.
pub type PoolIndex = Arc<DashMap<String, String>>;

/// Outcome of decoding one transaction update.
pub enum DecodeOutput {
    /// One or more events decoded; may include [`IngestEvent::RawTx`] when the
    /// `raw-tx` feature is enabled.
    Events(Vec<IngestEvent>),
    /// Not relevant (other program, ping, parse failure, dust, etc.).
    Ignored,
}

/// One venue = one program family's classify + decode + pool derivation.
///
/// The venue owns its pool state (index + resubscribe signal) so its decoder and
/// the transport share exactly one `PoolIndex` instance — auto-discovered pools
/// become subscription accounts without any cross-task hand-off.
pub trait IngestVenue: Send + Sync + 'static {
    /// Venue-owned relevance tag carried opaquely from classify → decode (pump:
    /// `Curve | Amm`). Copy so the transport can forward it cheaply.
    type Relevance: Copy + Send + 'static;

    /// gRPC transaction-filter map key (was the hardcoded `"pumpfun"`).
    fn filter_key(&self) -> &'static str;

    /// Accounts to place in the gRPC `account_include` filter — the venue's
    /// program id(s) plus every tracked pool PDA (read from its `PoolIndex`).
    fn subscription_accounts(&self) -> Vec<String>;

    /// Cheap pre-filter over a raw update (log scan); `None` ⇒ ignore (do not
    /// forward to `decode`). Runs once, in the transport task.
    fn classify(&self, update: &SubscribeUpdateTransaction) -> Option<Self::Relevance>;

    /// Full decode of a pre-classified update into neutral events.
    fn decode(
        &self,
        update: &SubscribeUpdateTransaction,
        relevance: Self::Relevance,
        received_at: DateTime<Utc>,
    ) -> DecodeOutput;

    /// Pool PDA for a mint (for `IngestHandle::track_pools`); `None` if the venue
    /// has no pools or the mint is unparseable.
    fn derive_pool(&self, mint: &str) -> Option<String>;

    /// The shared pool→mint index (same `Arc` the venue's decoder holds).
    fn pool_index(&self) -> PoolIndex;

    /// Signal fired when the pool set changes (auto-discovery or `track_pools`);
    /// the transport waits on it to resubscribe.
    fn pools_changed(&self) -> Arc<Notify>;
}
