//! Storage layer.
//!
//! - `postgres`     — workload-isolated pools (hot/api/batch) + `sqlx::migrate!` + cagg boot.
//! - `timescale`    — continuous-aggregate boot (idempotent, run after `migrate!`).
//! - `repositories` — one repo per table; converts `amount_*` base-unit columns to
//!   human values with the quote/base decimals via [`crate::units`] at the boundary.

pub mod postgres;
pub mod repositories;
pub mod timescale;

pub use postgres::{connect, DbPools};
