//! Deploy strategy handlers. The generic fingerprint+metrics engine owns rule CRUD
//! and lifecycle ([`engine`]); position reads run over the unified `StrategyRepo`
//! (`strategy_positions`) through one set of handlers ([`positions`]). The legacy
//! there are no per-strategy `rules` / `tpsl\{1,2\}_positions` modules.

pub mod action_progress;
pub mod engine;
pub mod positions;
pub mod rule_readout;
