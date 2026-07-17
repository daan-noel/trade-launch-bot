//! Strategy-agnostic param-sweep & backtest engine.
//!
//! A backtest is a pure function `simulate(trades, params) -> TokenOutcome`; a
//! sweep loads the corpus **once** and calls `simulate` over combos × tokens in
//! memory (never touching the DB in the loop), then folds the outcomes to one
//! ranked metrics row per param-pair combo.
//!
//! This module owns everything that is **not** strategy-specific, so a new
//! strategy (e.g. Swing Detection) only adds a [`strategy::Strategy`] +
//! [`strategy::ParamSpace`] impl under [`strategies`] — the corpus, grouping,
//! engine, aggregate, and persistence layers are reused verbatim. Module layout,
//! one responsibility each:
//! - [`corpus`] — `CorpusSource` (cache | DB) → `CorpusToken`, each token
//!   projected once into a slim, wallet-interned [`projection::CorpusTrade`] buffer
//!   (carrying a grouping [`grouping::TokenFingerprint`]), cached to Parquet by
//!   corpus hash.
//! - [`projection`] — `CorpusTrade`, the slim row the hot loop walks; the shared
//!   entry/exit fns are generic over `TradeRow` so they serve it and the live
//!   `Trade` from one implementation.
//! - [`strategy`] — `Strategy` + `ParamSpace` traits + `TokenOutcome`; re-exports
//!   the core `CostModel`/`round_trip_with_costs`/`ExitCode`.
//! - [`engine`] — `rayon` over tokens (combos inner) → folded `ComboAgg`s.
//! - [`aggregate`] — `ComboAgg` (a thin wrapper over the core kernel's `RunAgg`) →
//!   `ComboMetrics`: outcomes → one ranked row per combo.
//! - [`grouping`] — token fingerprint → exact-value `GroupKey` (strategy-agnostic).
//! - [`grouped_engine`] — partition the corpus by group key, run [`engine`] per group.
//! - [`registry`] — `strategy_id` → builds the concrete `Strategy` + axes.
//! - [`strategies`] — the per-strategy `Strategy` impls (tpsl2, …).
//!
//! Decision parity: `simulate` calls the *same* pure fns the live path uses (now
//! generic over `TradeRow`, monomorphized per row type), so backtest and real
//! trading resolve identical entry/exit decisions. PnL is priced through the core
//! [`CostModel`](strategy::CostModel) so the table reflects live frictions.

pub mod aggregate;
pub mod corpus;
pub mod engine;
pub mod generic;
pub mod grouped_engine;
// `grouping` is strategy-blind shared data (TokenFingerprint/GroupField/GroupKey)
// that a core handler + core repo also need, so it lives in `trading_core`; this
// re-export keeps the `crate::sweep::grouping::…` paths working.
pub use trading_core::grouping;
pub mod obs;
pub mod progress;
pub mod projection;
pub mod registry;
pub mod retention;
pub mod shard;
pub mod spill;
pub mod strategies;
pub mod strategy;
