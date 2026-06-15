//! TPSL2 param-sweep & backtest engine.
//!
//! A backtest is a pure function `simulate(trades, params) -> TokenOutcome`; a
//! sweep loads the corpus **once** and calls `simulate` over combos × tokens in
//! memory (never touching the DB in the loop), then folds the outcomes to one
//! ranked metrics row per param-pair combo. Those rows persist to
//! `tpsl2_sweep_results` and the dashboard's TPSL2 sweep page reads them.
//!
//! TPSL2-specific by design — other strategies get their own separate sweep
//! module (clone-and-tweak this one). Module layout, one responsibility each:
//!   - [`corpus`]         — `CorpusSource` (cache | DB) → compact `TokenTrades`,
//!                          cached to Parquet by corpus hash.
//!   - [`strategy`]       — `Strategy` + `ParamSpace` traits + `TokenOutcome` and
//!                          the frictionless `round_trip`.
//!   - [`engine`]         — `rayon` over tokens (combos inner) → folded `ComboAgg`s.
//!   - [`aggregate`]      — `ComboAgg`/`ComboMetrics`: outcomes → one ranked row per combo.
//!   - [`tpsl2_strategy`] — the TPSL2 `Strategy` impl, wrapping tpsl2's live pure fns.
//!   - [`cli`]            — `backend -- tpsl2-sweep …` entrypoint (sweep → persist).
//!
//! Decision parity: `simulate` calls the *same* pure fns the live path uses, so
//! backtest and real trading resolve identical entry/exit decisions. PnL is
//! frictionless for now (pure price ratio — see [`strategy::round_trip`]).

pub mod aggregate;
pub mod cli;
pub mod corpus;
pub mod engine;
pub mod strategy;
pub mod tpsl2_strategy;
