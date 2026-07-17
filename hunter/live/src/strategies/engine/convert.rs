//! DB model ↔ engine type converters (plan 4.x). These are the shared SSOT in
//! `trading_core::strategies::fingerprint_axes` so the live engine and the lab
//! replay driver feed the engine identical fingerprints + parsed rules + observed
//! axes — a rule prices the same live or replayed (redesign parity, decision 6).
//! Re-exported here for path stability with the rest of the live engine adapter.

pub use trading_core::strategies::fingerprint_axes::{fp_to_engine, observed_axes, rule_to_loaded};
