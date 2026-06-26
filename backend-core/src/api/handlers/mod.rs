//! Core HTTP handlers shared by every backend bin. Deploy-only (strategies,
//! trading, live-mode) and local-only (sweep, swing, jobs, analysis) handlers
//! live in their respective bin crates.

pub mod system;
pub mod tokens;
