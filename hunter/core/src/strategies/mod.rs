//! Strategy **domain** layer (the trading-free decision logic shared by both
//! runtime edges). These modules compute *what* a strategy would do — buy-rule
//! matching, entry-fill resolution, the exit ladder, and the candidate-token
//! scan — without ever touching the trader, RPC, or the live runtime cache.
//! That keeps them usable by **both** crates: `live`'s live runner/execution
//! path re-exports them; `lab`'s sweep engine and backtest harness depend on
//! them directly. Single-source so the backtest's entry/exit model can never
//! drift from live (see the crate-split plan).
//!
//! tpsl1 and tpsl2 carry distinct decision logic (tpsl2 adds scalp-continuation
//! gates) and are intentional clones — a fix in one usually belongs in both.

pub mod analysis;
pub mod death;
pub mod exit_state;
pub mod fingerprint_axes;
pub mod kernel;
pub mod match_keys;
pub mod registry;
pub mod rules;
pub mod runtime_cache;
// Lives in the pure `hunter-engine` crate; re-exported for path stability.
pub use hunter_engine::rule_params;
pub mod swing_1;
pub mod tpsl_sniper_1;
pub mod tpsl_sniper_2;
