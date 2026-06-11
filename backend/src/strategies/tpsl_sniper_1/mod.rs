//! `tpsl_sniper_1` — the live TPSL strategy (the canonical module; the older
//! `tpsl` fork was removed). The live service is dispatched by the runner; the
//! backtest path (`run_backtest`) is wired into the API and develops the
//! launch-sniper exit/entry features (see `@project_plans/@TPSL_plan`).
//!
//! Module map:
//!   - `entry`        — buy-criteria matching + entry-fill resolution.
//!   - `exit`         — the single exit ladder (trade-driven + clock-driven).
//!   - `handler`      — thin active-rule holder over `entry` (`check_buy_entry`).
//!   - `execution`    — real/paper trade execution (buy, sell, fill polling).
//!   - `backtest`     — the DB-backed backtest harness over `entry` + `exit`.
//!   - `service`      — live event/timer drivers wiring it all together.
//!   - `paper_run`    — paper-run completion lifecycle.
//!   - `runtime_cache`— in-memory rules/positions/run state.
pub mod backtest;
pub mod entry;
pub mod execution;
pub mod exit;
pub mod handler;
pub mod paper_run;
pub mod runtime_cache;
pub mod service;
mod util;

pub use backtest::run_backtest;
pub use handler::TPSL1StrategyHandler;
pub use runtime_cache::Tpsl1RuntimeCache;
pub use service::Tpsl1StrategyService;
