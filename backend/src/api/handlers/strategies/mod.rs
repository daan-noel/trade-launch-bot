// `grouped_sweep` (local) moved to `backend-local`; re-export so existing
// `handlers::strategies::grouped_sweep::…` paths keep resolving.
pub use backend_local::api::handlers::strategies::grouped_sweep;
// The rule **domain** (`tpsl_rules_core`: request DTOs + validation + repo write)
// moved to `backend-core`; re-export so existing `super::tpsl_rules_core::…`
// paths in tpsl1/tpsl2 keep resolving.
pub use backend_core::api::handlers::strategies::tpsl_rules_core;
pub mod tpsl1;
pub mod tpsl2;

// The live position-read handlers moved to `backend-deploy`; re-export them so
// existing `handlers::strategies::tpsl1_positions::…` paths keep resolving.
pub use backend_deploy::api::handlers::strategies::{tpsl1_positions, tpsl2_positions};
