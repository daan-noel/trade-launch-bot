//! Backend-resident services. The shared clients/RPC/HTTP/price services live in
//! `backend-core::services`; this module re-exports them and keeps the
//! ingest/trade-specific services that depend on backend-only modules.

// `http` is used only by the core services (clients/helius_rpc); backend code
// doesn't reference it directly, so it's not re-exported here.
#[allow(unused_imports)]
pub use backend_core::services::{clients, helius_rpc, sol_price};

// The ingest/trade-specific services moved to `backend-deploy`; re-export them so
// existing `crate::services::…` paths keep resolving (path-stability re-exports;
// not all are consumed within `backend` itself any more).
#[allow(unused_imports)]
pub use backend_deploy::services::{laserstream_replay, token_sync, wallet_reconcile, wallet_tokens};
