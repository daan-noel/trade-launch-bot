//! Strategy **domain** layer (the trading-free decision logic shared by both
//! runtime edges). These modules compute *what* a strategy would do — fingerprint
//! matching, entry-fill resolution, the shared cost/summary kernel — without ever
//! touching the trader, RPC, or a live runtime cache. That keeps them usable by
//! **both** crates: `live`'s generic engine and `lab`'s sweep/replay harness. The
//! legacy per-strategy tpsl1/tpsl2/swing1 decision stack was retired in Phase 7;
//! only the generic fingerprint+metrics engine (`hunter-engine`) remains.

pub mod analysis;
pub mod death;
pub mod fingerprint_axes;
pub mod kernel;
pub mod rules;
// Lives in the pure `hunter-engine` crate; re-exported for path stability.
pub use hunter_engine::rule_params;
