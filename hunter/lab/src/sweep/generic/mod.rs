//! Generic (redesigned-engine) grouped sweep — the precompute-then-scan sweep that
//! replaces the three per-strategy wrappers (plan §5.4–5.6).
//!
//! * [`axes`] — the registry-driven axes model + combo → `RuleParams` assembly.
//! * [`strategy`] — [`GenericSweepStrategy`], the [`Strategy`](crate::sweep::strategy::Strategy)
//!   impl doing the per-token precompute ([`MetricSeries`](hunter_engine::metrics::series::MetricSeries))
//!   and the per-combo scan, reusing the `grouped_engine` partition + persistence.
//! * `guard` (test-only) — the scan ≡ `run_replay` drift lock (step 5.5).

pub mod axes;
pub mod strategy;

#[cfg(test)]
mod guard;

pub use axes::{AxesModel, AxesRequest, AxisSide, AxisSpec, ResolvedAxis};
pub use strategy::GenericSweepStrategy;
