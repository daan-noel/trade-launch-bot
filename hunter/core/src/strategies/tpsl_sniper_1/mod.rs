//! `tpsl_sniper_1` strategy **domain** modules (trading-free):
//!   - `entry` — buy-criteria matching + entry-fill resolution.
//!   - `exit`  — the single exit ladder (trade-driven + clock-driven).
//!   - `util`  — small shared `none_if_zero_*` helpers.
//!
//! The live runtime edge (`execution`/`handler`/`service`/`lifecycle`/
//! `paper_run`/`runtime_cache`) lives in `live` and re-exports these.

pub mod entry;
pub mod exit;
pub mod util;
