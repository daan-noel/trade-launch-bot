//! Venue-agnostic ingest engine — the shared read-side core.
//!
//! Phase G (leaf move): this crate owns the venue-neutral pieces the pump.fun
//! decoder + transport in `ingest-pumpfun` build on — the Yellowstone gRPC wire
//! types ([`proto`]), the host-facing [`event::IngestEvent`] contract, tunables
//! ([`config`]), slot→time estimation ([`slot_anchor`]), and the RPC-backfill /
//! raw-tx adapters. **No pump.fun coupling.**
//!
//! The `IngestVenue` trait seam + the generic transport/`Ingest` session land in
//! Phase H (see `docs/ingest-redesign-plan.md`); for now the transport and the
//! `Ingest` builder still live venue-side in `ingest-pumpfun`.
#[allow(clippy::all, dead_code)]
#[rustfmt::skip]
pub mod proto {
    include!("generated/mod.rs");
}

pub mod config;
pub mod error;
pub mod event;
pub mod slot_anchor;

#[cfg(feature = "rpc-backfill")]
pub mod backfill;

#[cfg(feature = "raw-tx")]
pub mod raw_tx;

pub use config::{Commitment, IngestConfig};
pub use error::{IngestError, Result};
pub use event::IngestEvent;
