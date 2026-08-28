//! Venue-agnostic ingest **engine** — the shared read-side core.
//!
//! Owns everything that is neither a wire nor a program family: the transaction
//! wire types ([`proto`], messages only — no gRPC client), the [`feed`] seam,
//! the one reconnect/route loop ([`supervisor`]), the [`venue`] seam, the
//! host-facing [`event::IngestEvent`] contract, the generic [`Ingest<V>`]
//! session, tunables ([`config`]), slot→time estimation ([`slot_anchor`]), the
//! single JSON→protobuf adapter ([`convert`]), and the RPC-backfill / raw-tx
//! paths.
//!
//! **Three prohibitions, and they are the whole design.** Nothing here knows a
//! wire (that is `ingest-laserstream` / `ingest-nats`, each implementing
//! [`feed::Feed`]), nothing here knows a venue (that is `ingest-pumpfun`,
//! implementing [`venue::IngestVenue`]), and nothing here reads env (the host
//! builds [`IngestConfig`]). Adding a transport touches no file in this crate.
#[allow(clippy::all, dead_code)]
#[rustfmt::skip]
pub mod proto {
    include!("generated/mod.rs");
}

pub mod config;
pub mod dedupe;
pub mod error;
pub mod event;
pub mod feed;
pub mod push;
pub mod session;
pub mod slot_anchor;
pub mod supervisor;
pub mod venue;

/// Shared JSON -> protobuf adapter for every non-gRPC transaction source
/// (RPC backfill, NATS relay, WebSocket).
#[cfg(feature = "json-tx")]
pub mod convert;

#[cfg(feature = "rpc-backfill")]
pub mod backfill;

#[cfg(feature = "raw-tx")]
pub mod raw_tx;

pub use config::{Commitment, IngestConfig};
pub use dedupe::SignatureDedupe;
pub use error::{IngestError, Result};
pub use event::IngestEvent;
pub use feed::{Feed, FeedCaps, FeedConn, FeedError, FeedUpdate, StreamScope, Subscription};
pub use push::PushHooks;
pub use session::{FeedLanes, Ingest, IngestHandle};
pub use supervisor::FeedPolicy;
pub use venue::{DecodeOutput, IngestVenue, PoolIndex};
