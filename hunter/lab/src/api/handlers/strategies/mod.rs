//! Local strategy handlers: grouped param-sweeps + the tpsl1/tpsl2 **local
//! runtime edge** (rule CRUD without live-cache reload, the detached backtest,
//! paper-result reads). The rule domain (`tpsl_rules_core`) lives in
//! `trading_core` and is shared with the deploy edge.

pub mod engine;
pub mod grouped_sweep;
pub mod positions;
pub mod swing1;
pub mod tpsl1;
pub mod tpsl2;
