//! `tpsl_sniper_2` — a clone of `tpsl_sniper_1`, structurally identical but fully
//! separate (own DB tables `strategy_TPSL2_rules` / `tpsl2_positions` /
//! `tpsl2_paper_*`, own runtime cache, own API surface under `/strategies/tpsl2`).
//! It exists so the N1–N10 sniper features can be developed here without touching
//! the live `tpsl_sniper_1` strategy.
pub mod handler_tpsl;
pub mod runtime_cache;
pub mod service_tpsl;
pub mod simulation_tpsl;
mod util;

pub use runtime_cache::Tpsl2RuntimeCache;
pub use handler_tpsl::TPSL2StrategyHandler;
pub use service_tpsl::Tpsl2StrategyService;
pub use simulation_tpsl::run_simulation;
