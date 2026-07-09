//! lab HTTP handlers (the analysis-box surface): sweeps, swing
//! detection, analysis/creator reads, background-job control, and the tpsl
//! rule-authoring + backtest edge. Composed with `trading_core`'s core routes by
//! `configure_local_routes`.

pub mod strategies;
pub mod system;
pub mod tokens;
pub mod wallets;
