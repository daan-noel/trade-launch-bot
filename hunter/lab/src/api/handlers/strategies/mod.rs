//! Local strategy handlers: grouped param-sweeps + the generic-engine simulate /
//! rule-CRUD edge (`engine`, `engine_crud`) and position reads.
//!
//! Two distinct position surfaces, deliberately named apart: [`positions`] pages a
//! *simulated* run's per-token rows (RAM/disk working set), while
//! [`live_positions`] pages the *traded* real/paper positions off the synced mirror.

pub mod engine;
pub mod engine_crud;
pub mod flow_discovery;
pub mod grouped_sweep;
pub mod live_positions;
pub mod positions;
