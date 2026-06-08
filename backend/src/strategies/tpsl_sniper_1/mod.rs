//! `tpsl_sniper_1` — forked TPSL strategy used to develop the launch-sniper
//! exit/entry features (see `@project_plans/@TPSL_plan`). The simulation path
//! (`run_simulation` → `simulate_exit`) is wired into the API; the live
//! service is carried along from the fork and not yet dispatched by the runner.
pub mod handler_tpsl;
pub mod runtime_cache;
pub mod service_tpsl;
pub mod simulation_tpsl;
mod util;

#[allow(unused_imports)]
pub use runtime_cache::TpslRuntimeCache;
#[allow(unused_imports)]
pub use handler_tpsl::{ExitReason, TPSLStrategyHandler};
#[allow(unused_imports)]
pub use service_tpsl::TpslStrategyService;
pub use simulation_tpsl::run_simulation;
