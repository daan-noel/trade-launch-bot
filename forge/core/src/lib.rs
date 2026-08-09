//! `platform-core` — the venue- and quote-asset-generalized data layer.
//!
//! One job: all tables + types + read/write for the launch platform. It carries
//! hunter's proven data-layer *ideas* (raw feed + typed projection, integer
//! base units, identity SSOT, interning, hypertables, JSONB brain, derived views)
//! but lifts the two SOL/pump.fun assumptions into first-class dimensions:
//!   * quote asset is a `quote_assets` row (native SOL is just `is_native`); amounts
//!     are `amount_quote` / `amount_base` base units — the unit is the *referenced
//!     asset*, not a hard-coded lamport.
//!   * launchpad + market kind are `launchpad_id` + `market_kind`, not `venue`.
//!
//! Depends on NO ingest / trader / lake crate — it is the shared base of both bins.

pub mod units;

// // Kept as declared-but-empty modules so the crate map is visible from commit 1.
pub mod config;
pub mod models;
pub mod storage;
pub mod venue;
