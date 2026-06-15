//! Strategy param-sweep & backtest engine.
//!
//! A backtest is a pure function `simulate(trades, params) -> TokenOutcome`; a
//! sweep loads the corpus **once** and calls `simulate` over combos × tokens in
//! memory (never touching the DB in the loop), then folds the outcomes to one
//! ranked metrics row per param-pair combo. Those rows persist to `sweep_results`
//! and the dashboard's per-strategy sweep page reads them.
//!
//! Layering (one responsibility per module, strategy-agnostic by design):
//!   - [`corpus`]    — `CorpusSource` (cache | DB) → compact `TokenTrades`,
//!                     cached to Parquet by corpus hash.
//!   - [`strategy`]  — `Strategy` + `ParamSpace` traits + `TokenOutcome` and the
//!                     frictionless `round_trip` — the only surface a new strategy adds.
//!   - [`tpsl2`]     — first `Strategy` impl, wrapping tpsl2's live pure fns.
//!   - [`tpsl1`]     — peer `Strategy` impl (clone-parity).
//!   - [`sweep`]     — `rayon` over tokens (combos inner) → folded `ComboAgg`s.
//!   - [`aggregate`] — `ComboAgg`/`ComboMetrics`: outcomes → one ranked row per combo.
//!   - [`cli`]       — `backend -- sweep …` entrypoint (sweep → persist).
//!
//! Decision parity: each strategy's `simulate` calls the *same* pure fns the live
//! path uses, so backtest and real trading resolve identical entry/exit decisions.
//! PnL is frictionless for now (pure price ratio — see [`strategy::round_trip`]).

pub mod aggregate;
pub mod cli;
pub mod corpus;
pub mod strategy;
pub mod sweep;
pub mod tpsl1;
pub mod tpsl2;
