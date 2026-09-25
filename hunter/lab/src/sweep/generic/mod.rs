//! The grouped sweep over the rule engine — precompute each token once, then scan
//! every combo over it.
//!
//! * [`axes`] — the swept dimensions and combo → rule assembly.
//! * [`strategy`] — [`GenericSweepStrategy`], the [`Strategy`](crate::sweep::strategy::Strategy)
//!   impl: per-token precompute ([`MetricSeries`](hunter_engine::metrics::series::MetricSeries)),
//!   the axes grid, the Pass-2 stage overlay.
//! * [`scan`] — the per-combo walk: the engine's entry and held-side decisions over a
//!   series row.
//! * [`fast_exit`] — first-exit-row queries for a flat held side (index, AVX-512).
//! * [`frozen_tail`] — clock decisions past a token's own series cut.
//! * `guard` (test-only) — the scan ≡ `run_replay` drift lock.

pub mod axes;
pub mod exit_index;
pub mod fast_exit;
pub mod frozen_tail;
pub mod scan;
pub mod strategy;

#[cfg(test)]
mod guard;

pub use axes::{AxesModel, AxesRequest, AxisSide, AxisSpec, ResolvedAxis};
pub use strategy::{GenericSweepStrategy, Pricing};
