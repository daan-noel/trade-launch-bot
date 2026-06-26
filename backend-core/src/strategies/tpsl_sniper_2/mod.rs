//! `tpsl_sniper_2` strategy **domain** modules (trading-free; clone of tpsl1 plus
//! the scalp-continuation entry gates + cohort features):
//!   - `entry`  — scalp entry matching + worst-case paper-entry resolution.
//!   - `exit`   — the exit ladder (trade-driven + clock-driven).
//!   - `cohort` — scalp cohort detection / per-prefix feature oracle.
//!   - `util`   — small shared `none_if_zero_*` helpers.
//!
//! The live runtime edge lives in `backend-deploy` and re-exports these.

pub mod cohort;
pub mod entry;
pub mod exit;
pub mod util;
