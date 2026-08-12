//! Deploy strategy handlers. The generic fingerprint+metrics engine owns rule CRUD
//! and lifecycle ([`engine`]); position reads run over the unified `StrategyRepo`
//! (`strategy_positions`) through one set of handlers ([`positions`]). The legacy
//! there are no per-strategy `rules` / `tpsl\{1,2\}_positions` modules.

pub mod action_progress;
// The durable arm ledger's reads. `engine::list_armed` is the live twin (in-RAM
// registry, no window); these page `strategy_arms` over a date range.
pub mod arms;
pub mod engine;
pub mod positions;
pub mod rule_readout;
