//! `tpsl_sniper_2` strategy **domain** modules (trading-free; clone of tpsl1 plus
//! the scalp-continuation entry gates):
//!   - `entry`  — scalp entry matching + worst-case paper-entry resolution.
//!   - `exit`   — the exit ladder (trade-driven + clock-driven).
//!   - `util`   — small shared `none_if_zero_*` helpers.
//!
//! The live runtime edge lives in `live` and re-exports these.

pub mod entry;
pub mod exit;
pub mod util;
