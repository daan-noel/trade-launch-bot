//! The venue seam — the trait that makes the session generic over a program family.
//!
//! `ingest-core` owns the reconnect policy, pre-filter plumbing and the neutral
//! [`crate::event::IngestEvent`] output; a *venue* (e.g. `ingest-pumpfun`'s
//! `PumpFunVenue`) plugs in the program-family-specific bits: which accounts to
//! subscribe to, how to classify a raw update, how to decode a relevant one, and
//! how to derive its pool PDAs. Static dispatch (`Ingest<V>`,
//! `supervisor::run<V, F>`) — no `Box<dyn>` on the hot path — mirrors the
//! write-side `executor-core` `Venue` seam.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use tokio::sync::Notify;

use crate::event::IngestEvent;
use crate::feed::StreamScope;
use crate::proto::geyser::SubscribeUpdateTransaction;

/// Shared pool address → base mint address index (venue-neutral).
///
/// - A venue's decode task inserts on pool discovery (auto-tracking).
/// - The host inserts known-active pools at warm-restart via
///   [`crate::IngestHandle::track_pools`].
/// - A feed supervisor reads it (through [`IngestVenue::subscription_accounts`])
///   to build the `account_include` filter.
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
/// every feed share exactly one `PoolIndex` instance — auto-discovered pools
/// become subscription accounts without any cross-task hand-off.
pub trait IngestVenue: Send + Sync + 'static {
    /// Venue-owned relevance tag carried opaquely from classify → decode (pump:
    /// `Curve | Amm`). Copy so a feed can forward it cheaply.
    type Relevance: Copy + Send + 'static;

    /// Transaction filter-map key — a label this venue owns (pump: `"pumpfun"`).
    fn filter_key(&self) -> &'static str;

    /// Accounts to place in a feed's `account_include` filter, for the slice of
    /// the venue that feed is carrying.
    ///
    /// [`StreamScope::program`] adds the venue's program id(s);
    /// [`StreamScope::pools`] adds every tracked pool PDA. A scope with
    /// `program` false must omit the program id(s) — another feed is carrying
    /// curve traffic, and leaving the program id in would pay the provider for
    /// it twice.
    ///
    /// An empty result means "nothing to watch": a server-filtered feed idles
    /// instead of subscribing, because an empty `account_include` matches *every*
    /// transaction on chain.
    fn subscription_accounts(&self, scope: StreamScope) -> Vec<String>;

    /// Cheap pre-filter over a raw update; `None` ⇒ ignore (do not forward to
    /// `decode`). Runs once, on the feed supervisor task.
    fn classify(&self, update: &SubscribeUpdateTransaction) -> Option<Self::Relevance>;

    /// Whether this relevance rides the **create fast lane** (dedicated
    /// feed→decode channel + decode task). Default `false` — all traffic shares
    /// the normal lane. Pump.fun overrides for `TxRelevance::Create` so AMM/curve
    /// swap volume can never delay a create decode.
    fn is_create_lane(relevance: Self::Relevance) -> bool {
        let _ = relevance;
        false
    }

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
    /// a server-filtered feed waits on it to resubscribe.
    fn pools_changed(&self) -> Arc<Notify>;
}
