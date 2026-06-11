//! `tpsl_sniper_2` — a clone of `tpsl_sniper_1`, structurally identical but fully
//! separate (own DB tables `tpsl2_strategy_rules` / `tpsl2_real_positions` /
//! `tpsl2_paper_*`, own runtime cache, own API surface under `/strategies/tpsl2`).
//! It exists so the N1–N10 sniper features can be developed here without touching
//! the live `tpsl_sniper_1` strategy.
//!
//! Module map:
//!   - `entry`        — buy-criteria matching + entry-fill resolution.
//!   - `exit`         — the single exit ladder (trade-driven + clock-driven).
//!   - `handler`      — thin active-rule holder over `entry` (`check_buy_entry`).
//!   - `execution`    — real/paper trade execution (buy, sell, fill polling).
//!   - `backtest`     — the DB-backed backtest harness over `entry` + `exit`.
//!   - `service`      — live event/timer drivers wiring it all together.
//!   - `lifecycle`    — manual activate / pause / stop-and-close transitions.
//!   - `paper_run`    — paper-run completion lifecycle.
//!   - `runtime_cache`— in-memory rules/positions/run state.
pub mod backtest;
pub mod entry;
pub mod execution;
pub mod exit;
pub mod handler;
pub mod lifecycle;
pub mod paper_run;
pub mod runtime_cache;
pub mod service;
mod util;

pub use backtest::run_backtest;
pub use handler::TPSL2StrategyHandler;
pub use lifecycle::{activate_rule, pause_rule, stop_and_close_rule, PaperActivation};
pub use runtime_cache::Tpsl2RuntimeCache;
pub use service::Tpsl2StrategyService;
