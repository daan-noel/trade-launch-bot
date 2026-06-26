//! Backend system handlers shim: re-exports the core system handlers (SSE,
//! settings, price, profiles/wallets/tags), the deploy live-mode handlers, and the
//! local job status/cancel/result handlers (now in `backend-local`), so
//! `backend`'s paths resolve unchanged until the combined bin is deleted (T15).

pub use backend_core::api::handlers::system::*;

// Deploy live-mode.
pub use backend_deploy::api::handlers::system::*;

// Local background-job control.
pub use backend_local::api::handlers::system::{
    cancel_simulation, cancel_swing, job_status, simulation_result, swing_result,
};
