//! Metric-combo **discovery** pipeline (lab-only) — the automated
//! screen → family-grid → validate flow that finds the metric/param combos worth
//! promoting for a token cohort. Plan: `docs/roadmap/metric-combo-discovery.md`.
//!
//! Built in layers, each independently useful:
//! * [`objective`] — the robust re-rank over persisted combo metrics (step 1).
//! * [`candidates`] — registry-driven screen plan + measured percentile ladders +
//!   the candidate value menus they generate (step 2).
//! * [`additive`] — the shared scan mode: N sub-models as one flat combo space over
//!   ONE per-token precompute (the pipeline's dominant performance lever).
//! * [`screen`] — Layer 1: the additive per-metric scan, its response curves, and
//!   the ranked metric shortlist (step 3).
//! * [`family`] — Layer 2: a grid per registry metric family over Layer 1's narrowed
//!   ranges, plus the pairwise interaction check (step 4).
//!
//! Everything here reads the metric [`REGISTRY`](hunter_engine::metrics) and the
//! already-persisted `ComboMetrics` columns, so a metric added later flows through
//! with no edit (extensibility contract, plan §5).

pub mod additive;
pub mod candidates;
pub mod family;
pub mod objective;
pub mod screen;

#[cfg(test)]
pub(crate) mod fixtures;
