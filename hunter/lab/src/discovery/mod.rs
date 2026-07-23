//! Metric-combo **discovery** pipeline (lab-only) — the automated
//! screen → family-grid → validate flow that finds the metric/param combos worth
//! promoting for a token cohort. Plan: `docs/roadmap/metric-combo-discovery.md`.
//!
//! Built in layers, each independently useful:
//! * [`objective`] — the robust re-rank over persisted combo metrics (step 1).
//! * [`candidates`] — registry-driven screen plan + measured percentile ladders +
//!   the candidate value menus they generate (step 2).
//!
//! Everything here reads the metric [`REGISTRY`](hunter_engine::metrics) and the
//! already-persisted `ComboMetrics` columns, so a metric added later flows through
//! with no edit (extensibility contract, plan §5).

pub mod candidates;
pub mod objective;
