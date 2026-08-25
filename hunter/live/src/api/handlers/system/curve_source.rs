//! Runtime switch for which transport carries bonding-curve traffic.
//!
//! Mirrors [`super::live_mode`]: persist the key first, then update the in-memory
//! settings snapshot. The ingest adapter watches that snapshot and calls
//! `IngestHandle::set_curve_source`, so the feed re-points with no restart — the
//! new source connects while the old one drains, and the dedupe ring absorbs the
//! overlap.
//!
//! AMM pool traffic is unaffected either way: it always rides the gRPC
//! subscription, whose filter is keyed on the pool PDAs this bot tracks, so open
//! positions never lose their feed across a switch.

use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use serde::{Deserialize, Serialize};

use crate::state::deploy_state::DeployState;
use trading_core::storage::repositories::settings_repo::keys;

/// The only accepted values, and what each means.
const GRPC: &str = "grpc";
const NATS: &str = "nats";

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CurveSourceResponse {
    /// The operator's persisted choice: `"grpc"` or `"nats"`.
    pub curve_source: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateCurveSourceRequest {
    pub curve_source: String,
}

pub async fn get_curve_source(state: web::Data<Arc<DeployState>>) -> impl Responder {
    HttpResponse::Ok().json(CurveSourceResponse {
        curve_source: state.settings().curve_source,
    })
}

/// Switch the curve feed.
///
/// Selecting `nats` with no `NATS_URL` configured is accepted and persisted, but
/// the ingest adapter keeps the curve on gRPC and logs a warning — the feed is
/// never pointed at a transport that cannot run.
pub async fn set_curve_source(
    state: web::Data<Arc<DeployState>>,
    req: web::Json<UpdateCurveSourceRequest>,
) -> impl Responder {
    let value = req.curve_source.trim().to_ascii_lowercase();
    if value != GRPC && value != NATS {
        return HttpResponse::BadRequest().json(ErrorBody {
            error: format!("curve_source must be \"{GRPC}\" or \"{NATS}\", got {value:?}"),
        });
    }

    // Persist first: a failed write must not leave the running feed on a source
    // the next boot would not restore.
    let repo = state.settings_repo();
    if let Err(e) = repo.set_one(&keys::CURVE_SOURCE, &value).await {
        return HttpResponse::InternalServerError().json(ErrorBody {
            error: format!("Failed to persist curve source: {e}"),
        });
    }
    // Publishing the snapshot is what actually moves the feed — the ingest
    // adapter is subscribed to it.
    state.modify_settings(|s| s.curve_source = value.clone());

    HttpResponse::Ok().json(CurveSourceResponse {
        curve_source: value,
    })
}
