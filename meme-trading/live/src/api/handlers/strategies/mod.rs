//! Deploy strategy handlers. Position reads run over the unified `StrategyRepo`
//! (`strategy_positions`) through one set of handlers keyed by a `{strategy}`
//! path segment — see [`positions`]. The legacy per-strategy `tpsl{1,2}_positions`
//! modules were retired in Phase 3.

pub mod positions;
pub mod rules;
