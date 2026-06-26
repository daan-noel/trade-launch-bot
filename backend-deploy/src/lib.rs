//! `backend-deploy` library surface: the live-trading modules (strategies,
//! trader, deploy services/state, deploy API handlers, token-cache seed). The
//! `backend-deploy` binary (`main.rs`) is the composition root that wires these
//! together; `backend` (until deleted in T15) re-exports these modules so its
//! existing `crate::strategies::…` / `crate::trader::…` paths keep resolving.

pub mod api;
pub mod seed;
pub mod services;
pub mod state;
pub mod strategies;
pub mod trader;
