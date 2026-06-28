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
//!   - `lifecycle`    — manual activate / pause / stop-and-close transitions.
//!   - `paper_run`    — paper-run completion lifecycle.
//!   - `runtime_cache`— in-memory rules/positions/run state.
//!
//! The decision layer (`entry`/`exit`/`util`) moved to
//! `trading_core::strategies::tpsl_sniper_1`; it's re-exported here so this
//! crate's live modules keep resolving `super::{entry,exit,util}`.
pub use trading_core::strategies::tpsl_sniper_1::{entry, exit, util};
pub mod execution;
pub mod handler;
pub mod lifecycle;
pub mod paper_run;
pub mod runtime_cache;
pub mod service;

pub use handler::TPSL1StrategyHandler;
pub use lifecycle::{activate_rule, pause_rule, stop_and_close_rule, PaperActivation};
pub use runtime_cache::{ExitGuard, Tpsl1RuntimeCache};
pub use service::Tpsl1StrategyService;
