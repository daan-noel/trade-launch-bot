//! Venue-agnostic ingest engine — the shared read-side core.
//!
//! Phase G (leaf move): this crate owns the venue-neutral pieces the pump.fun
//! decoder + transport in `ingest-pumpfun` build on — the Yellowstone gRPC wire
//! types ([`proto`]), the host-facing [`event::IngestEvent`] contract, tunables
//! ([`config`]), slot→time estimation ([`slot_anchor`]), and the RPC-backfill /
//! raw-tx adapters. **No pump.fun coupling.**
//!
//! Phase H added the `IngestVenue` trait seam ([`venue`]), moved the gRPC
//! transport + reconnect loop here ([`transport`], generic over `V`), and the
//! [`Ingest<V>`] / [`IngestHandle<V>`] session ([`session`]). A venue crate
//! (`ingest-pumpfun`) supplies the classify/decode/pool-derivation impl; this
//! crate knows nothing about pump.fun.
#[allow(clippy::all, dead_code)]
#[rustfmt::skip]
pub mod proto {
    include!("generated/mod.rs");
}

pub mod config;
pub mod error;
pub mod event;
pub mod session;
pub mod slot_anchor;
pub mod transport;
pub mod venue;

#[cfg(feature = "rpc-backfill")]
pub mod backfill;

#[cfg(feature = "raw-tx")]
pub mod raw_tx;

pub use config::{Auth, Commitment, IngestConfig};
pub use error::{IngestError, Result};
pub use event::IngestEvent;
pub use session::{Ingest, IngestHandle};
pub use venue::{DecodeOutput, IngestVenue, PoolIndex};
