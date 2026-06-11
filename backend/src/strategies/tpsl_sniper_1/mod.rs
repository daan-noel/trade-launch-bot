//! `tpsl_sniper_1` — the live TPSL strategy (the canonical module; the older
//! `tpsl` fork was removed). The live service is dispatched by the runner; the
//! simulation path (`run_simulation` → `simulate_exit`) is wired into the API
//! and develops the launch-sniper exit/entry features (see
//! `@project_plans/@TPSL_plan`).
pub mod handler_tpsl;
pub mod runtime_cache;
pub mod service_tpsl;
pub mod simulation_tpsl;
mod util;

pub use runtime_cache::Tpsl1RuntimeCache;
pub use handler_tpsl::TPSL1StrategyHandler;
pub use service_tpsl::Tpsl1StrategyService;
pub use simulation_tpsl::run_simulation;
