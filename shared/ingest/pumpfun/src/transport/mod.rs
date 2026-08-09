//! Back-compat façade over `ingest_core::transport`.
//!
//! The generic gRPC transport + reconnect loop lives in `ingest-core`
//! (`transport::run<V>`). This shim preserves the pre-split call surface used by
//! the standalone one-shot replay path
//! (`hunter/live/src/services/laserstream_replay.rs`), which opens its own
//! short-lived `Subscribe` stream outside the `Ingest` session: `connect(endpoint,
//! api_key, cfg)` (x-token / Helius) and `build_subscribe_request(accounts,
//! from_slot, commitment)` with the pump.fun filter key baked in.

pub use ingest_core::transport::{LaserStreamClient, TransportConfig, XTokenInterceptor};

use ingest_core::config::Auth;
use ingest_core::proto::geyser::{CommitmentLevel, SubscribeRequest};

/// Connect to a Yellowstone endpoint using an `x-token` (Helius) provider — the
/// pre-split signature. Wraps [`ingest_core::transport::connect`] with
/// [`Auth::XToken`].
pub async fn connect(
    endpoint: &str,
    api_key: &str,
    cfg: &TransportConfig,
) -> crate::Result<LaserStreamClient> {
    ingest_core::transport::connect(endpoint, &Auth::XToken(api_key.to_string()), cfg).await
}

/// Build a pump.fun transaction `Subscribe` request (filter-map key `"pumpfun"`).
/// Transactions only — the one-shot replay path has no use for the optional
/// push feeds (`blocks_meta` / accounts), which belong to the long-lived
/// `Ingest` session.
pub fn build_subscribe_request(
    account_include: Vec<String>,
    from_slot: Option<u64>,
    commitment: CommitmentLevel,
) -> SubscribeRequest {
    ingest_core::transport::build_subscribe_request(
        "pumpfun",
        account_include,
        from_slot,
        commitment,
        false,
        Vec::new(),
    )
}
