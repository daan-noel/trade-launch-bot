//! Core HTTP handlers shared by every backend bin. Deploy-only (live strategy
//! lifecycle/positions, trading, live-mode) and local-only (sweep, swing, jobs,
//! analysis) handlers live in their respective bin crates. The trading-free
//! strategy rule **domain** (`strategies::tpsl_rules_core`) lives here so both
//! the deploy and local CRUD edges share one source of validation + write logic.

pub mod strategies;
pub mod system;
pub mod tokens;
