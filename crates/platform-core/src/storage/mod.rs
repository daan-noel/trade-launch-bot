//! Storage layer.
//!
//! - `timescale` — continuous-aggregate boot (idempotent, run after `migrate!`).
//! - `postgres` (Phase 3) — workload-isolated pools (hot/api/batch) + `sqlx::migrate!`.
//! - `repositories` (Phase 3) — one repo per table, converting `amount_*` base-unit
//!   columns to human `_*` model fields at the boundary via [`crate::units`].

pub mod timescale;
